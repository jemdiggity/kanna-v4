//! Where terminals' programs come from. For now the daemon spawns each one
//! on a pty itself (`LocalSpawner`); the keeper takes this over in step 0.5,
//! behind the same `Spawner`.

use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Command, Stdio};

/// What to run, and on how big a terminal.
pub struct Spec {
    pub program: String,
    pub args: Vec<String>,
    /// The program's `argv[0]`, if not its path (`-zsh` for a login shell).
    pub arg0: Option<String>,
    pub cwd: Option<String>,
    /// Set in the program's environment, over the daemon's own.
    pub env: Vec<(String, String)>,
    /// Taken out of it.
    pub unset: Vec<String>,
    pub cols: u16,
    pub rows: u16,
}

/// A running program: its pid and the pty's master side.
pub struct Spawned {
    pub pid: i32,
    pub master: OwnedFd,
}

/// Called once, from any thread, with the program's exit code (128 + the
/// signal if a signal ended it).
pub type OnExit = Box<dyn FnOnce(i32) + Send>;

pub trait Spawner {
    fn spawn(&self, spec: &Spec, exited: OnExit) -> io::Result<Spawned>;
    /// Hangs up on a program: the pane is closing.
    fn hang_up(&self, pid: i32);
}

/// Spawns programs as the daemon's own children.
pub struct LocalSpawner;

impl Spawner for LocalSpawner {
    fn spawn(&self, spec: &Spec, exited: OnExit) -> io::Result<Spawned> {
        let (master, slave) = open_pty(spec.cols, spec.rows)?;
        let mut command = Command::new(&spec.program);
        command.args(&spec.args);
        if let Some(arg0) = &spec.arg0 {
            command.arg0(arg0);
        }
        if let Some(cwd) = spec.cwd.as_deref().filter(|d| std::path::Path::new(d).is_dir()) {
            command.current_dir(cwd);
        }
        for name in &spec.unset {
            command.env_remove(name);
        }
        command.envs(spec.env.iter().map(|(k, v)| (k, v)));
        command.stdin(Stdio::from(slave.try_clone()?)).stdout(Stdio::from(slave.try_clone()?)).stderr(Stdio::from(slave));
        // SAFETY: only async-signal-safe calls between fork and exec.
        unsafe {
            command.pre_exec(|| {
                // A session of its own, with the pty as its controlling terminal.
                if libc::setsid() < 0 {
                    return Err(io::Error::last_os_error());
                }
                if libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                    return Err(io::Error::last_os_error());
                }
                // The daemon blocks the signals it waits for; the program shouldn't.
                let mut none: libc::sigset_t = std::mem::zeroed();
                libc::sigemptyset(&mut none);
                libc::pthread_sigmask(libc::SIG_SETMASK, &none, std::ptr::null_mut());
                Ok(())
            });
        }
        let mut child = command.spawn()?;
        drop(command); // closes the parent's copies of the slave side
        let pid = child.id() as i32;
        std::thread::Builder::new().name(format!("kanna.wait.{pid}")).spawn(move || {
            let code = match child.wait() {
                Ok(status) => status.code().or_else(|| status.signal().map(|s| 128 + s)).unwrap_or(1),
                Err(_) => 1,
            };
            exited(code);
        })?;
        Ok(Spawned { pid, master })
    }

    fn hang_up(&self, pid: i32) {
        // SAFETY: plain syscalls. The program leads its own process group.
        unsafe {
            libc::kill(-pid, libc::SIGHUP);
            libc::kill(pid, libc::SIGHUP);
        }
    }
}

/// A new pty of `cols`×`rows`: (master, slave). The master isn't inherited.
fn open_pty(cols: u16, rows: u16) -> io::Result<(OwnedFd, OwnedFd)> {
    let (mut master, mut slave) = (-1, -1);
    let mut size = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
    // SAFETY: valid out-pointers; no name or termios.
    if unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), &mut size) } < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: openpty returned two open descriptors we now own.
    let (master, slave) = unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
    // SAFETY: a valid descriptor.
    unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) };
    Ok((master, slave))
}

/// Tells the pty (and so the program, by SIGWINCH) its new size.
pub fn set_size(master: &OwnedFd, cols: u16, rows: u16, width: u16, height: u16) {
    let size = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: width, ws_ypixel: height };
    // SAFETY: a valid descriptor and winsize.
    unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ as _, &size) };
}
