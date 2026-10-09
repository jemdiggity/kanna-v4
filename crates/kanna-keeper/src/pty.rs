//! Spawning a process on a new pty, in its own session with the pty as its
//! controlling terminal. From the relay keeper (feat/keeper), without
//! login(1).

use crate::ghostty_env::{self, Env};
use anyhow::{Context, Result};
use serde_json::{Map, Value};
use std::ffi::{CString, OsStr, OsString};
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

pub struct Spawned {
    pub pid: i32,
    /// Blocking and close-on-exec. The keeper never reads it.
    pub master: OwnedFd,
}

/// What to run.
pub enum What<'a> {
    /// An interactive shell, run as a login shell (argv[0] is `-zsh`).
    /// None is the user's login shell.
    Shell(Option<&'a str>),
    /// `/bin/sh -c LINE`.
    Command(&'a str),
}

#[cfg(any(target_os = "linux", target_os = "android"))]
const SIGNALS: libc::c_int = 65;
#[cfg(not(any(target_os = "linux", target_os = "android")))]
const SIGNALS: libc::c_int = 32;

static ORIGINAL_NOFILE: std::sync::OnceLock<libc::rlimit> = std::sync::OnceLock::new();

pub fn raise_nofile() {
    let mut limit: libc::rlimit = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) } != 0 {
        return;
    }
    let _ = ORIGINAL_NOFILE.set(limit);
    for cap in [limit.rlim_max, 10_240] {
        let raised = libc::rlimit { rlim_cur: limit.rlim_max.min(cap).max(limit.rlim_cur), rlim_max: limit.rlim_max };
        if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &raised) } == 0 {
            return;
        }
    }
}

fn cstring(bytes: &[u8]) -> Result<CString> {
    CString::new(bytes).context("argument or environment entry contains NUL")
}

struct Passwd {
    name: OsString,
    home: OsString,
    shell: OsString,
}

fn passwd() -> Option<Passwd> {
    let mut pw: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    let mut buf = vec![0 as libc::c_char; 16384];
    let rc = unsafe { libc::getpwuid_r(libc::getuid(), &mut pw, buf.as_mut_ptr(), buf.len(), &mut result) };
    if rc != 0 || result.is_null() {
        return None;
    }
    let field = |p: *const libc::c_char| {
        if p.is_null() {
            OsString::new()
        } else {
            OsStr::from_bytes(unsafe { std::ffi::CStr::from_ptr(p) }.to_bytes()).to_os_string()
        }
    };
    Some(Passwd { name: field(pw.pw_name), home: field(pw.pw_dir), shell: field(pw.pw_shell) })
}

/// The user's login shell: passwd, else $SHELL, else /bin/sh.
pub fn login_shell(env: &Env) -> OsString {
    passwd()
        .map(|p| p.shell)
        .filter(|s| !s.is_empty())
        .or_else(|| env.get(OsStr::new("SHELL")).filter(|s| !s.is_empty()).cloned())
        .unwrap_or_else(|| "/bin/sh".into())
}

/// The file execvp would run for `arg0` under the session's PATH.
fn resolve(arg0: &OsStr, env: &Env) -> PathBuf {
    if arg0.as_bytes().contains(&b'/') {
        return PathBuf::from(arg0);
    }
    let path = env
        .get(OsStr::new("PATH"))
        .cloned()
        .unwrap_or_else(|| "/usr/bin:/bin:/usr/sbin:/sbin".into());
    std::env::split_paths(&path)
        .map(|dir| dir.join(arg0))
        .find(|c| {
            std::fs::metadata(c)
                .map(|m| m.is_file() && std::os::unix::fs::PermissionsExt::mode(&m.permissions()) & 0o111 != 0)
                .unwrap_or(false)
        })
        .unwrap_or_else(|| PathBuf::from(arg0))
}

/// The argv a request runs, before Ghostty's planning: what `adopt` reports
/// as the pty's command.
pub fn argv(what: &What, env: &Env) -> Vec<OsString> {
    match what {
        What::Shell(Some(shell)) if !shell.is_empty() => vec![(*shell).into()],
        What::Shell(_) => vec![login_shell(env)],
        What::Command(line) => vec!["/bin/sh".into(), "-c".into(), (*line).into()],
    }
}

fn keeper_only(key: &OsStr) -> bool {
    let key = key.as_bytes();
    key == b"KANNA_STATE_DIR" || key == b"KANNA_KEEPER" || key.starts_with(b"KANNA_KEEPER_")
}

/// The keeper's own environment, with what login(1) would have set filled
/// in from passwd where it is missing.
pub fn base_env() -> Env {
    let mut env: Env = std::env::vars_os().filter(|(key, _)| !keeper_only(key)).collect();
    if let Some(pw) = passwd() {
        for (key, value) in [("HOME", pw.home), ("USER", pw.name.clone()), ("LOGNAME", pw.name), ("SHELL", pw.shell)] {
            if !value.is_empty() && env.get(OsStr::new(key)).is_none_or(|v| v.is_empty()) {
                env.insert(key.into(), value);
            }
        }
    }
    env
}

/// Plans the command and environment as Ghostty would, then spawns it on a
/// new pty of `rows` x `cols`, as a session leader with the pty as its
/// controlling terminal. A shell gets the login-shell convention (`-zsh`).
/// The exit status is the command's own: nothing sits in between.
pub fn spawn(
    what: &What,
    cwd: Option<&str>,
    request_env: Option<&Map<String, Value>>,
    rows: u16,
    cols: u16,
) -> Result<(Spawned, Vec<OsString>)> {
    let inherited = base_env();
    let argv = argv(what, &inherited);
    let ctx = ghostty_env::Context::for_env(&inherited);
    let home = inherited.get(OsStr::new("HOME")).cloned();
    let cwd = cwd
        .filter(|c| Path::new(c).is_dir())
        .map(OsString::from)
        .or(home)
        .unwrap_or_else(|| "/".into());
    let argv_str: Vec<String> = argv.iter().map(|a| a.to_string_lossy().into_owned()).collect();
    let plan = ghostty_env::plan(&argv_str, cwd.to_str(), request_env, inherited, &ctx);

    let program = cstring(resolve(&plan.argv[0], &plan.env).as_os_str().as_bytes())?;
    let mut args = plan.argv.iter().map(|a| cstring(a.as_bytes())).collect::<Result<Vec<_>>>()?;
    if matches!(what, What::Shell(_)) {
        // The login-shell convention, as login(1) and Ghostty's `exec -l`.
        let base = plan.argv[0].as_bytes().rsplit(|b| *b == b'/').next().unwrap_or_default();
        let mut dashed = b"-".to_vec();
        dashed.extend_from_slice(base.strip_prefix(b"-").unwrap_or(base));
        args[0] = cstring(&dashed)?;
    }
    let mut arg_ptrs: Vec<*const libc::c_char> = args.iter().map(|a| a.as_ptr()).collect();
    arg_ptrs.push(std::ptr::null());
    let envs = plan
        .env
        .iter()
        .filter(|(k, _)| !k.is_empty() && !k.as_bytes().contains(&b'='))
        .filter_map(|(k, v)| {
            let mut kv = k.as_bytes().to_vec();
            kv.push(b'=');
            kv.extend_from_slice(v.as_bytes());
            CString::new(kv).ok()
        })
        .collect::<Vec<_>>();
    let mut env_ptrs: Vec<*const libc::c_char> = envs.iter().map(|e| e.as_ptr()).collect();
    env_ptrs.push(std::ptr::null());
    let cwd_c = cstring(cwd.as_bytes())?;
    let home_c = plan.env.get(OsStr::new("HOME")).and_then(|h| CString::new(h.as_bytes()).ok());
    let failure = format!("kanna-keeper: cannot run {}\r\n", plan.argv[0].to_string_lossy());

    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    ws.ws_row = rows.max(1);
    ws.ws_col = cols.max(1);
    let (mut master, mut slave) = (0, 0);
    if unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), &mut ws) } < 0 {
        return Err(std::io::Error::last_os_error()).context("openpty");
    }
    // No child (this one or later ones) may inherit a master.
    crate::sig::set_cloexec(master);
    // Closes when the child execs (or gives up): until then it may not be
    // its own process group yet, so a close couldn't reach it.
    let mut exec_pipe = [-1; 2];
    if unsafe { libc::pipe(exec_pipe.as_mut_ptr()) } < 0 {
        let e = std::io::Error::last_os_error();
        unsafe {
            libc::close(master);
            libc::close(slave);
        }
        return Err(e).context("pipe");
    }
    crate::sig::set_cloexec(exec_pipe[0]);
    crate::sig::set_cloexec(exec_pipe[1]);
    let mut saved: libc::sigset_t = unsafe { std::mem::zeroed() };
    let pid = unsafe {
        let mut all: libc::sigset_t = std::mem::zeroed();
        libc::sigfillset(&mut all);
        libc::pthread_sigmask(libc::SIG_SETMASK, &all, &mut saved);
        libc::fork()
    };
    if pid != 0 {
        unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, &saved, std::ptr::null_mut()) };
    }
    if pid < 0 {
        let e = std::io::Error::last_os_error();
        unsafe {
            libc::close(master);
            libc::close(slave);
            libc::close(exec_pipe[0]);
            libc::close(exec_pipe[1]);
        }
        return Err(e).context("fork");
    }
    if pid == 0 {
        // Only async-signal-safe calls from here to exec.
        unsafe {
            let mut default: libc::sigaction = std::mem::zeroed();
            default.sa_sigaction = libc::SIG_DFL;
            libc::sigemptyset(&mut default.sa_mask);
            for signo in 1..SIGNALS {
                libc::sigaction(signo, &default, std::ptr::null_mut());
            }
            let mut empty: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut empty);
            libc::sigprocmask(libc::SIG_SETMASK, &empty, std::ptr::null_mut());
            libc::close(exec_pipe[0]);
            libc::close(master);
            libc::setsid();
            libc::ioctl(slave, libc::TIOCSCTTY as _, 0);
            libc::dup2(slave, 0);
            libc::dup2(slave, 1);
            libc::dup2(slave, 2);
            if slave > 2 {
                libc::close(slave);
            }
            if let Some(limit) = ORIGINAL_NOFILE.get() {
                libc::setrlimit(libc::RLIMIT_NOFILE, limit);
            }
            if libc::chdir(cwd_c.as_ptr()) != 0 {
                if let Some(home) = &home_c {
                    libc::chdir(home.as_ptr());
                }
            }
            libc::execve(program.as_ptr(), arg_ptrs.as_ptr(), env_ptrs.as_ptr());
            libc::write(2, failure.as_ptr().cast(), failure.len());
            libc::_exit(127);
        }
    }
    unsafe {
        libc::close(slave);
        libc::close(exec_pipe[1]);
        let mut byte = 0u8;
        while libc::read(exec_pipe[0], (&mut byte as *mut u8).cast(), 1) < 0
            && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
        {}
        libc::close(exec_pipe[0]);
    }
    Ok((Spawned { pid, master: unsafe { OwnedFd::from_raw_fd(master) } }, argv))
}
