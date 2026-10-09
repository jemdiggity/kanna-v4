use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const DAEMON: &str = env!("CARGO_BIN_EXE_kanna-daemon");

fn keeper_binary() -> PathBuf {
    let keeper = Path::new(DAEMON).with_file_name("kanna-keeper");
    if !keeper.exists() {
        let status = Command::new(env!("CARGO")).args(["build", "-p", "kanna-keeper"]).status().unwrap();
        assert!(status.success(), "building kanna-keeper failed");
    }
    keeper
}

struct Instance {
    dir: PathBuf,
    daemon: Child,
}

impl Instance {
    fn start() -> Instance {
        let dir = std::env::temp_dir().join(format!("kd{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home")).unwrap();
        let dir = dir.canonicalize().unwrap();
        let log = std::fs::File::create(dir.join("daemon.log")).unwrap();
        let mut command = Command::new(DAEMON);
        command.env_clear();
        for key in ["PATH", "USER", "LOGNAME", "TMPDIR"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        command
            .env("HOME", dir.join("home"))
            .env("SHELL", "/bin/sh")
            .env("KANNA_SOCKET", dir.join("kanna.sock"))
            .env("KANNA_STATE_DIR", dir.join("state"))
            .env("KANNA_KEEPER", keeper_binary())
            .env("KANNA_KEEPER_CLOSE_GRACE_MS", "300")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log);
        let instance = Instance { daemon: command.spawn().unwrap(), dir };
        wait_for("the daemon's socket", || UnixStream::connect(instance.socket()).is_ok());
        instance
    }

    fn socket(&self) -> PathBuf {
        self.dir.join("kanna.sock")
    }

    fn keeper_pid(&self) -> i32 {
        std::fs::read_to_string(self.dir.join("state/keeper.pid")).unwrap().trim().parse().unwrap()
    }

    fn call(&self, cmd: &str, args: Value) -> Value {
        let mut stream = UnixStream::connect(self.socket()).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let mut line = json!({"id": 1, "cmd": cmd, "args": args}).to_string();
        line.push('\n');
        stream.write_all(line.as_bytes()).unwrap();
        let mut reply = String::new();
        BufReader::new(stream).read_line(&mut reply).unwrap();
        serde_json::from_str(&reply).unwrap()
    }

    fn pane(&self, name: &str) -> Value {
        let list = self.call("list", json!({}));
        list["panes"].as_array().unwrap().iter().find(|p| p["name"] == name).cloned().unwrap_or(Value::Null)
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.dir.join("daemon.log")).unwrap_or_default()
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
        if let Ok(pid) = std::fs::read_to_string(self.dir.join("state/keeper.pid")) {
            if let Ok(pid) = pid.trim().parse::<i32>() {
                if pid > 0 {
                    unsafe { libc::kill(pid, libc::SIGTERM) };
                }
            }
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[track_caller]
fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn alive(pid: i64) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[test]
fn terminals_come_from_the_keeper_and_the_daemon_stops_when_it_goes() {
    let mut instance = Instance::start();

    let opened = instance.call("open", json!({"type": "term", "name": "code", "cmd": "exit 3"}));
    assert_eq!(opened["ok"], true, "{opened}");
    wait_for("code to exit", || instance.pane("code")["state"] == "exited");
    let code = instance.pane("code");
    assert_eq!(code["exitCode"], 3, "{code}");
    assert!(code["pid"].as_i64().unwrap() > 0, "{code}");

    let opened = instance.call("open", json!({"type": "term", "name": "shell"}));
    let pid = opened["pane"]["pid"].as_i64().unwrap_or_else(|| panic!("no pid in {opened}"));
    assert!(alive(pid));
    assert_eq!(instance.pane("shell")["pid"], pid);
    instance.call("send", json!({"pane": "shell", "text": "echo keeper-$((40+2))"}));
    wait_for("the shell's output", || instance.call("read", json!({"pane": "shell"}))["text"].as_str().unwrap_or("").contains("keeper-42"));

    let restarted = instance.call("restart", json!({"pane": "shell"}));
    let new_pid = restarted["pane"]["pid"].as_i64().unwrap_or_else(|| panic!("no pid in {restarted}"));
    assert_ne!(new_pid, pid);
    wait_for("the old shell to end", || !alive(pid));

    instance.call("close", json!({"pane": "shell"}));
    wait_for("the closed shell to end", || !alive(new_pid));

    let keeper = instance.keeper_pid();
    unsafe { libc::kill(keeper, libc::SIGTERM) };
    wait_for("the daemon to stop", || instance.daemon.try_wait().unwrap().is_some());
    assert!(!instance.daemon.try_wait().unwrap().unwrap().success());
    assert!(instance.log().contains("went away"), "{}", instance.log());
}
