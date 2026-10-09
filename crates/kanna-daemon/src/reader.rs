use crate::Event;
use kanna_vt::{Lines, Terminal};
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

pub const SCROLLBACK_LINES: usize = 10_000;

pub const SCROLLBACK_BYTES: usize = 10_000_000;

const OUTBOX: usize = 1 << 20;

const QUIET: Duration = Duration::from_millis(50);

const DRAIN_LIMIT: Duration = Duration::from_millis(500);

pub struct Feed {
    screen: Terminal,
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
    pub fn new(cols: u16, rows: u16) -> io::Result<Arc<Mutex<Feed>>> {
        let mut screen = Terminal::new(cols, rows, SCROLLBACK_LINES).map_err(io::Error::other)?;
        screen.set_scrollback_bytes(Some(SCROLLBACK_BYTES)).map_err(io::Error::other)?;
        Ok(Arc::new(Mutex::new(Feed { screen, outbox: Vec::new(), overflowed: false, posted: false, cols, rows })))
    }

    pub fn lock(feed: &Mutex<Feed>) -> MutexGuard<'_, Feed> {
        feed.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn push(&mut self, bytes: &[u8]) -> (bool, Vec<u8>) {
        self.screen.write(bytes);
        let replies = self.screen.take_replies();
        if self.overflowed || self.outbox.len() + bytes.len() > OUTBOX {
            self.outbox = Vec::new();
            self.overflowed = true;
        } else {
            self.outbox.extend_from_slice(bytes);
        }
        (!std::mem::replace(&mut self.posted, true), replies)
    }

    pub fn take(&mut self) -> Option<Taken> {
        self.posted = false;
        if std::mem::take(&mut self.overflowed) {
            return Some(Taken::Overflowed);
        }
        let bytes = std::mem::take(&mut self.outbox);
        (!bytes.is_empty()).then_some(Taken::Bytes(bytes))
    }

    pub fn snapshot(&mut self) -> Result<Vec<u8>, kanna_vt::Error> {
        let snapshot = self.screen.snapshot()?;
        self.outbox = Vec::new();
        self.overflowed = false;
        Ok(snapshot)
    }

    pub fn resize(&mut self, id: &str, cols: u16, rows: u16, width: u16, height: u16) -> Vec<u8> {
        self.screen.set_cell_pixels(u32::from(width / cols.max(1)), u32::from(height / rows.max(1)));
        if (cols, rows) != (self.cols, self.rows) {
            (self.cols, self.rows) = (cols, rows);
            if let Err(error) = self.screen.resize(cols, rows) {
                eprintln!("kanna-daemon: couldn't make {id}'s screen {cols}×{rows}: {error}");
            }
        }
        self.screen.take_replies()
    }

    pub fn text(&self, lines: Option<usize>) -> String {
        self.screen.text(lines.map_or(Lines::Screen, Lines::Last))
    }
}

pub struct Reader {
    wake: File,
}

impl Reader {
    pub fn start(id: &str, serial: u64, master: OwnedFd, feed: Arc<Mutex<Feed>>, replies: Sender<Vec<u8>>, events: Sender<Event>) -> io::Result<Reader> {
        let (asleep, wake) = pipe()?;
        let pane = id.to_string();
        std::thread::Builder::new()
            .name(format!("kanna.read.{id}"))
            .spawn(move || read(Pty { pane, serial, master: File::from(master), wake: File::from(asleep), feed, replies, events }))?;
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
    replies: Sender<Vec<u8>>,
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
        let (post, replies) = Feed::lock(&pty.feed).push(&buffer[..n]);
        if !replies.is_empty() {
            let _ = pty.replies.send(replies);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(cols: u16, rows: u16, output: &str) -> Arc<Mutex<Feed>> {
        let feed = Feed::new(cols, rows).unwrap();
        Feed::lock(&feed).push(output.as_bytes());
        feed
    }

    #[test]
    fn read_trims_blanks_and_counts_scrollback() {
        let f = feed(20, 5, "$ echo hi\r\nhi\r\n\x1b[1;32m$\x1b[0m ");
        assert_eq!(Feed::lock(&f).text(None), "$ echo hi\nhi\n$");
        let f = feed(10, 3, "1\r\n2\r\n3\r\n4\r\n5");
        let f = Feed::lock(&f);
        assert_eq!(f.text(None), "3\n4\n5");
        assert_eq!(f.text(Some(4)), "2\n3\n4\n5");
        assert_eq!(f.text(Some(100)), "1\n2\n3\n4\n5");
    }

    #[test]
    fn queries_are_answered() {
        let f = Feed::new(20, 5).unwrap();
        let (posted, replies) = Feed::lock(&f).push(b"\x1b[2;3H\x1b[6n");
        assert!(posted);
        assert_eq!(replies, b"\x1b[2;3R");
    }

    #[test]
    fn a_snapshot_replaces_the_outbox_unless_it_fails() {
        let f = feed(20, 5, "before");
        let mut f = Feed::lock(&f);
        f.push(b"\x1b_a\x9d");
        assert!(f.snapshot().is_err());
        assert!(matches!(f.take(), Some(Taken::Bytes(bytes)) if bytes == b"before\x1b_a\x9d"));
        f.push(b"\x1b\\after");
        let snapshot = f.snapshot().unwrap();
        assert!(f.take().is_none());
        assert_eq!(Terminal::from_snapshot(&snapshot).unwrap().text(Lines::Screen), "beforeafter");
    }

    #[test]
    fn a_full_scrollback_stays_within_its_byte_limit() {
        let f = Feed::new(1000, 50).unwrap();
        let mut f = Feed::lock(&f);
        let line: String = (0..998u32).map(|c| char::from_u32(0x4e00 + c % 2000).unwrap()).collect();
        for i in 0..3000 {
            f.push(format!("\x1b[38;5;{}m\x1b[48;2;{i};1;2m{line}\x1b[0m\r\n", i % 256).as_bytes());
        }
        let snapshot = f.snapshot().unwrap();
        assert!(snapshot.len() < SCROLLBACK_BYTES, "{} bytes", snapshot.len());
    }
}
