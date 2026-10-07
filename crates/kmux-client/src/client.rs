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

pub fn socket_path() -> PathBuf {
    if let Some(path) = std::env::var_os("KMUX_SOCKET").filter(|p| !p.is_empty()) {
        return PathBuf::from(path);
    }
    let home = std::env::var_os("HOME").unwrap_or_default();
    PathBuf::from(home).join("Library/Application Support/kmux/kmux.sock")
}

impl Kmux {
    /// Connects to kmux, starting it first if it isn't running.
    pub fn connect() -> Result<Kmux, Error> {
        let path = socket_path();
        if let Ok(stream) = UnixStream::connect(&path) {
            return Kmux::from_stream(stream);
        }
        launch()?;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match UnixStream::connect(&path) {
                Ok(stream) => return Kmux::from_stream(stream),
                Err(err) if Instant::now() >= deadline => {
                    return Err(Error::Unreachable(format!("kmux did not start ({}: {err})", path.display())))
                }
                Err(_) => sleep(Duration::from_millis(100)),
            }
        }
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

/// Starts kmux in the background: `$KMUX_APP` if set, else the app by bundle ID.
fn launch() -> Result<(), Error> {
    let mut open = Command::new("/usr/bin/open");
    open.arg("-g");
    match std::env::var_os("KMUX_APP") {
        Some(app) => open.arg("-a").arg(app),
        None => open.arg("-b").arg("dev.kanna.kmux"),
    };
    let status = open.status().map_err(|e| Error::Unreachable(format!("could not start kmux: {e}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::Unreachable("kmux isn't running and couldn't be started (is kmux.app installed? set KMUX_APP)".into()))
    }
}
