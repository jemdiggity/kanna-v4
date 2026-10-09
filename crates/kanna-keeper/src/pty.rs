use crate::ghostty_env::{self, Env, Plan};
use anyhow::{bail, Context, Result};
use serde_json::{Map, Value};
use std::ffi::{CString, OsStr, OsString};
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

pub struct Spawned {
    pub pid: i32,
    pub master: OwnedFd,
    pub exec: OwnedFd,
}

pub enum What<'a> {
    Shell(Option<&'a str>),
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

const DEFAULT_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";

const SESSION_WIDE: &[&str] = &[
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "TMPDIR",
    "LANG",
    "__CF_USER_TEXT_ENCODING",
    "SSH_AUTH_SOCK",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XDG_RUNTIME_DIR",
    "XDG_SESSION_TYPE",
    "DBUS_SESSION_BUS_ADDRESS",
];

const LOGIN_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";

fn session_wide(key: &OsStr) -> bool {
    let key = key.as_bytes();
    SESSION_WIDE.iter().any(|wanted| wanted.as_bytes() == key) || key.starts_with(b"LC_")
}

#[cfg(debug_assertions)]
fn test_delay_us() -> u32 {
    std::env::var("KANNA_KEEPER_TEST_SPAWN_DELAY_MS")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .map_or(0, |ms| ms.saturating_mul(1000))
}

#[cfg(not(debug_assertions))]
fn test_delay_us() -> u32 {
    0
}

pub struct Base {
    env: Env,
    ctx: ghostty_env::Context,
    login_shell: OsString,
    delay_us: u32,
}

impl Base {
    pub fn load() -> Self {
        let user = ghostty_env::current_user();
        let mut env: Env = std::env::vars_os().filter(|(key, _)| session_wide(key)).collect();
        if let Some(user) = &user {
            let home = user.home.clone().map(PathBuf::into_os_string).unwrap_or_default();
            for (key, value) in
                [("HOME", home), ("USER", user.name.clone()), ("LOGNAME", user.name.clone()), ("SHELL", user.shell.clone())]
            {
                if !value.is_empty() && env.get(OsStr::new(key)).is_none_or(|v| v.is_empty()) {
                    env.insert(key.into(), value);
                }
            }
        }
        env.insert("PATH".into(), LOGIN_PATH.into());
        let login_shell = env
            .get(OsStr::new("SHELL"))
            .filter(|s| !s.is_empty())
            .cloned()
            .or_else(|| user.as_ref().map(|u| u.shell.clone()).filter(|s| !s.is_empty()))
            .unwrap_or_else(|| "/bin/sh".into());
        let ctx = ghostty_env::Context::for_env(&env, user);
        Base { env, ctx, login_shell, delay_us: test_delay_us() }
    }

    pub fn argv(&self, what: &What) -> Vec<OsString> {
        match what {
            What::Shell(Some(shell)) if !shell.is_empty() => vec![(*shell).into()],
            What::Shell(_) => vec![self.login_shell.clone()],
            What::Command(line) => vec![self.login_shell.clone(), "-l".into(), "-c".into(), (*line).into()],
        }
    }
}

struct Image {
    _args: Vec<CString>,
    _envs: Vec<CString>,
    argv: Vec<*const libc::c_char>,
    envp: Vec<*const libc::c_char>,
}

impl Image {
    fn new(plan: &Plan, login: bool) -> Result<Self> {
        let mut args = plan.argv.iter().map(|a| cstring(a.as_bytes())).collect::<Result<Vec<_>>>()?;
        if login {
            let base = plan.argv[0].as_bytes().rsplit(|b| *b == b'/').next().unwrap_or_default();
            let mut dashed = b"-".to_vec();
            dashed.extend_from_slice(base.strip_prefix(b"-").unwrap_or(base));
            args[0] = cstring(&dashed)?;
        }
        let envs = plan
            .env
            .iter()
            .filter(|(k, _)| !k.is_empty() && !k.as_bytes().contains(&b'=') && k.as_bytes() != b"PWD")
            .filter_map(|(k, v)| {
                let mut kv = k.as_bytes().to_vec();
                kv.push(b'=');
                kv.extend_from_slice(v.as_bytes());
                CString::new(kv).ok()
            })
            .collect::<Vec<_>>();
        let mut argv: Vec<*const libc::c_char> = args.iter().map(|a| a.as_ptr()).collect();
        argv.push(std::ptr::null());
        let mut envp: Vec<*const libc::c_char> = envs.iter().map(|e| e.as_ptr()).collect();
        envp.push(std::ptr::null());
        envp.push(std::ptr::null());
        Ok(Image { _args: args, _envs: envs, argv, envp })
    }

    fn set_pwd(&mut self, pwd: *const libc::c_char) {
        let slot = self.envp.len() - 2;
        self.envp[slot] = pwd;
    }
}

pub fn spawn(
    base: &Base,
    what: &What,
    cwd: Option<&str>,
    request_env: Option<&Map<String, Value>>,
    rows: u16,
    cols: u16,
) -> Result<(Spawned, Vec<OsString>)> {
    let argv = base.argv(what);
    let argv_str: Vec<String> = argv.iter().map(|a| a.to_string_lossy().into_owned()).collect();
    let plan = ghostty_env::plan(&argv_str, false, request_env, base.env.clone(), &base.ctx);
    let path = plan.env.get(OsStr::new("PATH")).cloned().unwrap_or_else(|| DEFAULT_PATH.into());
    let candidates = ghostty_env::candidates(&plan.argv[0], &path);
    if candidates.is_empty() {
        bail!("{} is not on PATH", plan.argv[0].to_string_lossy());
    }
    let apple_bash = Path::new("/bin/bash");
    let plain = (base.ctx.macos && candidates.iter().any(|c| c == apple_bash))
        .then(|| ghostty_env::plan(&argv_str, true, request_env, base.env.clone(), &base.ctx));
    let login = matches!(what, What::Shell(_));
    let mut image = Image::new(&plan, login)?;
    let mut plain = plain.map(|p| Image::new(&p, login)).transpose()?;
    let programs = candidates
        .iter()
        .map(|c| Ok((cstring(c.as_os_str().as_bytes())?, c == apple_bash)))
        .collect::<Result<Vec<_>>>()?;
    let home = plan.env.get(OsStr::new("HOME")).filter(|h| !h.is_empty());
    let mut dirs = Vec::new();
    for dir in cwd.filter(|c| !c.is_empty()).map(OsStr::new).into_iter().chain(home.map(OsString::as_os_str)) {
        let mut pwd = b"PWD=".to_vec();
        pwd.extend_from_slice(dir.as_bytes());
        dirs.push((cstring(dir.as_bytes())?, cstring(&pwd)?));
    }
    dirs.push((c"/".to_owned(), c"PWD=/".to_owned()));
    let delay_us = base.delay_us;

    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    ws.ws_row = rows.max(1);
    ws.ws_col = cols.max(1);
    let (mut master, mut slave) = (0, 0);
    if unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), &mut ws) } < 0 {
        return Err(std::io::Error::last_os_error()).context("openpty");
    }
    crate::sig::set_cloexec(master);
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
            if delay_us > 0 {
                libc::usleep(delay_us);
            }
            let mut pwd = std::ptr::null();
            for (dir, dir_pwd) in dirs.iter() {
                if libc::chdir(dir.as_ptr()) == 0 {
                    pwd = dir_pwd.as_ptr();
                    break;
                }
            }
            image.set_pwd(pwd);
            if let Some(plain) = plain.as_mut() {
                plain.set_pwd(pwd);
            }
            let mut error = 0;
            for (program, apple) in programs.iter() {
                let chosen = match plain.as_ref() {
                    Some(plain) if *apple => plain,
                    _ => &image,
                };
                libc::execve(program.as_ptr(), chosen.argv.as_ptr(), chosen.envp.as_ptr());
                let e = *crate::sig::errno();
                if error == 0 || (error == libc::ENOENT || error == libc::ENOTDIR) && e != libc::ENOENT && e != libc::ENOTDIR {
                    error = e;
                }
            }
            libc::write(exec_pipe[1], (&error as *const libc::c_int).cast(), std::mem::size_of::<libc::c_int>());
            libc::_exit(127);
        }
    }
    unsafe {
        libc::close(slave);
        libc::close(exec_pipe[1]);
    }
    crate::sig::set_nonblocking(exec_pipe[0]);
    let spawned = unsafe {
        Spawned { pid, master: OwnedFd::from_raw_fd(master), exec: OwnedFd::from_raw_fd(exec_pipe[0]) }
    };
    Ok((spawned, argv))
}
