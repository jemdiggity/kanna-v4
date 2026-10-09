use crate::screen::{Screen, TextScreen};
use crate::Event;
use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::panic::AssertUnwindSafe;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

pub const REPLAY: usize = 1 << 20;

const OUTBOX: usize = 1 << 20;

const QUIET: Duration = Duration::from_millis(50);

const DRAIN_LIMIT: Duration = Duration::from_millis(500);

pub struct Feed {
    screen: Box<dyn Screen + Send>,
    replay: VecDeque<u8>,
    outbox: Vec<u8>,
    overflowed: bool,
    posted: bool,
    pub cols: u16,
    pub rows: u16,
}

pub enum Taken {
    Bytes(Vec<u8>),
    Overflowed,
}

impl Feed {
    pub fn new(cols: u16, rows: u16) -> Arc<Mutex<Feed>> {
        Arc::new(Mutex::new(Feed {
            screen: Box::new(TextScreen::new(cols, rows)),
            replay: VecDeque::new(),
            outbox: Vec::new(),
            overflowed: false,
            posted: false,
            cols,
            rows,
        }))
    }

    pub fn lock(feed: &Mutex<Feed>) -> MutexGuard<'_, Feed> {
        feed.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn push(&mut self, id: &str, bytes: &[u8]) -> bool {
        self.guarded(id, |screen| screen.feed(bytes));
        self.replay.extend(bytes);
        let over = self.replay.len().saturating_sub(REPLAY);
        self.replay.drain(..over);
        if self.overflowed || self.outbox.len() + bytes.len() > OUTBOX {
            self.outbox = Vec::new();
            self.overflowed = true;
        } else {
            self.outbox.extend_from_slice(bytes);
        }
        !std::mem::replace(&mut self.posted, true)
    }

    pub fn take(&mut self) -> Option<Taken> {
        self.posted = false;
        if std::mem::take(&mut self.overflowed) {
            return Some(Taken::Overflowed);
        }
        let bytes = std::mem::take(&mut self.outbox);
        (!bytes.is_empty()).then_some(Taken::Bytes(bytes))
    }

    pub fn snapshot(&mut self) -> Vec<u8> {
        self.outbox = Vec::new();
        self.overflowed = false;
        self.replay.iter().copied().collect()
    }

    pub fn resize(&mut self, id: &str, cols: u16, rows: u16) {
        (self.cols, self.rows) = (cols, rows);
        self.guarded(id, |screen| screen.resize(cols, rows));
    }

    pub fn guarded<R>(&mut self, id: &str, f: impl FnOnce(&mut dyn Screen) -> R) -> Option<R> {
        match std::panic::catch_unwind(AssertUnwindSafe(|| f(self.screen.as_mut()))) {
            Ok(result) => Some(result),
            Err(_) => {
                eprintln!("kanna-daemon: {id}'s screen failed; it starts over");
                self.screen = Box::new(TextScreen::new(self.cols, self.rows));
                None
            }
        }
    }
}

pub struct Reader {
    wake: File,
}

impl Reader {
    pub fn start(id: &str, serial: u64, master: OwnedFd, feed: Arc<Mutex<Feed>>, events: Sender<Event>) -> io::Result<Reader> {
        let (asleep, wake) = pipe()?;
        let pane = id.to_string();
        std::thread::Builder::new()
            .name(format!("kanna.read.{id}"))
            .spawn(move || read(Pty { pane, serial, master: File::from(master), wake: File::from(asleep), feed, events }))?;
        Ok(Reader { wake: File::from(wake) })
    }

    pub fn drain(&self) {
        let _ = (&self.wake).write(&[1]);
    }
}

struct Pty {
    pane: String,
    serial: u64,
    master: File,
    wake: File,
    feed: Arc<Mutex<Feed>>,
    events: Sender<Event>,
}

impl Pty {
    fn drained(&self) -> bool {
        self.events.send(Event::Drained { pane: self.pane.clone(), serial: self.serial }).is_ok()
    }
}

fn read(mut pty: Pty) {
    let mut buffer = vec![0u8; 65536];
    let mut draining: Option<Instant> = None;
    loop {
        let mut fds = [
            libc::pollfd { fd: pty.master.as_raw_fd(), events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: pty.wake.as_raw_fd(), events: libc::POLLIN, revents: 0 },
        ];
        let timeout = if draining.is_some() { QUIET.as_millis() as libc::c_int } else { -1 };
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), 2, timeout) };
        if ready < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return;
        }
        if fds[1].revents != 0 {
            match pty.wake.read(&mut [0u8; 64]) {
                Ok(1..) => {
                    draining.get_or_insert_with(|| Instant::now() + DRAIN_LIMIT);
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                _ => return,
            }
            continue;
        }
        if draining.is_some_and(|limit| ready == 0 || Instant::now() >= limit) {
            draining = None;
            if !pty.drained() {
                return;
            }
            continue;
        }
        if fds[0].revents == 0 {
            continue;
        }
        let n = match pty.master.read(&mut buffer) {
            Ok(n @ 1..) => n,
            Err(error) if matches!(error.kind(), io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock) => continue,
            _ => {
                pty.drained();
                return;
            }
        };
        let post = Feed::lock(&pty.feed).push(&pty.pane, &buffer[..n]);
        if post && pty.events.send(Event::Output { pane: pty.pane.clone(), serial: pty.serial }).is_err() {
            return;
        }
    }
}

fn pipe() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } < 0 {
        return Err(io::Error::last_os_error());
    }
    let (read, write) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    for fd in [&read, &write] {
        unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) };
    }
    unsafe { libc::fcntl(write.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) };
    Ok((read, write))
}
