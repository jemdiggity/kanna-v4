use crate::pty::{Adopted, Spawned, Spawner, Spec};
use crate::Event;
use kanna_keeper::client::{Client, Exited, SpawnRequest};
use std::cell::RefCell;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

const REPLY_TIMEOUT: Duration = Duration::from_secs(10);

pub struct Keeper {
    client: RefCell<Client>,
    pid: i32,
    received: RefCell<Vec<Exited>>,
    events: Sender<Event>,
    lost: RefCell<Option<String>>,
    resume_watch: Sender<()>,
    watch_resumed: RefCell<Option<Receiver<()>>>,
}

pub fn binary() -> PathBuf {
    if let Some(bin) = std::env::var_os("KANNA_KEEPER").filter(|v| !v.is_empty()) {
        return PathBuf::from(bin);
    }
    let beside_daemon = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|dir| dir.join("kanna-keeper")));
    beside_daemon.unwrap_or_else(|| PathBuf::from("kanna-keeper"))
}

impl Keeper {
    pub fn connect(state_dir: &Path, bin: &Path, events: Sender<Event>) -> io::Result<Keeper> {
        let mut client = Client::connect_or_start(state_dir, bin)?;
        client.set_timeout(Some(REPLY_TIMEOUT))?;
        let (version, pid) = client.hello()?;
        if version != kanna_keeper::PROTOCOL_VERSION {
            return Err(io::Error::other(format!(
                "the keeper running for {} (pid {pid}) speaks protocol {version}, this daemon speaks {}; stop that keeper (its processes go with it) or run a matching daemon",
                state_dir.display(),
                kanna_keeper::PROTOCOL_VERSION
            )));
        }
        let (resume_watch, watch_resumed) = mpsc::channel();
        Ok(Keeper {
            client: RefCell::new(client),
            pid,
            received: RefCell::default(),
            events,
            lost: RefCell::new(None),
            resume_watch,
            watch_resumed: RefCell::new(Some(watch_resumed)),
        })
    }

    pub fn pid(&self) -> i32 {
        self.pid
    }

    pub fn watch(&self) -> io::Result<()> {
        let resumed = self.watch_resumed.borrow_mut().take().ok_or_else(|| io::Error::other("already watched"))?;
        let fd = unsafe { libc::fcntl(self.client.borrow().as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        let events = self.events.clone();
        std::thread::Builder::new().name("kanna.keeper".into()).spawn(move || loop {
            let mut pfd = libc::pollfd { fd: fd.as_raw_fd(), events: libc::POLLIN, revents: 0 };
            if unsafe { libc::poll(&mut pfd, 1, -1) } <= 0 {
                continue;
            }
            if events.send(Event::KeeperReadable).is_err() || resumed.recv().is_err() {
                return;
            }
        })?;
        Ok(())
    }

    pub fn resume_watch(&self) {
        let _ = self.resume_watch.send(());
    }

    pub fn take_exits(&self) -> Result<Vec<Exited>, String> {
        if let Some(reason) = self.lost.borrow().clone() {
            return Err(reason);
        }
        let polled = self.client.borrow_mut().poll_events();
        let mut exits = std::mem::take(&mut *self.received.borrow_mut());
        match polled {
            Ok(more) => {
                exits.extend(more);
                Ok(exits)
            }
            Err(error) => Err(self.lose(&error)),
        }
    }

    fn lose(&self, error: &io::Error) -> String {
        let reason = match error.kind() {
            io::ErrorKind::UnexpectedEof => format!("the keeper (pid {}) went away", self.pid),
            _ => format!("the keeper (pid {}) failed: {error}", self.pid),
        };
        self.lost.borrow_mut().get_or_insert(reason).clone()
    }

    fn collect_received(&self) {
        let polled = self.client.borrow_mut().poll_events();
        match polled {
            Ok(exits) if !exits.is_empty() => {
                self.received.borrow_mut().extend(exits);
                let _ = self.events.send(Event::KeeperExits);
            }
            Ok(_) => {}
            Err(error) => self.report_lost(&error),
        }
    }

    fn report_lost(&self, error: &io::Error) {
        let first = self.lost.borrow().is_none();
        let reason = self.lose(error);
        if first {
            let _ = self.events.send(Event::KeeperLost(reason));
        }
    }

    fn request<T>(&self, call: impl FnOnce(&mut Client) -> io::Result<T>) -> io::Result<T> {
        if let Some(reason) = self.lost.borrow().clone() {
            return Err(io::Error::new(io::ErrorKind::NotConnected, reason));
        }
        let result = call(&mut self.client.borrow_mut());
        match &result {
            Err(error) if is_connection_error(error) => self.report_lost(error),
            _ => self.collect_received(),
        }
        result
    }
}

fn is_connection_error(error: &io::Error) -> bool {
    use io::ErrorKind::*;
    matches!(error.kind(), UnexpectedEof | BrokenPipe | ConnectionReset | ConnectionAborted | NotConnected | TimedOut | WouldBlock)
}

impl Spawner for Keeper {
    fn spawn(&self, spec: &Spec) -> io::Result<Spawned> {
        let mut request = match &spec.command {
            Some(command) => SpawnRequest::command(&spec.id, command),
            None => SpawnRequest::shell(&spec.id),
        };
        request.skip_rc = spec.hands_over;
        request.cwd = spec.cwd.as_ref().map(PathBuf::from);
        request.env = spec.env.iter().cloned().collect();
        (request.cols, request.rows) = (spec.cols, spec.rows);
        let pty = self.request(|client| client.spawn(&request))?;
        Ok(Spawned { pid: pty.pid, master: pty.master })
    }

    fn close(&self, id: &str) {
        if let Err(error) = self.request(|client| client.close(id)) {
            eprintln!("kanna-daemon: the keeper couldn't close {id}: {error}");
        }
    }

    fn acknowledge_exit(&self, id: &str) {
        let _ = self.request(|client| client.ack(id));
    }

    fn adopt(&self) -> io::Result<Vec<Adopted>> {
        let ptys = self.request(|client| client.adopt())?;
        Ok(ptys
            .into_iter()
            .map(|pty| Adopted { id: pty.id, pid: pty.pid, cmd: pty.cmd, cwd: pty.cwd, master: pty.master, exit: pty.exit.map(|status| status.shell_status()) })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;

    #[test]
    fn a_keeper_speaking_another_protocol_is_refused() {
        let dir = std::env::temp_dir().join(format!("kdk{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let listener = UnixListener::bind(dir.join("keeper.sock")).unwrap();
        let fake = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            let req: serde_json::Value = serde_json::from_str(&line).unwrap();
            let reply = serde_json::json!({"ok": true, "version": 999, "pid": 4242, "req": req["req"]});
            (&stream).write_all(format!("{reply}\n").as_bytes()).unwrap();
        });
        let (events, _rx) = mpsc::channel();
        let error = Keeper::connect(&dir, Path::new("/nonexistent/kanna-keeper"), events).err().expect("refused");
        let message = error.to_string();
        assert!(message.contains("999") && message.contains(&kanna_keeper::PROTOCOL_VERSION.to_string()), "{message}");
        assert!(message.contains("4242"), "{message}");
        fake.join().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
