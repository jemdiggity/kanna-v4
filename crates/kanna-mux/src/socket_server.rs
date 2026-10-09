use crate::error::KmuxError;
use crate::json::{encode, json, Value};
use std::io::{BufRead, BufReader};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;

/// Answers one request. It may answer later, from any thread; the server's
/// connection thread waits for it. The embedder passes the request to the
/// core's thread (`Core::handle`) and sends the reply back.
pub type Handler = Arc<dyn Fn(Value, Box<dyn FnOnce(Value) + Send>) + Send + Sync>;

/// Takes a connection over after its first request, for a client that stays
/// connected and talks its own way from then on (Kanna's app). It gets that
/// request, the reader (which may hold lines already read) and the stream to
/// write to, and runs on the connection's thread for as long as it likes.
pub type Takeover = Arc<dyn Fn(Value, BufReader<UnixStream>, UnixStream) + Send + Sync>;

/// The control socket: a Unix domain socket carrying newline-delimited JSON.
/// Each connection is served on its own thread; requests on a connection are
/// answered in order.
pub struct SocketServer {
    pub path: String,
    handler: Handler,
    takeover: Option<(String, Takeover)>,
    running: Mutex<Option<Running>>,
}

struct Running {
    identity: Option<(u64, u64)>,
    wake: UnixStream,
    thread: JoinHandle<()>,
}

impl SocketServer {
    pub fn new(path: &str, handler: Handler) -> SocketServer {
        SocketServer { path: path.into(), handler, takeover: None, running: Mutex::new(None) }
    }

    /// A connection whose first request is `cmd` goes to `takeover` instead.
    /// Set before `start`.
    pub fn take_over(&mut self, cmd: &str, takeover: Takeover) {
        self.takeover = Some((cmd.into(), takeover));
    }

    pub fn start(&self) -> Result<(), KmuxError> {
        let mut running = self.running.lock().unwrap();
        if running.is_some() {
            return Err(KmuxError::new("bad_request", format!("already listening on {}", self.path)));
        }
        // SAFETY: sockaddr_un is plain data; all zeroes is a valid value.
        let address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        if self.path.len() >= address.sun_path.len() {
            return Err(KmuxError::new("bad_request", format!("socket path is too long: {}", self.path)));
        }
        if let Some(folder) = Path::new(&self.path).parent() {
            std::fs::create_dir_all(folder).map_err(|e| self.posix_error("mkdir", &e))?;
        }
        if SocketServer::is_listening(&self.path) {
            return Err(KmuxError::new("bad_request", format!("another kmux is already listening on {}", self.path)));
        }
        let _ = std::fs::remove_file(&self.path);
        let listener = UnixListener::bind(&self.path).map_err(|e| self.posix_error("bind", &e))?;
        let identity = identity_of(&self.path);
        let unbind = |error: KmuxError| {
            if identity_of(&self.path) == identity {
                let _ = std::fs::remove_file(&self.path);
            }
            error
        };
        let _ = std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600));
        listener.set_nonblocking(true).map_err(|e| unbind(self.posix_error("fcntl", &e)))?;
        let (wake, woken) = UnixStream::pair().map_err(|e| unbind(self.posix_error("socketpair", &e)))?;

        let handler = self.handler.clone();
        let takeover = self.takeover.clone();
        let thread = std::thread::Builder::new()
            .name("kmux.socket".into())
            .spawn(move || accept_loop(listener, woken, handler, takeover))
            .map_err(|e| unbind(self.posix_error("thread", &e)))?;
        *running = Some(Running { identity, wake, thread });
        Ok(())
    }

    pub fn stop(&self) {
        let Some(running) = self.running.lock().unwrap().take() else { return };
        let _ = running.wake.shutdown(Shutdown::Both);
        drop(running.wake);
        let _ = running.thread.join();
        if running.identity.is_some() && identity_of(&self.path) == running.identity {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    pub fn is_listening(path: &str) -> bool {
        UnixStream::connect(path).is_ok()
    }

    fn posix_error(&self, call: &str, error: &std::io::Error) -> KmuxError {
        KmuxError::new("bad_request", format!("{call} failed on {}: {}", self.path, crate::state_file::os_message(error)))
    }
}

fn identity_of(path: &str) -> Option<(u64, u64)> {
    std::fs::symlink_metadata(path).ok().map(|m| (m.dev(), m.ino()))
}

impl Drop for SocketServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn accept_loop(listener: UnixListener, woken: UnixStream, handler: Handler, takeover: Option<(String, Takeover)>) {
    let mut fds = [
        libc::pollfd { fd: listener.as_raw_fd(), events: libc::POLLIN, revents: 0 },
        libc::pollfd { fd: woken.as_raw_fd(), events: libc::POLLIN, revents: 0 },
    ];
    loop {
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) };
        if ready < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return;
        }
        if fds[1].revents != 0 {
            return;
        }
        loop {
            let client = match listener.accept() {
                Ok((client, _)) => client,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    break;
                }
            };
            if client.set_nonblocking(false).is_err() {
                continue;
            }
            no_sigpipe(&client);
            let (handler, takeover) = (handler.clone(), takeover.clone());
            let _ = std::thread::Builder::new().name("kmux.client".into()).spawn(move || serve(client, handler, takeover));
        }
    }
}

fn serve(client: UnixStream, handler: Handler, takeover: Option<(String, Takeover)>) {
    let Ok(writer) = client.try_clone() else { return };
    let mut reader = BufReader::with_capacity(65536, client);
    let mut line = Vec::new();
    let mut first = true;
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        // A last line without a newline is never answered.
        if line.last() != Some(&b'\n') {
            return;
        }
        line.pop();
        if line.iter().all(|b| *b == b' ' || *b == b'\r') {
            continue;
        }
        if let Some((cmd, takeover)) = takeover.as_ref().filter(|_| std::mem::take(&mut first)) {
            let request = serde_json::from_slice::<Value>(&line).ok();
            if let Some(request) = request.filter(|r| r.get("cmd").and_then(Value::as_str) == Some(cmd)) {
                return takeover(request, reader, writer);
            }
        }
        let Some(reply) = respond(&line, &handler) else { return };
        let mut out = encode(&reply).into_bytes();
        out.push(b'\n');
        if !write_all(&writer, &out) {
            return;
        }
    }
}

fn respond(line: &[u8], handler: &Handler) -> Option<Value> {
    let Ok(request) = serde_json::from_slice::<Value>(line) else {
        return Some(json!({ "id": null, "ok": false, "error": { "code": "bad_request", "message": "request is not valid JSON" } }));
    };
    let (send, receive) = mpsc::channel();
    handler(request, Box::new(move |reply| {
        let _ = send.send(reply);
    }));
    receive.recv().ok()
}

#[cfg(target_vendor = "apple")]
fn no_sigpipe(client: &UnixStream) {
    let on: libc::c_int = 1;
    // SAFETY: a valid socket and option value.
    unsafe {
        libc::setsockopt(client.as_raw_fd(), libc::SOL_SOCKET, libc::SO_NOSIGPIPE, &on as *const _ as *const libc::c_void, std::mem::size_of::<libc::c_int>() as libc::socklen_t);
    }
}

#[cfg(not(target_vendor = "apple"))]
fn no_sigpipe(_: &UnixStream) {}

/// Writes everything, without SIGPIPE if the client has gone.
pub fn write_all(stream: &UnixStream, data: &[u8]) -> bool {
    #[cfg(target_vendor = "apple")]
    const FLAGS: libc::c_int = 0;
    #[cfg(not(target_vendor = "apple"))]
    const FLAGS: libc::c_int = libc::MSG_NOSIGNAL;
    let mut offset = 0;
    while offset < data.len() {
        // SAFETY: the pointer and length are within `data`.
        let n = unsafe { libc::send(stream.as_raw_fd(), data[offset..].as_ptr() as *const libc::c_void, data.len() - offset, FLAGS) };
        if n < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return false;
        }
        offset += n as usize;
    }
    true
}
