//! The keeper: one per state directory, a single-threaded poll loop over its
//! socket and its clients. It spawns processes on ptys, keeps a copy of each
//! pty master, hands the masters to its client (the daemon), reaps the
//! processes and reports their exits. It never reads or writes a pty.
//! See docs/keeper.md.

use crate::pty::{self, What};
use crate::{fdpass, paths, sig, ExitStatus, PROTOCOL_VERSION};
use anyhow::{Context, Result};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, VecDeque};
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::rc::Rc;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Connections at once; more are refused.
const MAX_CONNS: usize = 16;
const ACCEPT_PAUSE: Duration = Duration::from_millis(100);
/// Longest request line.
const MAX_LINE: usize = 1 << 20;
/// Unsent replies and events per connection before it is dropped.
const MAX_OUT: usize = 8 << 20;

pub struct Settings {
    pub state_dir: PathBuf,
    /// Exit after this long with no live process and no client.
    pub idle: Duration,
    /// Between SIGHUP and SIGKILL on `close`.
    pub close_grace: Duration,
    pub max_ptys: usize,
    pub spawn_timeout: Duration,
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

impl Settings {
    /// From the environment: `KANNA_KEEPER_IDLE_MS` (60 s),
    /// `KANNA_KEEPER_CLOSE_GRACE_MS` (1 s), `KANNA_KEEPER_MAX_PTYS` (256).
    pub fn from_env(state_dir: PathBuf) -> Self {
        Settings {
            state_dir,
            idle: Duration::from_millis(env_u64("KANNA_KEEPER_IDLE_MS", 60_000)),
            close_grace: Duration::from_millis(env_u64("KANNA_KEEPER_CLOSE_GRACE_MS", 1_000)),
            max_ptys: env_u64("KANNA_KEEPER_MAX_PTYS", 256) as usize,
            spawn_timeout: Duration::from_millis(env_u64("KANNA_KEEPER_SPAWN_TIMEOUT_MS", 5_000)),
        }
    }
}

/// The uid a peer must have: ours. Debug builds let tests pretend otherwise.
fn expected_uid() -> libc::uid_t {
    #[cfg(debug_assertions)]
    if let Some(uid) = std::env::var("KANNA_KEEPER_TEST_PEER_UID").ok().and_then(|v| v.parse().ok()) {
        return uid;
    }
    unsafe { libc::getuid() }
}

/// A process the keeper spawned, and its pty.
struct Pty {
    pid: i32,
    cmd: Vec<String>,
    cwd: String,
    /// The keeper's copy, kept until the exit is acknowledged, so a daemon
    /// that connects later can still read the last output.
    master: Rc<OwnedFd>,
    exit: Option<ExitStatus>,
}

impl Pty {
    fn foreground(&self) -> Option<i32> {
        let fg = unsafe { libc::tcgetpgrp(self.master.as_raw_fd()) };
        (fg > 0 && fg != self.pid).then_some(fg)
    }

    fn describe(&self, id: &str) -> Value {
        let mut v = json!({"id": id, "pid": self.pid, "cmd": self.cmd, "cwd": self.cwd});
        if let Some(exit) = self.exit {
            v["exit"] = exit.to_json();
        }
        v
    }
}

/// A line to send, with the descriptors that go with its first byte.
struct Out {
    bytes: Vec<u8>,
    sent: usize,
    fds: Vec<Rc<OwnedFd>>,
}

struct Conn {
    serial: u64,
    stream: UnixStream,
    inbuf: Vec<u8>,
    out: VecDeque<Out>,
    out_bytes: usize,
    out_fds: usize,
    max_out_fds: usize,
    dead: bool,
}

impl Conn {
    fn send(&mut self, value: Value, fds: Vec<Rc<OwnedFd>>) {
        let mut bytes = serde_json::to_vec(&value).unwrap_or_default();
        bytes.push(b'\n');
        self.out_bytes += bytes.len();
        self.out_fds += fds.len();
        if self.out_bytes > MAX_OUT || self.out_fds > self.max_out_fds {
            eprintln!("kanna-keeper: dropped a client that isn't reading");
            self.dead = true;
            return;
        }
        self.out.push_back(Out { bytes, sent: 0, fds });
    }

    fn flush(&mut self) {
        while let Some(out) = self.out.front_mut() {
            let fds: Vec<_> = if out.sent == 0 { out.fds.iter().map(|f| f.as_raw_fd()).collect() } else { vec![] };
            match fdpass::send(self.stream.as_raw_fd(), &out.bytes[out.sent..], &fds, true) {
                Ok(n) => {
                    out.sent += n;
                    self.out_fds -= out.fds.len();
                    out.fds.clear();
                    if out.sent == out.bytes.len() {
                        self.out_bytes -= out.bytes.len();
                        self.out.pop_front();
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => {
                    self.dead = true;
                    break;
                }
            }
        }
    }
}

struct Pending {
    conn: u64,
    tag: Value,
    id: String,
    pid: i32,
    cmd: Vec<String>,
    cwd: String,
    master: OwnedFd,
    exec: OwnedFd,
    deadline: Instant,
    exit: Option<ExitStatus>,
}

struct Closed {
    pid: i32,
    foreground: Option<i32>,
    kill_at: Option<Instant>,
}

struct Keeper {
    listener: UnixListener,
    sigs: OwnedFd,
    conns: Vec<Conn>,
    ptys: BTreeMap<String, Pty>,
    pending: Vec<Pending>,
    base: pty::Base,
    next_serial: u64,
    /// Closed processes not reaped yet; their exits aren't reported.
    closed: Vec<Closed>,
    settings: Settings,
    idle_since: Instant,
    accept_paused_until: Option<Instant>,
    peer_uid: libc::uid_t,
    stop: bool,
}

pub const LISTENING: u8 = b'1';
pub const ALREADY_RUNNING: u8 = b'2';
pub const FAILED: u8 = b'E';

fn tell(ready: Option<&OwnedFd>, bytes: &[u8]) {
    if let Some(fd) = ready {
        unsafe { libc::write(fd.as_raw_fd(), bytes.as_ptr().cast(), bytes.len()) };
    }
}

/// Runs the keeper until it is idle or told to stop (SIGTERM, SIGINT).
/// Returns at once if another keeper holds the state directory.
pub fn run(settings: Settings, ready: Option<&OwnedFd>) -> Result<()> {
    sig::cloexec_inherited();
    pty::raise_nofile();
    let dir = settings.state_dir.clone();
    paths::ensure_dir(&dir).with_context(|| format!("state directory {}", dir.display()))?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(paths::lock(&dir))?;
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        tell(ready, &[ALREADY_RUNNING]);
        return Ok(());
    }
    unsafe {
        libc::signal(libc::SIGHUP, libc::SIG_IGN);
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        for signo in [libc::SIGCHLD, libc::SIGTERM, libc::SIGINT] {
            libc::sigaddset(&mut set, signo);
        }
        libc::pthread_sigmask(libc::SIG_UNBLOCK, &set, std::ptr::null_mut());
    }
    let sigs = sig::pipe(&[libc::SIGCHLD, libc::SIGTERM, libc::SIGINT])?;
    let sock = paths::socket(&dir);
    let listener = listen(&sock)?;
    let pid_file = paths::pid(&dir);
    std::fs::write(&pid_file, format!("{}\n", std::process::id()))?;
    tell(ready, &[LISTENING]);
    let mut keeper = Keeper {
        listener,
        sigs,
        conns: Vec::new(),
        ptys: BTreeMap::new(),
        pending: Vec::new(),
        base: pty::Base::load(),
        next_serial: 0,
        closed: Vec::new(),
        settings,
        idle_since: Instant::now(),
        accept_paused_until: None,
        peer_uid: expected_uid(),
        stop: false,
    };
    let result = keeper.run();
    drop(keeper);
    let _ = std::fs::remove_file(&sock);
    if std::fs::read_to_string(&pid_file).is_ok_and(|p| p.trim() == std::process::id().to_string()) {
        let _ = std::fs::remove_file(&pid_file);
    }
    drop(lock);
    result
}

fn listen(sock: &Path) -> Result<UnixListener> {
    let _ = std::fs::remove_file(sock);
    let old = unsafe { libc::umask(0o177) };
    let listener = UnixListener::bind(sock);
    unsafe { libc::umask(old) };
    let listener = listener.with_context(|| format!("binding {}", sock.display()))?;
    std::fs::set_permissions(sock, std::fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    Ok(listener)
}

fn pollfd(fd: libc::c_int, events: libc::c_short) -> libc::pollfd {
    libc::pollfd { fd, events, revents: 0 }
}

impl Keeper {
    fn run(&mut self) -> Result<()> {
        while !self.stop {
            if self.accept_paused_until.is_some_and(|until| Instant::now() >= until) {
                self.accept_paused_until = None;
            }
            let listening = if self.accept_paused_until.is_none() { self.listener.as_raw_fd() } else { -1 };
            let mut fds = vec![pollfd(self.sigs.as_raw_fd(), libc::POLLIN), pollfd(listening, libc::POLLIN)];
            for c in &self.conns {
                let events = if c.out.is_empty() { libc::POLLIN } else { libc::POLLIN | libc::POLLOUT };
                fds.push(pollfd(c.stream.as_raw_fd(), events));
            }
            let conns = self.conns.len();
            for p in &self.pending {
                fds.push(pollfd(p.exec.as_raw_fd(), libc::POLLIN));
            }
            let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as _, self.timeout()) };
            if n < 0 {
                let e = std::io::Error::last_os_error();
                if e.kind() != std::io::ErrorKind::Interrupted {
                    return Err(e.into());
                }
                continue;
            }
            if fds[0].revents != 0 {
                for signo in sig::drain(&self.sigs) {
                    match signo {
                        libc::SIGTERM | libc::SIGINT => self.stop = true,
                        libc::SIGCHLD => self.reap(),
                        _ => {}
                    }
                }
            }
            for i in 0..conns {
                if fds[2 + i].revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0 {
                    self.read_conn(i);
                }
            }
            for k in (0..fds.len() - 2 - conns).rev() {
                if fds[2 + conns + k].revents != 0 {
                    self.exec_heard(k);
                }
            }
            self.spawn_timeouts();
            if fds[1].revents != 0 {
                self.accept();
            }
            for c in &mut self.conns {
                c.flush();
            }
            self.conns.retain(|c| !c.dead);
            if self.run_kills() {
                self.reap();
            }
            self.check_idle();
        }
        Ok(())
    }

    fn timeout(&self) -> libc::c_int {
        let now = Instant::now();
        let mut wait = Duration::from_secs(1);
        if let Some(at) = self.closed.iter().filter_map(|c| c.kill_at).min() {
            wait = wait.min(at.saturating_duration_since(now));
        }
        if let Some(at) = self.pending.iter().map(|p| p.deadline).min() {
            wait = wait.min(at.saturating_duration_since(now));
        }
        if let Some(until) = self.accept_paused_until {
            wait = wait.min(until.saturating_duration_since(now));
        }
        if self.idle() {
            wait = wait.min((self.idle_since + self.settings.idle).saturating_duration_since(now));
        }
        wait.as_millis() as libc::c_int + 1
    }

    /// No client, no live process, nothing waiting to be killed.
    fn idle(&self) -> bool {
        self.conns.is_empty()
            && self.pending.is_empty()
            && self.closed.is_empty()
            && self.ptys.values().all(|p| p.exit.is_some())
    }

    fn check_idle(&mut self) {
        if !self.idle() {
            self.idle_since = Instant::now();
            return;
        }
        if self.idle_since.elapsed() < self.settings.idle {
            return;
        }
        let sock = paths::socket(&self.settings.state_dir);
        let _ = std::fs::remove_file(&sock);
        self.accept();
        if self.conns.is_empty() {
            self.stop = true;
            return;
        }
        match listen(&sock) {
            Ok(listener) => self.listener = listener,
            Err(e) => {
                eprintln!("kanna-keeper: {e:#}");
                self.stop = true;
            }
        }
        self.idle_since = Instant::now();
    }

    fn run_kills(&mut self) -> bool {
        let now = Instant::now();
        let mut killed = false;
        for c in &mut self.closed {
            if c.kill_at.is_none_or(|at| at > now) {
                continue;
            }
            unsafe { libc::killpg(c.pid, libc::SIGKILL) };
            if let Some(fg) = c.foreground.filter(|&fg| unsafe { libc::getsid(fg) } == c.pid) {
                unsafe { libc::killpg(fg, libc::SIGKILL) };
            }
            c.kill_at = None;
            killed = true;
        }
        killed
    }

    fn accept(&mut self) {
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    match fdpass::peer_uid(&stream) {
                        Ok(uid) if uid == self.peer_uid => {}
                        _ => {
                            eprintln!("kanna-keeper: refused a connection from another user");
                            continue;
                        }
                    }
                    if self.conns.len() >= MAX_CONNS {
                        eprintln!("kanna-keeper: refused a connection: too many clients");
                        continue;
                    }
                    let _ = stream.set_nonblocking(true);
                    self.next_serial += 1;
                    let mut conn = Conn {
                        serial: self.next_serial,
                        stream,
                        inbuf: Vec::new(),
                        out: VecDeque::new(),
                        out_bytes: 0,
                        out_fds: 0,
                        max_out_fds: 2 * self.settings.max_ptys + 16,
                        dead: false,
                    };
                    // Exits nobody has acknowledged yet.
                    for (id, p) in &self.ptys {
                        if let Some(exit) = p.exit {
                            conn.send(exited_event(id, p.pid, exit), vec![]);
                        }
                    }
                    self.conns.push(conn);
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) if matches!(e.raw_os_error(), Some(libc::EMFILE | libc::ENFILE | libc::ENOBUFS | libc::ENOMEM)) => {
                    eprintln!("kanna-keeper: can't accept a connection: {e}");
                    self.accept_paused_until = Some(Instant::now() + ACCEPT_PAUSE);
                    break;
                }
                Err(_) => break,
            }
        }
    }

    fn read_conn(&mut self, i: usize) {
        let mut buf = [0u8; 65536];
        let n = match self.conns[i].stream.read(&mut buf) {
            Ok(0) => {
                self.conns[i].dead = true;
                return;
            }
            Ok(n) => n,
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted) => return,
            Err(_) => {
                self.conns[i].dead = true;
                return;
            }
        };
        self.conns[i].inbuf.extend_from_slice(&buf[..n]);
        while let Some(end) = self.conns[i].inbuf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.conns[i].inbuf.drain(..=end).collect();
            if self.conns[i].dead {
                return;
            }
            match serde_json::from_slice::<Value>(&line) {
                Ok(req) => self.request(i, &req),
                Err(e) => self.conns[i].send(json!({"ok": false, "error": format!("bad request: {e}")}), vec![]),
            }
        }
        if self.conns[i].inbuf.len() > MAX_LINE {
            eprintln!("kanna-keeper: dropped a client: request too long");
            self.conns[i].dead = true;
        }
    }

    fn request(&mut self, i: usize, req: &Value) {
        let tag = req.get("req").cloned().unwrap_or(Value::Null);
        let id = req["id"].as_str().unwrap_or("");
        let result: Result<(Value, Vec<Rc<OwnedFd>>), String> = match req["op"].as_str().unwrap_or("") {
            "hello" => Ok((json!({"version": PROTOCOL_VERSION, "pid": std::process::id()}), vec![])),
            "spawn" => match self.spawn(i, &tag, id, req) {
                Ok(()) => return,
                Err(error) => Err(error),
            },
            "adopt" => {
                let lines: Vec<_> = self
                    .ptys
                    .iter()
                    .map(|(id, p)| (json!({"req": tag, "pty": p.describe(id), "fds": 1}), p.master.clone()))
                    .collect();
                let count = lines.len();
                for (line, fd) in lines {
                    self.conns[i].send(line, vec![fd]);
                }
                Ok((json!({"ptys": count}), vec![]))
            }
            "close" => self.close(id),
            "ack" => self.ack(id),
            op => Err(format!("unknown op {op:?}")),
        };
        let (mut reply, fds) = match result {
            Ok((body, fds)) => {
                let mut reply = json!({"ok": true});
                if let Value::Object(body) = body {
                    reply.as_object_mut().unwrap().extend(body);
                }
                if !fds.is_empty() {
                    reply["fds"] = json!(fds.len());
                }
                (reply, fds)
            }
            Err(error) => (json!({"ok": false, "error": error}), vec![]),
        };
        if !tag.is_null() {
            reply["req"] = tag;
        }
        self.conns[i].send(reply, fds);
    }

    fn spawn(&mut self, i: usize, tag: &Value, id: &str, req: &Value) -> Result<(), String> {
        valid_id(id)?;
        if self.ptys.contains_key(id) || self.pending.iter().any(|p| p.id == id) {
            return Err(format!("pty {id:?} exists"));
        }
        if self.ptys.len() + self.pending.len() + self.closed.len() >= self.settings.max_ptys {
            return Err(format!("too many ptys ({})", self.settings.max_ptys));
        }
        let what = match (req["command"].as_str(), req["shell"].as_str()) {
            (Some(_), Some(_)) => return Err("give command or shell, not both".into()),
            (Some(command), None) => What::Command(command),
            (None, shell) => What::Shell(shell),
        };
        let size = |key: &str, default: u16| req[key].as_u64().map_or(default, |v| v.clamp(1, 10_000) as u16);
        let env: Option<&Map<String, Value>> = req["env"].as_object();
        let (spawned, argv) = pty::spawn(&self.base, &what, req["cwd"].as_str(), env, size("rows", 24), size("cols", 80))
            .map_err(|e| format!("{e:#}"))?;
        self.pending.push(Pending {
            conn: self.conns[i].serial,
            tag: tag.clone(),
            id: id.to_string(),
            pid: spawned.pid,
            cmd: argv.iter().map(|a| a.to_string_lossy().into_owned()).collect(),
            cwd: req["cwd"].as_str().unwrap_or("").to_string(),
            master: spawned.master,
            exec: spawned.exec,
            deadline: Instant::now() + self.settings.spawn_timeout,
            exit: None,
        });
        Ok(())
    }

    fn exec_heard(&mut self, k: usize) {
        let mut errno = 0 as libc::c_int;
        let size = std::mem::size_of::<libc::c_int>();
        let n = unsafe { libc::read(self.pending[k].exec.as_raw_fd(), (&mut errno as *mut libc::c_int).cast(), size) };
        if n < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::WouldBlock {
            return;
        }
        let p = self.pending.remove(k);
        if n == 0 {
            self.started(p);
            return;
        }
        let error = if n as usize == size { std::io::Error::from_raw_os_error(errno).to_string() } else { "exec failed".into() };
        let program = p.cmd.first().cloned().unwrap_or_default();
        self.spawn_failed(p, format!("cannot run {program}: {error}"));
    }

    fn spawn_timeouts(&mut self) {
        let now = Instant::now();
        while let Some(k) = self.pending.iter().position(|p| p.deadline <= now) {
            let p = self.pending.remove(k);
            if p.exit.is_none() {
                unsafe { libc::kill(p.pid, libc::SIGKILL) };
            }
            let program = p.cmd.first().cloned().unwrap_or_default();
            let after = self.settings.spawn_timeout;
            self.spawn_failed(p, format!("spawning {program} timed out after {after:?}"));
        }
    }

    fn spawn_failed(&mut self, p: Pending, error: String) {
        if p.exit.is_none() {
            self.closed.push(Closed { pid: p.pid, foreground: None, kill_at: None });
        }
        let mut reply = json!({"ok": false, "error": error});
        if !p.tag.is_null() {
            reply["req"] = p.tag;
        }
        if let Some(c) = self.conns.iter_mut().find(|c| c.serial == p.conn) {
            c.send(reply, vec![]);
        }
        self.reap();
    }

    fn started(&mut self, p: Pending) {
        let master = Rc::new(p.master);
        let mut reply = json!({"ok": true, "id": p.id, "pid": p.pid, "cmd": p.cmd, "fds": 1});
        if !p.tag.is_null() {
            reply["req"] = p.tag;
        }
        if let Some(c) = self.conns.iter_mut().find(|c| c.serial == p.conn) {
            c.send(reply, vec![master.clone()]);
        }
        self.ptys.insert(p.id.clone(), Pty { pid: p.pid, cmd: p.cmd, cwd: p.cwd, master, exit: p.exit });
        if let Some(exit) = p.exit {
            let event = exited_event(&p.id, p.pid, exit);
            for c in &mut self.conns {
                c.send(event.clone(), vec![]);
            }
        }
    }

    /// SIGHUP to the process group and the pty's foreground group, SIGKILL
    /// to both after the grace period. The id is free at once.
    fn close(&mut self, id: &str) -> Result<(Value, Vec<Rc<OwnedFd>>), String> {
        if self.pending.iter().any(|p| p.id == id) {
            return Err(format!("pty {id:?} is still starting"));
        }
        let Some(p) = self.ptys.remove(id) else {
            return Err(format!("no pty {id:?}"));
        };
        if p.exit.is_none() {
            let foreground = p.foreground();
            for pg in std::iter::once(p.pid).chain(foreground) {
                unsafe { libc::killpg(pg, libc::SIGHUP) };
            }
            self.closed.push(Closed { pid: p.pid, foreground, kill_at: Some(Instant::now() + self.settings.close_grace) });
        }
        Ok((json!({}), vec![]))
    }

    /// Forgets an exited pty. Acknowledging one that's gone is fine.
    fn ack(&mut self, id: &str) -> Result<(Value, Vec<Rc<OwnedFd>>), String> {
        match self.ptys.get(id) {
            Some(p) if p.exit.is_none() => Err(format!("pty {id:?} is still running")),
            Some(_) => {
                self.ptys.remove(id);
                Ok((json!({}), vec![]))
            }
            None => Ok((json!({}), vec![])),
        }
    }

    fn reap(&mut self) {
        self.closed.retain(|c| c.kill_at.is_some() || wait(c.pid).is_none());
        for p in self.pending.iter_mut().filter(|p| p.exit.is_none()) {
            p.exit = wait(p.pid);
        }
        let mut events = Vec::new();
        for (id, p) in self.ptys.iter_mut().filter(|(_, p)| p.exit.is_none()) {
            if let Some(exit) = wait(p.pid) {
                p.exit = Some(exit);
                events.push(exited_event(id, p.pid, exit));
            }
        }
        for event in events {
            for c in &mut self.conns {
                c.send(event.clone(), vec![]);
            }
        }
    }
}

fn wait(pid: i32) -> Option<ExitStatus> {
    let mut raw = 0;
    if unsafe { libc::waitpid(pid, &mut raw, libc::WNOHANG) } != pid {
        return None;
    }
    Some(if libc::WIFSIGNALED(raw) {
        ExitStatus::Signal(libc::WTERMSIG(raw))
    } else {
        ExitStatus::Code(libc::WEXITSTATUS(raw))
    })
}

fn exited_event(id: &str, pid: i32, exit: ExitStatus) -> Value {
    let mut v = json!({"exited": id, "pid": pid});
    v.as_object_mut().unwrap().extend(exit.to_json().as_object().unwrap().clone());
    v
}

fn valid_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > 128 || id.chars().any(char::is_control) {
        return Err(format!("bad id {id:?}"));
    }
    Ok(())
}

/// `--detach`: runs the keeper in the background, in its own session, with
/// stdio on /dev/null and stderr appended to keeper.log. Returns once it is
/// listening, or once it has found another keeper already running.
pub fn detach(mut settings: Settings) -> Result<()> {
    paths::ensure_dir(&settings.state_dir).with_context(|| format!("state directory {}", settings.state_dir.display()))?;
    settings.state_dir = std::path::absolute(&settings.state_dir)?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(paths::log(&settings.state_dir))?;
    let mut fds = [-1; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let (read, write) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    sig::set_cloexec(read.as_raw_fd());
    sig::set_cloexec(write.as_raw_fd());
    match unsafe { libc::fork() } {
        -1 => Err(std::io::Error::last_os_error().into()),
        0 => {
            drop(read);
            unsafe { libc::setsid() };
            match unsafe { libc::fork() } {
                -1 => unsafe { libc::_exit(1) },
                0 => {}
                _ => unsafe { libc::_exit(0) },
            }
            unsafe { libc::chdir(c"/".as_ptr()) };
            let null = std::fs::OpenOptions::new().read(true).write(true).open("/dev/null");
            unsafe {
                if let Ok(null) = &null {
                    libc::dup2(null.as_raw_fd(), 0);
                    libc::dup2(null.as_raw_fd(), 1);
                }
                libc::dup2(log.as_raw_fd(), 2);
            }
            drop(null);
            drop(log);
            sig::close_inherited(&[write.as_raw_fd()]);
            let code = match run(settings, Some(&write)) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("kanna-keeper: {e:#}");
                    tell(Some(&write), format!("{}{e:#}", FAILED as char).as_bytes());
                    1
                }
            };
            std::process::exit(code);
        }
        child => {
            drop(write);
            let mut status = 0;
            unsafe { libc::waitpid(child, &mut status, 0) };
            let mut read = std::fs::File::from(read);
            let mut byte = [0u8; 1];
            match read.read(&mut byte) {
                Ok(1) if byte[0] == LISTENING || byte[0] == ALREADY_RUNNING => Ok(()),
                Ok(1) if byte[0] == FAILED => {
                    let mut error = String::new();
                    let _ = read.read_to_string(&mut error);
                    Err(anyhow::anyhow!(error))
                }
                _ => Err(anyhow::anyhow!(
                    "the keeper stopped before it listened; see {}",
                    paths::log(&settings.state_dir).display()
                )),
            }
        }
    }
}
