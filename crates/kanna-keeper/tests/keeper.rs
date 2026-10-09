//! The keeper end to end, through its client: a private state directory and
//! keeper per test, stopped by PID when the fixture drops.

use kanna_keeper::client::{Client, Exited, SpawnRequest};
use kanna_keeper::ExitStatus;
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_kanna-keeper");

static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

struct Fixture {
    dir: PathBuf,
    keeper: Option<Child>,
}

/// A short private directory, so the socket path fits in sun_path.
fn private_dir() -> PathBuf {
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("kk{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("home")).unwrap();
    dir.canonicalize().unwrap()
}

impl Fixture {
    fn new() -> Self {
        Self::with(&[])
    }

    fn with(env: &[(&str, &str)]) -> Self {
        let dir = private_dir();
        let mut cmd = Command::new(BIN);
        cmd.arg("--state-dir").arg(dir.join("state"));
        // Nothing from the terminal running the tests (it may be a Kanna or
        // kmux pane with its own sockets).
        cmd.env_clear();
        for key in ["PATH", "USER", "LOGNAME", "TMPDIR"] {
            if let Some(v) = std::env::var_os(key) {
                cmd.env(key, v);
            }
        }
        cmd.env("HOME", dir.join("home"))
            .env("SHELL", "/bin/sh")
            .env("PS1", "$ ")
            .env("KANNA_KEEPER_IDLE_MS", "30000")
            .env("KANNA_KEEPER_CLOSE_GRACE_MS", "500");
        for (k, v) in env {
            cmd.env(k, v);
        }
        let keeper = cmd.stdin(Stdio::null()).spawn().unwrap();
        let f = Fixture { dir, keeper: Some(keeper) };
        wait_for("the keeper's socket", || f.socket().exists() && Client::connect(&f.socket()).is_ok());
        f
    }

    fn state(&self) -> PathBuf {
        self.dir.join("state")
    }

    fn socket(&self) -> PathBuf {
        self.state().join("keeper.sock")
    }

    fn client(&self) -> Client {
        Client::connect(&self.socket()).unwrap()
    }

    fn keeper_pid(&self) -> i32 {
        self.keeper.as_ref().unwrap().id() as i32
    }

    /// Whether the keeper has exited (and so was reaped).
    fn keeper_exited(&mut self) -> bool {
        self.keeper.as_mut().unwrap().try_wait().unwrap().is_some()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(mut k) = self.keeper.take() {
            let _ = k.kill();
            let _ = k.wait();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[track_caller]
fn wait_for(what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn alive(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

/// Reads the pty until `needle` shows up; returns everything read.
#[track_caller]
fn read_until(master: &OwnedFd, needle: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut out = Vec::new();
    loop {
        if String::from_utf8_lossy(&out).contains(needle) {
            return String::from_utf8_lossy(&out).into_owned();
        }
        let left = deadline.saturating_duration_since(Instant::now());
        assert!(!left.is_zero(), "no {needle:?} in {:?}", String::from_utf8_lossy(&out));
        let mut pfd = libc::pollfd { fd: master.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        if unsafe { libc::poll(&mut pfd, 1, left.as_millis() as i32) } <= 0 {
            continue;
        }
        let mut buf = [0u8; 4096];
        let n = unsafe { libc::read(master.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
        assert!(n > 0, "pty closed; got {:?} without {needle:?}", String::from_utf8_lossy(&out));
        out.extend_from_slice(&buf[..n as usize]);
    }
}

fn write(master: &OwnedFd, s: &str) {
    let n = unsafe { libc::write(master.as_raw_fd(), s.as_ptr().cast(), s.len()) };
    assert_eq!(n, s.len() as isize);
}

#[track_caller]
fn next_exit(c: &mut Client) -> Exited {
    c.wait_event(Some(Duration::from_secs(5))).unwrap().expect("an exit event")
}

/// The next exit event, reading the pty meanwhile as the daemon would: on
/// macOS a process that exits with output unread waits for it to be read.
#[track_caller]
fn exit_reading(c: &mut Client, master: &OwnedFd) -> (Exited, String) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut out = Vec::new();
    loop {
        if let Some(e) = c.poll_events().unwrap().into_iter().next() {
            return (e, String::from_utf8_lossy(&out).into_owned());
        }
        assert!(Instant::now() < deadline, "no exit; read {:?}", String::from_utf8_lossy(&out));
        let mut pfd = libc::pollfd { fd: master.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        if unsafe { libc::poll(&mut pfd, 1, 50) } > 0 {
            let mut buf = [0u8; 4096];
            let n = unsafe { libc::read(master.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
            if n > 0 {
                out.extend_from_slice(&buf[..n as usize]);
            } else {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

#[test]
fn spawn_then_read_and_write_through_the_fd() {
    let f = Fixture::new();
    let mut c = f.client();
    let mut req = SpawnRequest::command("t1", "printf 'ready\\n'; read line; echo \"got:$line\"; sleep 30");
    req.cols = 100;
    req.rows = 30;
    let pty = c.spawn(&req).unwrap();
    assert!(pty.pid > 0 && alive(pty.pid));
    assert_eq!(pty.cmd, ["/bin/sh", "-c", req.command.as_deref().unwrap()]);
    read_until(&pty.master, "ready");
    write(&pty.master, "hello\n");
    read_until(&pty.master, "got:hello");
    // The size asked for is the pty's.
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    unsafe { libc::ioctl(pty.master.as_raw_fd(), libc::TIOCGWINSZ, &mut ws) };
    assert_eq!((ws.ws_col, ws.ws_row), (100, 30));
    c.close("t1").unwrap();
}

#[test]
fn shell_gets_the_login_convention_env_and_cwd() {
    let f = Fixture::new();
    let mut c = f.client();
    let mut req = SpawnRequest::shell("sh");
    req.shell = Some("/bin/sh".into());
    req.cwd = Some(f.dir.clone());
    req.env.insert("KANNA_TEST_VAR".into(), Some("hi there".into()));
    req.env.insert("KANNA_KEEPER_IDLE_MS".into(), None);
    let pty = c.spawn(&req).unwrap();
    assert_eq!(pty.cmd, ["/bin/sh"]);
    write(&pty.master, "echo \"[$0][$KANNA_TEST_VAR][${KANNA_KEEPER_IDLE_MS-unset}][$(pwd -P)][$TERM]\"\n");
    let out = read_until(&pty.master, "]\r\n");
    assert!(
        out.contains(&format!("[-sh][hi there][unset][{}][xterm-256color]", f.dir.display())),
        "{out:?}"
    );
    c.close("sh").unwrap();
}

#[test]
fn exit_codes_and_signals_are_the_commands_own() {
    let f = Fixture::new();
    let mut c = f.client();
    let p3 = c.spawn(&SpawnRequest::command("three", "exit 3")).unwrap();
    let e = next_exit(&mut c);
    assert_eq!(e, Exited { id: "three".into(), pid: p3.pid, status: ExitStatus::Code(3) });
    let p9 = c.spawn(&SpawnRequest::command("nine", "kill -9 $$")).unwrap();
    let e = next_exit(&mut c);
    assert_eq!(e, Exited { id: "nine".into(), pid: p9.pid, status: ExitStatus::Signal(9) });
    assert_eq!(e.status.shell_status(), 137);
    // A missing command is 127, as in a shell.
    let missing = c.spawn(&SpawnRequest::command("missing", "/nonexistent/x")).unwrap();
    assert_eq!(exit_reading(&mut c, &missing.master).0.status, ExitStatus::Code(127));
    // An interactive shell too: no login(1) in between to turn it into 0.
    let mut req = SpawnRequest::shell("shell");
    req.shell = Some("/bin/sh".into());
    let sh = c.spawn(&req).unwrap();
    write(&sh.master, "exit 4\n");
    assert_eq!(exit_reading(&mut c, &sh.master).0.status, ExitStatus::Code(4));
    for id in ["three", "nine", "missing", "shell"] {
        c.ack(id).unwrap();
    }
    assert!(c.adopt().unwrap().is_empty());
}

#[test]
fn adopt_after_the_client_goes_returns_the_same_process() {
    let f = Fixture::new();
    let (pid, cmd) = {
        let mut c = f.client();
        let pty = c
            .spawn(&SpawnRequest::command("t1", "sleep 0.5; echo while-away; read x; echo \"got:$x\"; sleep 30"))
            .unwrap();
        (pty.pid, pty.cmd)
        // The client and its copy of the master go here, as when the daemon dies.
    };
    std::thread::sleep(Duration::from_millis(1000));
    assert!(alive(pid));
    let mut c = f.client();
    let ptys = c.adopt().unwrap();
    assert_eq!(ptys.len(), 1);
    let pty = &ptys[0];
    assert_eq!((pty.id.as_str(), pty.pid, &pty.cmd, pty.exit), ("t1", pid, &cmd, None));
    // Written while nobody was connected: it waited in the pty.
    read_until(&pty.master, "while-away");
    write(&pty.master, "back\n");
    read_until(&pty.master, "got:back");
    // Adopting twice hands out another working copy.
    let again = c.adopt().unwrap();
    assert_eq!(again[0].pid, pid);
    c.close("t1").unwrap();
}

#[test]
fn an_exit_while_nobody_is_connected_waits_for_an_ack() {
    let f = Fixture::new();
    let (quiet, chatty) = {
        let mut c = f.client();
        let quiet = c.spawn(&SpawnRequest::command("quiet", "exit 6")).unwrap().pid;
        let chatty = c.spawn(&SpawnRequest::command("chatty", "sleep 0.2; echo last-words; exit 5")).unwrap().pid;
        (quiet, chatty)
    };
    wait_for("the quiet one to be reaped", || !alive(quiet));
    std::thread::sleep(Duration::from_millis(500));
    let mut c = f.client();
    // Sent on connect.
    assert_eq!(next_exit(&mut c), Exited { id: "quiet".into(), pid: quiet, status: ExitStatus::Code(6) });
    // The chatty one's last output is still in its pty (on macOS it can't
    // finish exiting until that is read).
    let ptys = c.adopt().unwrap();
    assert_eq!(ptys.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["chatty", "quiet"]);
    assert_eq!(ptys[1].exit, Some(ExitStatus::Code(6)));
    let (e, out) = exit_reading(&mut c, &ptys[0].master);
    assert_eq!(e, Exited { id: "chatty".into(), pid: chatty, status: ExitStatus::Code(5) });
    assert!(out.contains("last-words"), "{out:?}");
    drop(ptys);
    drop(c);
    // Not acknowledged: the next connection is told again.
    let mut c = f.client();
    let mut told = vec![next_exit(&mut c).id, next_exit(&mut c).id];
    told.sort();
    assert_eq!(told, ["chatty", "quiet"]);
    c.ack("chatty").unwrap();
    c.ack("quiet").unwrap();
    drop(c);
    let mut c = f.client();
    assert_eq!(c.wait_event(Some(Duration::from_millis(300))).unwrap(), None);
    assert!(c.adopt().unwrap().is_empty());
    // A running pty can't be acknowledged.
    c.spawn(&SpawnRequest::command("t2", "sleep 30")).unwrap();
    assert!(c.ack("t2").is_err());
    c.close("t2").unwrap();
}

/// Starts `script` (which prints `kid:PID` for a background child) and
/// returns its pid and the child's.
fn group(c: &mut Client, id: &str, script: &str) -> (i32, i32) {
    let pty = c.spawn(&SpawnRequest::command(id, script)).unwrap();
    let out = read_until(&pty.master, "\n");
    let kid = out.trim().strip_prefix("kid:").unwrap_or_else(|| panic!("{out:?}")).parse().unwrap();
    (pty.pid, kid)
}

#[test]
fn close_ends_the_process_group() {
    let f = Fixture::new();
    let mut c = f.client();
    let (sh, kid) = group(&mut c, "plain", "sleep 60 & echo \"kid:$!\"; wait");
    // These ignore SIGHUP, so they need the SIGKILL after the grace period.
    let (sh2, kid2) = group(&mut c, "stubborn", "trap '' HUP; sleep 60 & echo \"kid:$!\"; wait");
    assert!(alive(sh) && alive(kid) && alive(sh2) && alive(kid2));
    c.close("plain").unwrap();
    c.close("stubborn").unwrap();
    for pid in [sh, kid, sh2, kid2] {
        wait_for(&format!("{pid} to end"), || !alive(pid));
    }
    // Closing right after spawning works too (the reply waits for exec).
    for i in 0..20 {
        let id = format!("quick{i}");
        let pid = c.spawn(&SpawnRequest::command(&id, "sleep 60")).unwrap().pid;
        c.close(&id).unwrap();
        wait_for(&format!("{pid} to end"), || !alive(pid));
    }
    // Closed processes aren't reported as exits, and the ids are free.
    assert_eq!(c.wait_event(Some(Duration::from_millis(300))).unwrap(), None);
    assert!(c.adopt().unwrap().is_empty());
    assert!(c.close("plain").is_err());
    c.spawn(&SpawnRequest::command("plain", "true")).unwrap();
    assert_eq!(next_exit(&mut c).status, ExitStatus::Code(0));
}

#[test]
fn errors_are_replies() {
    let f = Fixture::new();
    let mut c = f.client();
    c.spawn(&SpawnRequest::command("t1", "sleep 30")).unwrap();
    let dup = c.spawn(&SpawnRequest::command("t1", "true")).unwrap_err();
    assert!(dup.to_string().contains("exists"), "{dup}");
    assert!(c.spawn(&SpawnRequest::command("", "true")).is_err());
    let mut both = SpawnRequest::command("t2", "true");
    both.shell = Some("/bin/sh".into());
    assert!(c.spawn(&both).is_err());
    assert!(c.close("nope").is_err());
    // The connection is still fine.
    assert_eq!(c.hello().unwrap().0, kanna_keeper::PROTOCOL_VERSION);
    c.close("t1").unwrap();
}

#[test]
fn private_files_and_the_peer_check() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let mut c = f.client();
    let (version, pid) = c.hello().unwrap();
    assert_eq!((version, pid), (kanna_keeper::PROTOCOL_VERSION, f.keeper_pid()));
    let pid_file = std::fs::read_to_string(f.state().join("keeper.pid")).unwrap();
    assert_eq!(pid_file.trim(), pid.to_string());
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&f.state()), 0o700);
    assert_eq!(mode(&f.socket()), 0o600);
    // A second keeper for the same directory leaves the first alone.
    let status = Command::new(BIN).arg("--state-dir").arg(f.state()).status().unwrap();
    assert!(status.success());
    assert_eq!(f.client().hello().unwrap().1, pid);

    // A keeper that expects another user refuses us.
    let uid = unsafe { libc::getuid() } + 1;
    let other = Fixture::with(&[("KANNA_KEEPER_TEST_PEER_UID", &uid.to_string())]);
    let mut c = Client::connect(&other.socket()).unwrap();
    assert!(c.hello().is_err());
}

#[test]
fn idle_exit_only_without_processes_or_clients() {
    let mut f = Fixture::with(&[("KANNA_KEEPER_IDLE_MS", "300")]);
    let mut c = f.client();
    c.spawn(&SpawnRequest::command("t1", "sleep 30")).unwrap();
    drop(c);
    // A live process keeps it.
    std::thread::sleep(Duration::from_millis(800));
    assert!(!f.keeper_exited());
    let mut c = f.client();
    c.close("t1").unwrap();
    // So does a client.
    std::thread::sleep(Duration::from_millis(800));
    assert!(!f.keeper_exited());
    drop(c);
    wait_for("the keeper to exit", || f.keeper_exited());
    assert!(!f.socket().exists());
    assert!(!f.state().join("keeper.pid").exists());
}

#[test]
fn connect_or_start_runs_one_detached_keeper() {
    let dir = private_dir();
    let state = dir.join("state");
    let mut c = Client::connect_or_start(&state, Path::new(BIN)).unwrap();
    let (_, pid) = c.hello().unwrap();
    let file: i32 = std::fs::read_to_string(state.join("keeper.pid")).unwrap().trim().parse().unwrap();
    assert_eq!(pid, file);
    let mut c2 = Client::connect_or_start(&state, Path::new(BIN)).unwrap();
    assert_eq!(c2.hello().unwrap().1, pid);
    let pty = c.spawn(&SpawnRequest::command("t1", "echo detached-ok; sleep 30")).unwrap();
    read_until(&pty.master, "detached-ok");
    c.close("t1").unwrap();
    // Stop the keeper this test started.
    unsafe { libc::kill(pid, libc::SIGTERM) };
    wait_for("the detached keeper to stop", || !alive(pid));
    let _ = std::fs::remove_dir_all(&dir);
}

struct KilledOnDrop(Child);

impl Drop for KilledOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn a_keeper_started_with_its_signals_blocked_still_stops_on_sigterm() {
    use std::os::unix::process::CommandExt;
    let dir = private_dir();
    let mut cmd = Command::new(BIN);
    cmd.arg("--state-dir").arg(dir.join("state")).stdin(Stdio::null());
    unsafe {
        cmd.pre_exec(|| {
            let mut set: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut set);
            for signo in [libc::SIGTERM, libc::SIGINT, libc::SIGCHLD] {
                libc::sigaddset(&mut set, signo);
            }
            libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
            Ok(())
        });
    }
    let mut keeper = KilledOnDrop(cmd.spawn().unwrap());
    let socket = dir.join("state/keeper.sock");
    wait_for("the keeper's socket", || Client::connect(&socket).is_ok());
    let mut c = Client::connect(&socket).unwrap();
    c.set_timeout(Some(Duration::from_secs(5))).unwrap();
    let pty = c.spawn(&SpawnRequest::command("t1", "exit 6")).unwrap();
    assert_eq!(exit_reading(&mut c, &pty.master).0.status, ExitStatus::Code(6));
    unsafe { libc::kill(keeper.0.id() as i32, libc::SIGTERM) };
    wait_for("the keeper to stop on SIGTERM", || keeper.0.try_wait().unwrap().is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_request_to_a_keeper_that_doesnt_answer_times_out() {
    let dir = private_dir();
    let socket = dir.join("mute.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let mut c = Client::connect(&socket).unwrap();
    let _held = listener.accept().unwrap();
    c.set_timeout(Some(Duration::from_millis(200))).unwrap();
    assert_eq!(c.hello().unwrap_err().kind(), std::io::ErrorKind::TimedOut);
    let _ = std::fs::remove_dir_all(&dir);
}

fn clean_command(state: &Path, home: &Path) -> Command {
    let mut cmd = Command::new(BIN);
    cmd.arg("--state-dir").arg(state).env_clear().stdin(Stdio::null());
    for key in ["PATH", "USER", "LOGNAME", "TMPDIR"] {
        if let Some(v) = std::env::var_os(key) {
            cmd.env(key, v);
        }
    }
    cmd.env("HOME", home).env("SHELL", "/bin/sh");
    cmd
}

fn with_inherited_fd(cmd: &mut Command, target: i32) -> OwnedFd {
    use std::os::fd::FromRawFd;
    use std::os::unix::process::CommandExt;
    let mut fds = [-1; 2];
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
    for fd in fds {
        unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
    }
    let read = fds[0];
    unsafe {
        cmd.pre_exec(move || {
            libc::dup2(read, target);
            Ok(())
        });
        libc::close(fds[1]);
        OwnedFd::from_raw_fd(read)
    }
}

const FD_PROBE: &str = "if [ -e /dev/fd/57 ]; then echo fd57-leaked; else echo fd57-clean; fi; sleep 30";

#[test]
fn spawned_processes_inherit_no_descriptor_the_keeper_was_started_with() {
    let dir = private_dir();
    let state = dir.join("state");
    let mut cmd = clean_command(&state, &dir.join("home"));
    let _pipe = with_inherited_fd(&mut cmd, 57);
    let _keeper = KilledOnDrop(cmd.spawn().unwrap());
    wait_for("the keeper's socket", || Client::connect(&state.join("keeper.sock")).is_ok());
    let mut c = Client::connect(&state.join("keeper.sock")).unwrap();
    let pty = c.spawn(&SpawnRequest::command("t1", FD_PROBE)).unwrap();
    read_until(&pty.master, "fd57-clean");
    c.close("t1").unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_detached_keeper_closes_what_it_inherited() {
    let dir = private_dir();
    let state = dir.join("state");
    let mut cmd = clean_command(&state, &dir.join("home"));
    cmd.arg("--detach");
    let _pipe = with_inherited_fd(&mut cmd, 57);
    assert!(cmd.status().unwrap().success());
    let mut c = Client::connect(&state.join("keeper.sock")).unwrap();
    let pid = c.hello().unwrap().1;
    let pty = c.spawn(&SpawnRequest::command("t1", FD_PROBE)).unwrap();
    let out = std::panic::catch_unwind(|| read_until(&pty.master, "fd57-clean"));
    c.close("t1").unwrap();
    unsafe { libc::kill(pid, libc::SIGTERM) };
    wait_for("the detached keeper to stop", || !alive(pid));
    out.unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn detach_fails_when_the_keeper_cannot_listen() {
    let dir = private_dir();
    let state = dir.join("state");
    std::fs::create_dir_all(state.join("keeper.sock/in-the-way")).unwrap();
    let out = clean_command(&state, &dir.join("home")).arg("--detach").output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{stderr}");
    assert!(stderr.contains("keeper.sock"), "{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}
