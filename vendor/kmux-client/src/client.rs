//! The kmux control socket: newline-delimited JSON requests and
//! replies (docs/kmux-spec.md §7).

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant};

pub struct Kmux {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next_id: u64,
}

#[derive(Debug)]
pub enum Error {
    /// kmux isn't running and couldn't be started.
    Unreachable(String),
    /// kmux answered with an error.
    Mux { code: String, message: String },
}

/// Which kmux to talk to: an instance name and its socket. The default
/// instance listens on `kmux.sock`; one named `work` on `kmux-work.sock`.
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub instance: String,
    pub socket: PathBuf,
    /// If kmux has to be started, start it without bringing it to the front
    /// (`--bg`, or `KMUX_BG=1`).
    pub background: bool,
}

fn background_from_env() -> bool {
    std::env::var("KMUX_BG").is_ok_and(|v| v == "1")
}

pub const DEFAULT_INSTANCE: &str = "default";

/// `~/Library/Application Support/kmux`, where the sockets live.
pub fn socket_dir() -> PathBuf {
    let home = std::env::var_os("HOME").unwrap_or_default();
    PathBuf::from(home).join("Library/Application Support/kmux")
}

pub fn instance_socket(name: &str) -> PathBuf {
    socket_dir().join(if name == DEFAULT_INSTANCE { "kmux.sock".to_string() } else { format!("kmux-{name}.sock") })
}

/// Instance names: letters, digits, `-` and `_`, up to 32.
pub fn valid_instance(name: &str) -> bool {
    !name.is_empty() && name.len() <= 32 && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

impl Target {
    /// `$KMUX_SOCKET` if set (kmux sets it in its terminals), else the
    /// socket of `$KMUX_INSTANCE`, else the default instance.
    pub fn from_env() -> Target {
        let instance = std::env::var("KMUX_INSTANCE").ok().filter(|n| !n.is_empty()).unwrap_or_else(|| DEFAULT_INSTANCE.into());
        let socket = std::env::var_os("KMUX_SOCKET").filter(|p| !p.is_empty()).map(PathBuf::from);
        Target { socket: socket.unwrap_or_else(|| instance_socket(&instance)), instance, background: background_from_env() }
    }

    /// An instance chosen by name (`--instance`), whatever the environment says.
    pub fn named(name: &str) -> Target {
        Target { instance: name.into(), socket: instance_socket(name), background: background_from_env() }
    }

    /// The running instances, default first, from the sockets that answer.
    pub fn running() -> Vec<Target> {
        let mut names: Vec<String> = std::fs::read_dir(socket_dir())
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let file = entry.file_name().into_string().ok()?;
                if file == "kmux.sock" {
                    return Some(DEFAULT_INSTANCE.to_string());
                }
                Some(file.strip_prefix("kmux-")?.strip_suffix(".sock")?.to_string())
            })
            .collect();
        names.sort_by_key(|n| (n != DEFAULT_INSTANCE, n.clone()));
        names.into_iter().map(|n| Target::named(&n)).filter(|t| UnixStream::connect(&t.socket).is_ok()).collect()
    }
}

/// The socket the environment points at (see `Target::from_env`).
pub fn socket_path() -> PathBuf {
    Target::from_env().socket
}

impl Kmux {
    /// Connects to kmux, starting it first if it isn't running.
    pub fn connect() -> Result<Kmux, Error> {
        Kmux::connect_to(&Target::from_env())
    }

    /// Connects to `target`, starting that instance first if it isn't running.
    pub fn connect_to(target: &Target) -> Result<Kmux, Error> {
        if let Some(mux) = Kmux::try_connect(target) {
            return Ok(mux);
        }
        launch(target)?;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match UnixStream::connect(&target.socket) {
                Ok(stream) => return Kmux::from_stream(stream),
                Err(err) if Instant::now() >= deadline => {
                    return Err(Error::Unreachable(format!("kmux did not start ({}: {err})", target.socket.display())))
                }
                Err(_) => sleep(Duration::from_millis(100)),
            }
        }
    }

    /// Connects to `target` only if it is already running.
    pub fn try_connect(target: &Target) -> Option<Kmux> {
        UnixStream::connect(&target.socket).ok().and_then(|stream| Kmux::from_stream(stream).ok())
    }

    fn from_stream(stream: UnixStream) -> Result<Kmux, Error> {
        let writer = stream.try_clone().map_err(|e| Error::Unreachable(e.to_string()))?;
        Ok(Kmux { reader: BufReader::new(stream), writer, next_id: 1 })
    }

    /// Sends one request and returns the reply, minus `id` and `ok`.
    pub fn call(&mut self, cmd: &str, args: Value) -> Result<Value, Error> {
        let id = self.next_id;
        self.next_id += 1;
        let mut line = json!({ "id": id, "cmd": cmd, "args": args }).to_string();
        line.push('\n');
        let lost = |e: std::io::Error| Error::Unreachable(format!("lost connection to kmux: {e}"));
        self.writer.write_all(line.as_bytes()).map_err(lost)?;
        let mut reply = String::new();
        if self.reader.read_line(&mut reply).map_err(lost)? == 0 {
            return Err(Error::Unreachable("kmux closed the connection".into()));
        }
        let mut reply: Value =
            serde_json::from_str(&reply).map_err(|e| Error::Unreachable(format!("bad reply from kmux: {e}")))?;
        if reply["ok"] != json!(true) {
            let error = &reply["error"];
            return Err(Error::Mux {
                code: error["code"].as_str().unwrap_or("bad_request").into(),
                message: error["message"].as_str().unwrap_or("unknown error").into(),
            });
        }
        if let Some(fields) = reply.as_object_mut() {
            fields.remove("id");
            fields.remove("ok");
        }
        Ok(reply)
    }
}

/// Starts the instance (in the background with `target.background`), as
/// a new copy of the app (other instances may be running): `$KMUX_APP` if set, else the app by bundle ID.
fn launch(target: &Target) -> Result<(), Error> {
    let mut open = Command::new("/usr/bin/open");
    open.arg("-n");
    if target.background {
        open.arg("-g");
    }
    if target.socket != instance_socket(&target.instance) {
        open.arg("--env").arg(format!("KMUX_SOCKET={}", target.socket.display()));
    }
    match std::env::var_os("KMUX_APP") {
        Some(app) => open.arg("-a").arg(app),
        None => open.arg("-b").arg("dev.kanna.kmux"),
    };
    open.args(["--args", "--instance", &target.instance]);
    if target.background {
        open.arg("--bg");
    }
    let status = open.status().map_err(|e| Error::Unreachable(format!("could not start kmux: {e}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::Unreachable("kmux isn't running and couldn't be started (is kmux.app installed? set KMUX_APP)".into()))
    }
}
