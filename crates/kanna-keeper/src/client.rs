//! The daemon's side of the keeper protocol: spawn, adopt, close and
//! acknowledge ptys, and receive exit events. Blocking; for a poll loop,
//! watch [`Client::as_raw_fd`] and call [`Client::poll_events`].

use crate::{fdpass, paths, ExitStatus};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::os::fd::{AsRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const START_TIMEOUT: Duration = Duration::from_secs(5);

/// A pty the keeper holds: the process in it, and a copy of its master for
/// the daemon to read, write and resize.
#[derive(Debug)]
pub struct Pty {
    pub id: String,
    pub pid: i32,
    /// What runs in it, e.g. `["/bin/zsh"]` or `["/bin/sh", "-c", "make"]`.
    pub cmd: Vec<String>,
    /// The working directory asked for at spawn ("" if none).
    pub cwd: String,
    /// Close-on-exec, blocking.
    pub master: OwnedFd,
    /// Set (by `adopt`) when the process has exited but the exit hasn't been
    /// acknowledged: its last output may still be in the pty.
    pub exit: Option<ExitStatus>,
}

/// An exit event: the process in pty `id` ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exited {
    pub id: String,
    pub pid: i32,
    pub status: ExitStatus,
}

/// What to spawn. `command` runs as `/bin/sh -c COMMAND`; otherwise `shell`
/// (or the user's login shell) runs as an interactive login shell.
#[derive(Clone, Debug, Default)]
pub struct SpawnRequest {
    pub id: String,
    pub command: Option<String>,
    pub shell: Option<String>,
    pub cwd: Option<PathBuf>,
    /// Over the keeper's environment and Ghostty's additions; None unsets.
    pub env: BTreeMap<String, Option<String>>,
    pub cols: u16,
    pub rows: u16,
}

impl SpawnRequest {
    /// The user's login shell, 80x24.
    pub fn shell(id: &str) -> Self {
        SpawnRequest { id: id.into(), cols: 80, rows: 24, ..Default::default() }
    }

    /// `/bin/sh -c COMMAND`, 80x24.
    pub fn command(id: &str, command: &str) -> Self {
        SpawnRequest { command: Some(command.into()), ..Self::shell(id) }
    }

    fn to_json(&self) -> Value {
        let mut v = json!({"op": "spawn", "id": self.id, "cols": self.cols, "rows": self.rows});
        if let Some(c) = &self.command {
            v["command"] = c.as_str().into();
        }
        if let Some(s) = &self.shell {
            v["shell"] = s.as_str().into();
        }
        if let Some(cwd) = &self.cwd {
            v["cwd"] = cwd.to_string_lossy().into();
        }
        if !self.env.is_empty() {
            let env: Map<String, Value> =
                self.env.iter().map(|(k, v)| (k.clone(), v.as_deref().map_or(Value::Null, Value::from))).collect();
            v["env"] = Value::Object(env);
        }
        v
    }
}

/// A connection to the keeper.
pub struct Client {
    stream: UnixStream,
    inbuf: Vec<u8>,
    /// Descriptors received and not yet claimed by a line.
    fds: VecDeque<OwnedFd>,
    events: VecDeque<Exited>,
    next_req: u64,
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect()).unwrap_or_default()
}

fn keeper_error(msg: impl Into<String>) -> io::Error {
    io::Error::other(msg.into())
}

impl Client {
    /// Connects to the keeper listening on `socket`, which must run as the
    /// same user.
    pub fn connect(socket: &Path) -> io::Result<Self> {
        let stream = UnixStream::connect(socket)?;
        if fdpass::peer_uid(&stream)? != unsafe { libc::getuid() } {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "the keeper runs as another user"));
        }
        Ok(Client { stream, inbuf: Vec::new(), fds: VecDeque::new(), events: VecDeque::new(), next_req: 1 })
    }

    /// Connects to the keeper of `state_dir`, first starting `keeper_bin`
    /// (detached, in its own session) if none is listening. The keeper
    /// inherits this process's environment: it is the base of every
    /// spawned process's environment.
    pub fn connect_or_start(state_dir: &Path, keeper_bin: &Path) -> io::Result<Self> {
        let socket = paths::socket(state_dir);
        let deadline = Instant::now() + START_TIMEOUT;
        let mut backoff = Duration::from_millis(5);
        loop {
            let error = match Self::connect(&socket) {
                Ok(client) => return Ok(client),
                Err(e) if e.kind() == io::ErrorKind::PermissionDenied => return Err(e),
                Err(e) => e,
            };
            if Instant::now() >= deadline {
                return Err(keeper_error(format!("no keeper is listening on {}: {error}", socket.display())));
            }
            let out = std::process::Command::new(keeper_bin)
                .arg("--state-dir")
                .arg(state_dir)
                .arg("--detach")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .output()?;
            if !out.status.success() {
                let stderr = String::from_utf8_lossy(&out.stderr);
                return Err(keeper_error(format!(
                    "{} --detach failed ({}): {}",
                    keeper_bin.display(),
                    out.status,
                    stderr.trim()
                )));
            }
            if let Ok(client) = Self::connect(&socket) {
                return Ok(client);
            }
            std::thread::sleep(backoff.min(deadline.saturating_duration_since(Instant::now())));
            backoff = (backoff * 2).min(Duration::from_millis(200));
        }
    }

    pub fn set_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.stream.set_read_timeout(timeout)?;
        self.stream.set_write_timeout(timeout)
    }

    /// The protocol version and the keeper's pid.
    pub fn hello(&mut self) -> io::Result<(u64, i32)> {
        let (reply, _) = self.request(json!({"op": "hello"}), |_, _| {})?;
        Ok((reply["version"].as_u64().unwrap_or(0), reply["pid"].as_i64().unwrap_or(0) as i32))
    }

    /// Spawns a process on a new pty and returns it with a copy of the master.
    pub fn spawn(&mut self, req: &SpawnRequest) -> io::Result<Pty> {
        let (reply, mut fds) = self.request(req.to_json(), |_, _| {})?;
        let master = fds.pop().ok_or_else(|| keeper_error("spawn: no pty in the reply"))?;
        Ok(Pty {
            id: req.id.clone(),
            pid: reply["pid"].as_i64().unwrap_or(0) as i32,
            cmd: strings(&reply["cmd"]),
            cwd: req.cwd.as_ref().map(|c| c.to_string_lossy().into_owned()).unwrap_or_default(),
            master,
            exit: None,
        })
    }

    /// Every pty the keeper holds, each with a new copy of its master: how
    /// a restarted daemon gets its terminals back.
    pub fn adopt(&mut self) -> io::Result<Vec<Pty>> {
        let mut ptys = Vec::new();
        self.request(json!({"op": "adopt"}), |line, fds| {
            let p = &line["pty"];
            if let Some(master) = fds.pop() {
                ptys.push(Pty {
                    id: p["id"].as_str().unwrap_or("").into(),
                    pid: p["pid"].as_i64().unwrap_or(0) as i32,
                    cmd: strings(&p["cmd"]),
                    cwd: p["cwd"].as_str().unwrap_or("").into(),
                    master,
                    exit: p.get("exit").and_then(ExitStatus::from_json),
                });
            }
        })?;
        Ok(ptys)
    }

    /// Ends the process group in pty `id` (SIGHUP, then SIGKILL after the
    /// keeper's grace period) and drops the keeper's copy. Its exit isn't
    /// reported. Also forgets an exited pty.
    pub fn close(&mut self, id: &str) -> io::Result<()> {
        self.request(json!({"op": "close", "id": id}), |_, _| {}).map(|_| ())
    }

    /// Acknowledges an exit: the keeper forgets the pty and its copy.
    pub fn ack(&mut self, id: &str) -> io::Result<()> {
        self.request(json!({"op": "ack", "id": id}), |_, _| {}).map(|_| ())
    }

    /// Exit events received so far, without blocking. Events not yet
    /// acknowledged are sent again on the next connection. An error
    /// (`UnexpectedEof` when the keeper went away) means reconnect.
    pub fn poll_events(&mut self) -> io::Result<Vec<Exited>> {
        while self.fill(false)? {}
        self.parse_lines(None, &mut |_, _| {});
        Ok(self.events.drain(..).collect())
    }

    /// The next exit event, waiting up to `timeout` (forever if None).
    pub fn wait_event(&mut self, timeout: Option<Duration>) -> io::Result<Option<Exited>> {
        let deadline = timeout.map(|t| Instant::now() + t);
        loop {
            self.parse_lines(None, &mut |_, _| {});
            if let Some(e) = self.events.pop_front() {
                return Ok(Some(e));
            }
            let wait = match deadline {
                Some(d) => {
                    let left = d.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return Ok(None);
                    }
                    left.as_millis() as libc::c_int + 1
                }
                None => -1,
            };
            let mut pfd = libc::pollfd { fd: self.stream.as_raw_fd(), events: libc::POLLIN, revents: 0 };
            if unsafe { libc::poll(&mut pfd, 1, wait) } > 0 {
                self.fill(true)?;
            }
        }
    }

    /// Reads once. False when not blocking and nothing is there;
    /// `UnexpectedEof` when the keeper has closed the connection.
    fn fill(&mut self, block: bool) -> io::Result<bool> {
        let mut buf = vec![0u8; 65536];
        match fdpass::recv(self.stream.as_raw_fd(), &mut buf, !block) {
            Ok((0, _)) => Err(io::Error::new(io::ErrorKind::UnexpectedEof, "the keeper closed the connection")),
            Ok((n, fds)) => {
                self.inbuf.extend_from_slice(&buf[..n]);
                self.fds.extend(fds);
                Ok(true)
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock && !block => Ok(false),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                Err(io::Error::new(io::ErrorKind::TimedOut, "the keeper didn't answer in time"))
            }
            Err(e) => Err(e),
        }
    }

    /// Takes complete lines off the buffer: events are queued, lines tagged
    /// `req` go to `on_line` with their descriptors; returns the final reply
    /// to `req` (the one with `ok`) if it came.
    fn parse_lines(
        &mut self,
        req: Option<u64>,
        on_line: &mut dyn FnMut(&Value, &mut Vec<OwnedFd>),
    ) -> Option<(Value, Vec<OwnedFd>)> {
        while let Some(end) = self.inbuf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.inbuf.drain(..=end).collect();
            let Ok(v) = serde_json::from_slice::<Value>(&line) else { continue };
            let n = v["fds"].as_u64().unwrap_or(0) as usize;
            let mut fds: Vec<OwnedFd> = (0..n).filter_map(|_| self.fds.pop_front()).collect();
            if let Some(id) = v["exited"].as_str() {
                if let Some(status) = ExitStatus::from_json(&v) {
                    self.events.push_back(Exited { id: id.into(), pid: v["pid"].as_i64().unwrap_or(0) as i32, status });
                }
                continue;
            }
            if req.is_some() && v["req"].as_u64() == req {
                if v.get("ok").is_some() {
                    return Some((v, fds));
                }
                on_line(&v, &mut fds);
            }
        }
        None
    }

    fn request(
        &mut self,
        mut body: Value,
        mut on_line: impl FnMut(&Value, &mut Vec<OwnedFd>),
    ) -> io::Result<(Value, Vec<OwnedFd>)> {
        let tag = self.next_req;
        self.next_req += 1;
        body["req"] = tag.into();
        let mut bytes = serde_json::to_vec(&body).map_err(io::Error::other)?;
        bytes.push(b'\n');
        let mut sent = 0;
        while sent < bytes.len() {
            sent += fdpass::send(self.stream.as_raw_fd(), &bytes[sent..], &[], false)?;
        }
        loop {
            if let Some((reply, fds)) = self.parse_lines(Some(tag), &mut on_line) {
                if reply["ok"].as_bool() == Some(true) {
                    return Ok((reply, fds));
                }
                return Err(keeper_error(reply["error"].as_str().unwrap_or("keeper error").to_string()));
            }
            self.fill(true)?;
        }
    }
}

impl AsRawFd for Client {
    /// Readable when events (or stray data) arrive; then call
    /// [`Client::poll_events`].
    fn as_raw_fd(&self) -> RawFd {
        self.stream.as_raw_fd()
    }
}
