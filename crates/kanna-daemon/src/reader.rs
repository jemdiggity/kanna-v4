use crate::settings::Settings;
use crate::Event;
use kanna_vt::{Lines, Terminal};
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

const OUTBOX: usize = 1 << 20;

pub const JOURNAL_LIMIT: usize = 64 << 20;

const OUTPUT: u8 = 0;

const RESIZE: u8 = 1;

const QUIET: Duration = Duration::from_millis(50);

const DRAIN_LIMIT: Duration = Duration::from_millis(500);

const DRAIN: u8 = 1;

const STOP: u8 = 2;

pub struct Feed {
    screen: Terminal,
    outbox: Vec<u8>,
    overflowed: bool,
    posted: bool,
    pub unsaved: bool,
    pub cols: u16,
    pub rows: u16,
    cwd: Option<String>,
    journal: Option<File>,
    journaled: usize,
    pub generation: u64,
}

pub enum Taken {
    Bytes(Vec<u8>),
    Overflowed,
}

impl Feed {
    pub fn new(cols: u16, rows: u16, settings: &Settings) -> io::Result<Arc<Mutex<Feed>>> {
        let mut screen = Terminal::new(cols, rows, 0).map_err(io::Error::other)?;
        settings.start(&mut screen);
        Feed::with(screen, true)
    }

    pub fn restore(snapshot: &[u8], journal: &[u8], settings: &Settings) -> io::Result<Arc<Mutex<Feed>>> {
        let mut screen = Terminal::from_snapshot(snapshot).map_err(io::Error::other)?;
        let replayed = replay(&mut screen, journal);
        settings.apply(&mut screen);
        screen.take_replies();
        screen.take_changed();
        Feed::with(screen, replayed)
    }

    fn with(screen: Terminal, unsaved: bool) -> io::Result<Arc<Mutex<Feed>>> {
        let (cols, rows) = screen.size();
        Ok(Arc::new(Mutex::new(Feed {
            screen,
            outbox: Vec::new(),
            overflowed: false,
            posted: false,
            unsaved,
            cols,
            rows,
            cwd: None,
            journal: None,
            journaled: 0,
            generation: 0,
        })))
    }

    pub fn journal_to(&mut self, file: Option<File>, generation: u64) {
        (self.journal, self.journaled, self.generation) = (file, 0, generation);
    }

    fn record(&mut self, kind: u8, payload: &[u8]) {
        let Some(file) = self.journal.as_mut() else { return };
        self.journaled += payload.len() + 5;
        let mut record = Vec::with_capacity(payload.len() + 5);
        record.push(kind);
        record.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        record.extend_from_slice(payload);
        if self.journaled > JOURNAL_LIMIT || file.write_all(&record).is_err() {
            self.journal = None;
        }
    }

    pub fn apply(&mut self, settings: &Settings) -> Vec<u8> {
        settings.apply(&mut self.screen);
        self.unsaved = true;
        self.screen.take_replies()
    }

    pub fn take_cwd(&mut self) -> Option<String> {
        self.cwd.take()
    }

    pub fn lock(feed: &Mutex<Feed>) -> MutexGuard<'_, Feed> {
        feed.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn push(&mut self, bytes: &[u8]) -> (bool, Vec<u8>) {
        self.screen.write(bytes);
        self.record(OUTPUT, bytes);
        self.unsaved = true;
        let replies = self.screen.take_replies();
        if self.screen.take_changed().pwd {
            if let Some(cwd) = cwd_from_url(&self.screen.pwd()) {
                self.cwd = Some(cwd);
            }
        }
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

    pub fn save(&self) -> Result<Vec<u8>, kanna_vt::Error> {
        self.screen.snapshot()
    }

    pub fn resize(&mut self, id: &str, cols: u16, rows: u16, width: u16, height: u16) -> Vec<u8> {
        let mut payload = Vec::with_capacity(8);
        for value in [cols, rows, width, height] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        self.record(RESIZE, &payload);
        self.screen.set_cell_pixels(u32::from(width / cols.max(1)), u32::from(height / rows.max(1)));
        if (cols, rows) != (self.cols, self.rows) {
            (self.cols, self.rows) = (cols, rows);
            self.unsaved = true;
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

fn replay(screen: &mut Terminal, mut journal: &[u8]) -> bool {
    let mut replayed = false;
    while journal.len() >= 5 {
        let length = u32::from_le_bytes([journal[1], journal[2], journal[3], journal[4]]) as usize;
        let Some(payload) = journal.get(5..5 + length) else { break };
        match journal[0] {
            OUTPUT => screen.write(payload),
            RESIZE if length == 8 => {
                let value = |i: usize| u16::from_le_bytes([payload[i], payload[i + 1]]);
                let (cols, rows, width, height) = (value(0).max(1), value(2).max(1), value(4), value(6));
                screen.set_cell_pixels(u32::from(width / cols), u32::from(height / rows));
                let _ = screen.resize(cols, rows);
            }
            _ => break,
        }
        screen.take_replies();
        replayed = true;
        journal = &journal[5 + length..];
    }
    replayed
}

pub fn cwd_from_url(url: &str) -> Option<String> {
    let (rest, raw) = if let Some(rest) = url.strip_prefix("file://") {
        (rest, false)
    } else {
        (url.strip_prefix("kitty-shell-cwd://")?, true)
    };
    let slash = rest.find('/')?;
    let (host, path) = rest.split_at(slash);
    if !is_local(host) {
        return None;
    }
    if raw {
        return Some(path.to_string());
    }
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let byte = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok().and_then(|h| u8::from_str_radix(h, 16).ok())?;
                out.push(byte);
                i += 3;
            }
            b'?' | b'#' => break,
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

fn is_local(host: &str) -> bool {
    if host.is_empty() || host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    let mut name = [0u8; 256];
    if unsafe { libc::gethostname(name.as_mut_ptr() as *mut libc::c_char, name.len()) } != 0 {
        return false;
    }
    let length = name.iter().position(|b| *b == 0).unwrap_or(name.len());
    let local = String::from_utf8_lossy(&name[..length]).to_string();
    let short = |h: &str| h.split('.').next().unwrap_or(h).to_ascii_lowercase();
    host.eq_ignore_ascii_case(&local) || short(host) == short(&local) && (local.ends_with(".local") || host.ends_with(".local"))
}

pub struct Reader {
    wake: File,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Reader {
    pub fn start(id: &str, serial: u64, master: OwnedFd, feed: Arc<Mutex<Feed>>, replies: Sender<Vec<u8>>, events: Sender<Event>) -> io::Result<Reader> {
        let (asleep, wake) = pipe()?;
        let pane = id.to_string();
        let thread = std::thread::Builder::new()
            .name(format!("kanna.read.{id}"))
            .spawn(move || read(Pty { pane, serial, master: File::from(master), wake: File::from(asleep), feed, replies, events }))?;
        Ok(Reader { wake: File::from(wake), thread: Some(thread) })
    }

    pub fn drain(&self) {
        let _ = (&self.wake).write(&[DRAIN]);
    }

    pub fn stop(&self) {
        let _ = (&self.wake).write(&[STOP]);
    }

    pub fn join(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
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
            let mut asked = [0u8; 64];
            match pty.wake.read(&mut asked) {
                Ok(n @ 1..) if asked[..n].contains(&STOP) => return,
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
        let feed = Feed::new(cols, rows, &Settings::default()).unwrap();
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
    fn a_shell_reporting_its_folder_changes_the_cwd() {
        let f = feed(20, 5, "\x1b]7;file://localhost/tmp/a%20b\x07");
        assert_eq!(Feed::lock(&f).take_cwd().as_deref(), Some("/tmp/a b"));
        assert_eq!(Feed::lock(&f).take_cwd(), None);
        Feed::lock(&f).push(b"\x1b]7;file://elsewhere.example/etc\x07\x1b]7;kitty-shell-cwd:///x%y\x07");
        assert_eq!(Feed::lock(&f).take_cwd().as_deref(), Some("/x%y"));
        assert_eq!(cwd_from_url("file:///a%2"), None);
        assert_eq!(cwd_from_url("http://localhost/a"), None);
        assert_eq!(cwd_from_url("file://"), None);
    }

    #[test]
    fn queries_are_answered() {
        let f = Feed::new(20, 5, &Settings::default()).unwrap();
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
        let f = Feed::new(1000, 50, &Settings::default()).unwrap();
        let mut f = Feed::lock(&f);
        let line: String = (0..998u32).map(|c| char::from_u32(0x4e00 + c % 2000).unwrap()).collect();
        for i in 0..3000 {
            f.push(format!("\x1b[38;5;{}m\x1b[48;2;{i};1;2m{line}\x1b[0m\r\n", i % 256).as_bytes());
        }
        let snapshot = f.snapshot().unwrap();
        assert!(snapshot.len() < crate::settings::MAX_SCROLLBACK_BYTES, "{} bytes", snapshot.len());
    }
}
