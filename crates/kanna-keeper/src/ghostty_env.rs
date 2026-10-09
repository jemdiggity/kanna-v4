use serde_json::{Map, Value};
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

pub type Env = BTreeMap<OsString, OsString>;

const SHELL_FEATURES: &str = "cursor:blink,path,title";

#[derive(Clone, Debug)]
pub struct Resources {
    pub share: PathBuf,
    version: Option<String>,
    integration: bool,
    zsh: bool,
    bash: bool,
}

impl Resources {
    pub fn at(share: PathBuf) -> Self {
        let integ = share.join("ghostty/shell-integration");
        let version = std::fs::read_to_string(share.join("ghostty-version"))
            .ok()
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty());
        Resources {
            version,
            integration: integ.is_dir(),
            zsh: integ.join("zsh").is_dir(),
            bash: integ.join("bash/ghostty.bash").is_file(),
            share,
        }
    }
    fn ghostty(&self) -> PathBuf {
        self.share.join("ghostty")
    }
    fn version(&self) -> Option<String> {
        self.version.clone()
    }
}

fn has_terminfo(share: &Path) -> bool {
    ["78/xterm-ghostty", "x/xterm-ghostty"]
        .iter()
        .any(|entry| share.join("terminfo").join(entry).is_file())
}

pub fn find_resources(env: &Env) -> Option<Resources> {
    if let Some(share) = get(env, "GHOSTTY_RESOURCES_DIR")
        .and_then(|dir| Path::new(dir).parent())
        .filter(|share| has_terminfo(share))
    {
        return Some(Resources::at(share.to_path_buf()));
    }
    if let Some(dir) = std::env::var_os("KANNA_KEEPER_RESOURCES_DIR").filter(|d| !d.is_empty()) {
        let share = PathBuf::from(dir);
        return has_terminfo(&share).then(|| Resources::at(share));
    }
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    for dir in exe.ancestors().skip(1) {
        let mut candidates = vec![dir.join("share")];
        if cfg!(target_os = "macos") {
            candidates.insert(0, dir.join("Contents/Resources"));
        }
        if let Some(share) = candidates.into_iter().find(|c| has_terminfo(c)) {
            return Some(Resources::at(share));
        }
    }
    None
}

#[derive(Clone, Debug)]
pub struct User {
    pub name: OsString,
    pub home: Option<PathBuf>,
    pub shell: OsString,
}

pub fn current_user() -> Option<User> {
    let mut pw: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    let mut buf = vec![0 as libc::c_char; 16384];
    let rc = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            &mut pw,
            buf.as_mut_ptr(),
            buf.len(),
            &mut result,
        )
    };
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
    let home = Some(PathBuf::from(field(pw.pw_dir))).filter(|h| !h.as_os_str().is_empty());
    Some(User { name: field(pw.pw_name), home, shell: field(pw.pw_shell) })
}

pub struct Context {
    pub resources: Option<Resources>,
    pub exe_dir: Option<PathBuf>,
    pub macos: bool,
    pub user: Option<User>,
}

impl Context {
    pub fn for_env(env: &Env, user: Option<User>) -> Self {
        Self {
            resources: find_resources(env),
            exe_dir: std::env::current_exe()
                .ok()
                .and_then(|e| e.parent().map(Path::to_path_buf)),
            macos: cfg!(target_os = "macos"),
            user,
        }
    }
}

#[derive(Debug)]
pub struct Plan {
    pub argv: Vec<OsString>,
    pub env: Env,
}

fn set(env: &mut Env, key: &str, value: impl Into<OsString>) {
    env.insert(key.into(), value.into());
}
fn get<'a>(env: &'a Env, key: &str) -> Option<&'a OsStr> {
    env.get(OsStr::new(key)).map(OsString::as_os_str)
}
fn remove(env: &mut Env, key: &str) {
    env.remove(OsStr::new(key));
}
fn join(a: &OsStr, b: &OsStr) -> OsString {
    let mut out = a.to_os_string();
    out.push(":");
    out.push(b);
    out
}

fn append_env(current: &OsStr, value: &OsStr) -> OsString {
    if current.is_empty() {
        value.to_os_string()
    } else {
        join(current, value)
    }
}

fn prepend_env(current: &OsStr, value: &OsStr) -> OsString {
    if current.is_empty() {
        value.to_os_string()
    } else {
        join(value, current)
    }
}

pub fn plan(
    argv: &[String],
    runs_bin_bash: bool,
    request_env: Option<&Map<String, Value>>,
    inherited: Env,
    ctx: &Context,
) -> Plan {
    let mut env = inherited;

    if ctx.macos {
        if get(&env, "__XCODE_BUILT_PRODUCTS_DIR_PATHS").is_some() {
            for key in [
                "__XCODE_BUILT_PRODUCTS_DIR_PATHS",
                "__XPC_DYLD_LIBRARY_PATH",
                "DYLD_FRAMEWORK_PATH",
                "DYLD_INSERT_LIBRARIES",
                "DYLD_LIBRARY_PATH",
                "LD_LIBRARY_PATH",
                "SECURITYSESSIONID",
                "XPC_SERVICE_NAME",
            ] {
                remove(&mut env, key);
            }
        }
        remove(&mut env, "GHOSTTY_MAC_LAUNCH_SOURCE");
    }
    remove(&mut env, "GHOSTTY_LOG");
    remove(&mut env, "GHOSTTY_SURFACE_ID");

    if let Some(res) = &ctx.resources {
        set(&mut env, "GHOSTTY_RESOURCES_DIR", res.ghostty());
        set(&mut env, "TERM", "xterm-ghostty");
        set(&mut env, "COLORTERM", "truecolor");
        set(&mut env, "TERMINFO", res.share.join("terminfo"));
    } else {
        if get(&env, "TERM").is_none_or(OsStr::is_empty) {
            set(&mut env, "TERM", "xterm-256color");
        }
        set(&mut env, "COLORTERM", "truecolor");
    }

    if let Some(dir) = &ctx.exe_dir {
        let dir = dir.as_os_str();
        match get(&env, "PATH") {
            Some(path) if std::env::split_paths(path).any(|p| p.as_os_str() == dir) => {}
            Some(path) => {
                let value = append_env(path, dir);
                set(&mut env, "PATH", value);
            }
            None => set(&mut env, "PATH", dir),
        }
    }

    if ctx.macos {
        if let Some(res) = &ctx.resources {
            let data = append_env(
                get(&env, "XDG_DATA_DIRS").unwrap_or(OsStr::new("/usr/local/share:/usr/share")),
                res.share.as_os_str(),
            );
            set(&mut env, "XDG_DATA_DIRS", data);
            let man = join(
                get(&env, "MANPATH").unwrap_or_default(),
                res.share.join("man").as_os_str(),
            );
            set(&mut env, "MANPATH", man);
        }
    }

    set(&mut env, "TERM_PROGRAM", "ghostty");
    if let Some(version) = ctx.resources.as_ref().and_then(Resources::version) {
        set(&mut env, "TERM_PROGRAM_VERSION", version);
    }
    remove(&mut env, "VTE_VERSION");
    set(&mut env, "GHOSTTY_SHELL_FEATURES", SHELL_FEATURES);

    for (key, value) in request_env.into_iter().flatten() {
        match value.as_str() {
            Some(value) => {
                env.insert(key.into(), value.into());
            }
            None if value.is_null() => {
                env.remove(OsStr::new(key));
            }
            None => {}
        }
    }

    let mut argv = argv.iter().map(OsString::from).collect::<Vec<_>>();
    let shell = detect_shell(&argv, ctx.macos && runs_bin_bash);
    if let (Some(res), Some(shell)) = (&ctx.resources, shell) {
        if let Some(new_argv) = integrate(shell, &argv, res, &mut env, ctx) {
            argv = new_argv;
        }
    }

    Plan { argv, env }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shell {
    Bash,
    Elvish,
    Fish,
    Nushell,
    Zsh,
}

fn basename(arg: &OsStr) -> &[u8] {
    let bytes = arg.as_bytes();
    let base = bytes.rsplit(|b| *b == b'/').next().unwrap_or(bytes);
    base.strip_prefix(b"-").unwrap_or(base)
}

pub fn candidates(arg0: &OsStr, path: &OsStr) -> Vec<PathBuf> {
    if arg0.as_bytes().contains(&b'/') {
        return vec![PathBuf::from(arg0)];
    }
    std::env::split_paths(path).filter(|dir| dir.is_absolute()).map(|dir| dir.join(arg0)).collect()
}

fn detect_shell(argv: &[OsString], apple_bash: bool) -> Option<Shell> {
    let arg0 = argv.first()?;
    Some(match basename(arg0) {
        b"bash" if apple_bash => return None,
        b"bash" => Shell::Bash,
        b"elvish" => Shell::Elvish,
        b"fish" => Shell::Fish,
        b"nu" => Shell::Nushell,
        b"zsh" => Shell::Zsh,
        _ => return None,
    })
}

fn integrate(
    shell: Shell,
    argv: &[OsString],
    res: &Resources,
    env: &mut Env,
    ctx: &Context,
) -> Option<Vec<OsString>> {
    let integ = res.ghostty().join("shell-integration");
    match shell {
        Shell::Zsh => {
            let dir = integ.join("zsh");
            if !res.zsh {
                return None;
            }
            if let Some(old) = get(env, "ZDOTDIR").map(OsStr::to_os_string) {
                set(env, "GHOSTTY_ZSH_ZDOTDIR", old);
            }
            set(env, "ZDOTDIR", dir);
            None
        }
        Shell::Elvish | Shell::Fish => {
            setup_xdg_data_dirs(&integ, res.integration, env);
            None
        }
        Shell::Nushell => {
            if !setup_xdg_data_dirs(&integ, res.integration, env) {
                return None;
            }
            let mut out = vec![argv[0].clone(), "--execute".into(), "use ghostty *".into()];
            let mut rest = argv[1..].iter();
            while let Some(arg) = rest.next() {
                let b = arg.as_bytes();
                if b == b"--command" || b == b"--lsp"
                    || (b.len() > 1 && b[0] == b'-' && b[1] != b'-' && b.contains(&b'c')) {
                    return None;
                } else if b == b"-" || b == b"--" {
                    out.push(arg.clone());
                    out.extend(rest.cloned());
                    break;
                }
                out.push(arg.clone());
            }
            Some(out)
        }
        Shell::Bash => setup_bash(argv, &integ, res.bash, env, ctx),
    }
}

fn setup_xdg_data_dirs(integ: &Path, present: bool, env: &mut Env) -> bool {
    if !present {
        return false;
    }
    set(env, "GHOSTTY_SHELL_INTEGRATION_XDG_DIR", integ);
    let dirs = prepend_env(
        get(env, "XDG_DATA_DIRS").unwrap_or(OsStr::new("/usr/local/share:/usr/share")),
        integ.as_os_str(),
    );
    set(env, "XDG_DATA_DIRS", dirs);
    true
}

fn setup_bash(
    argv: &[OsString],
    integ: &Path,
    present: bool,
    env: &mut Env,
    ctx: &Context,
) -> Option<Vec<OsString>> {
    let mut out = vec![argv[0].clone(), "--posix".into()];
    let mut inject = String::from("1");
    let mut rcfile = None;
    let mut rest = argv[1..].iter();
    while let Some(arg) = rest.next() {
        let b = arg.as_bytes();
        if b == b"--posix" {
            return None;
        } else if b == b"--norc" {
            inject.push_str(" --norc");
        } else if b == b"--noprofile" {
            inject.push_str(" --noprofile");
        } else if b == b"--rcfile" || b == b"--init-file" {
            rcfile = rest.next().cloned();
        } else if b.len() > 1 && b[0] == b'-' && b[1] != b'-' {
            if b.contains(&b'c') {
                return None;
            }
            out.push(arg.clone());
        } else if b == b"-" || b == b"--" {
            out.push(arg.clone());
            out.extend(rest.cloned());
            break;
        } else {
            out.push(arg.clone());
        }
    }
    let script = integ.join("bash/ghostty.bash");
    if !present {
        return None;
    }
    if let Some(old) = get(env, "ENV").map(OsStr::to_os_string) {
        set(env, "GHOSTTY_BASH_ENV", old);
    }
    set(env, "ENV", script);
    set(env, "GHOSTTY_BASH_INJECT", inject);
    if let Some(rcfile) = rcfile {
        set(env, "GHOSTTY_BASH_RCFILE", rcfile);
    }
    if get(env, "HISTFILE").is_none() {
        let home = get(env, "HOME")
            .filter(|h| !h.is_empty())
            .map(PathBuf::from)
            .or_else(|| ctx.user.as_ref().and_then(|u| u.home.clone()));
        if let Some(home) = home {
            set(env, "HISTFILE", home.join(".bash_history"));
            set(env, "GHOSTTY_BASH_UNEXPORT_HISTFILE", "1");
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resources(name: &str) -> (PathBuf, Resources) {
        let root = std::env::temp_dir()
            .join("kanna-keeper-unit")
            .join(format!("{}-{name}", std::process::id()));
        let share = root.join("share");
        for dir in [
            "terminfo/78",
            "ghostty/shell-integration/zsh",
            "ghostty/shell-integration/bash",
            "ghostty/shell-integration/fish",
        ] {
            std::fs::create_dir_all(share.join(dir)).unwrap();
        }
        std::fs::write(share.join("terminfo/78/xterm-ghostty"), "").unwrap();
        std::fs::write(
            share.join("ghostty/shell-integration/bash/ghostty.bash"),
            "",
        )
        .unwrap();
        std::fs::write(share.join("ghostty-version"), "1.3.2-dev+4ec3122\n").unwrap();
        (root, Resources::at(share))
    }

    fn ctx(res: Option<Resources>, macos: bool) -> Context {
        Context {
            resources: res,
            exe_dir: Some("/opt/kanna/bin".into()),
            macos,
            user: Some(User {
                name: "u".into(),
                home: Some("/nonexistent-home".into()),
                shell: "/bin/zsh".into(),
            }),
        }
    }

    fn base() -> Env {
        let mut env = Env::new();
        set(&mut env, "PATH", "/usr/bin:/bin");
        set(&mut env, "VTE_VERSION", "7000");
        set(&mut env, "GHOSTTY_SURFACE_ID", "0x1");
        env
    }

    fn val<'a>(plan: &'a Plan, key: &str) -> Option<&'a str> {
        get(&plan.env, key).and_then(OsStr::to_str)
    }

    fn args(plan: &Plan) -> Vec<&str> {
        plan.argv.iter().map(|a| a.to_str().unwrap()).collect()
    }

    #[test]
    fn ghostty_env_with_resources() {
        let (_root, res) = resources("env");
        let share = res.share.clone();
        let p = plan(
            &["zsh".into(), "-l".into()],
            false,
            None,
            base(),
            &ctx(Some(res), false),
        );
        assert_eq!(val(&p, "TERM"), Some("xterm-ghostty"));
        assert_eq!(val(&p, "COLORTERM"), Some("truecolor"));
        assert_eq!(val(&p, "TERMINFO"), share.join("terminfo").to_str());
        assert_eq!(
            val(&p, "GHOSTTY_RESOURCES_DIR"),
            share.join("ghostty").to_str()
        );
        assert_eq!(val(&p, "TERM_PROGRAM"), Some("ghostty"));
        assert_eq!(val(&p, "TERM_PROGRAM_VERSION"), Some("1.3.2-dev+4ec3122"));
        assert_eq!(
            val(&p, "GHOSTTY_SHELL_FEATURES"),
            Some("cursor:blink,path,title")
        );
        assert_eq!(val(&p, "PATH"), Some("/usr/bin:/bin:/opt/kanna/bin"));
        assert_eq!(val(&p, "VTE_VERSION"), None);
        assert_eq!(val(&p, "GHOSTTY_SURFACE_ID"), None);
        assert_eq!(
            val(&p, "ZDOTDIR"),
            share.join("ghostty/shell-integration/zsh").to_str()
        );
        assert_eq!(args(&p), ["zsh", "-l"]);
        assert_eq!(val(&p, "XDG_DATA_DIRS"), None);
    }

    #[test]
    fn fallback_without_resources() {
        let p = plan(&["zsh".into()], false, None, base(), &ctx(None, false));
        assert_eq!(val(&p, "TERM"), Some("xterm-256color"));
        assert_eq!(val(&p, "COLORTERM"), Some("truecolor"));
        assert_eq!(val(&p, "TERMINFO"), None);
        assert_eq!(val(&p, "ZDOTDIR"), None);
        assert_eq!(val(&p, "TERM_PROGRAM"), Some("ghostty"));
    }

    #[test]
    fn request_env_overrides_and_zdotdir_is_preserved() {
        let (_root, res) = resources("override");
        let mut req = Map::new();
        req.insert("TERM".into(), "dumb".into());
        req.insert("ZDOTDIR".into(), "/my/zdot".into());
        let p = plan(
            &["/bin/zsh".into()],
            false,
            Some(&req),
            base(),
            &ctx(Some(res), false),
        );
        assert_eq!(val(&p, "TERM"), Some("dumb"));
        assert_eq!(val(&p, "GHOSTTY_ZSH_ZDOTDIR"), Some("/my/zdot"));
        assert!(val(&p, "ZDOTDIR")
            .unwrap()
            .ends_with("shell-integration/zsh"));
    }

    #[test]
    fn macos_never_wraps_in_login() {
        let (_root, res) = resources("login");
        let share = res.share.clone();
        let c = ctx(Some(res), true);
        let p = plan(&["/bin/zsh".into(), "-l".into()], false, None, base(), &c);
        assert_eq!(args(&p), ["/bin/zsh", "-l"]);
        assert_eq!(
            val(&p, "XDG_DATA_DIRS").map(String::from),
            Some(format!("/usr/local/share:/usr/share:{}", share.display()))
        );
        assert_eq!(
            val(&p, "MANPATH").map(String::from),
            Some(format!(":{}", share.join("man").display()))
        );
        let p = plan(&["sh".into(), "-c".into(), "exit 3".into()], false, None, base(), &c);
        assert_eq!(args(&p), ["sh", "-c", "exit 3"]);
    }

    #[test]
    fn null_removes_a_variable() {
        let mut req = Map::new();
        req.insert("VTE_VERSION".into(), Value::Null);
        req.insert("PATH".into(), Value::Null);
        let p = plan(&["sh".into()], false, Some(&req), base(), &ctx(None, false));
        assert_eq!(val(&p, "PATH"), None);
    }

    #[test]
    fn bash_integration_matches_ghostty() {
        let (_root, res) = resources("bash");
        let share = res.share.clone();
        let mut env = base();
        set(&mut env, "ENV", "/old/env.sh");
        set(&mut env, "HOME", "/h");
        let argv = ["bash", "--norc", "--rcfile", "rc.sh", "-i"].map(String::from);
        let p = plan(&argv, false, None, env, &ctx(Some(res.clone()), false));
        assert_eq!(args(&p), ["bash", "--posix", "-i"]);
        assert_eq!(val(&p, "GHOSTTY_BASH_INJECT"), Some("1 --norc"));
        assert_eq!(val(&p, "GHOSTTY_BASH_RCFILE"), Some("rc.sh"));
        assert_eq!(val(&p, "GHOSTTY_BASH_ENV"), Some("/old/env.sh"));
        assert_eq!(
            val(&p, "ENV"),
            share
                .join("ghostty/shell-integration/bash/ghostty.bash")
                .to_str()
        );
        assert_eq!(val(&p, "HISTFILE"), Some("/h/.bash_history"));
        let p = plan(
            &["/bin/bash".into()],
            true,
            None,
            base(),
            &ctx(Some(res.clone()), true),
        );
        assert_eq!(val(&p, "ENV"), None);
        let mut path = Map::new();
        path.insert("PATH".into(), "/bin".into());
        let argv = ["bash".into(), "--norc".into()];
        let p = plan(
            &argv,
            true,
            Some(&path),
            base(),
            &ctx(Some(res.clone()), true),
        );
        assert_eq!(val(&p, "ENV"), None, "bash resolving to /bin/bash");
        let p = plan(&argv, true, None, base(), &ctx(Some(res.clone()), false));
        assert_eq!(args(&p), ["bash", "--posix"]);
        let p = plan(
            &["bash".into(), "-c".into(), "x".into()],
            false,
            None,
            base(),
            &ctx(Some(res), false),
        );
        assert_eq!(args(&p), ["bash", "-c", "x"]);
        assert_eq!(val(&p, "ENV"), None);
    }

    #[test]
    fn fish_uses_xdg_data_dirs() {
        let (_root, res) = resources("fish");
        let integ = res.ghostty().join("shell-integration");
        let p = plan(&["fish".into()], false, None, base(), &ctx(Some(res), false));
        assert_eq!(val(&p, "GHOSTTY_SHELL_INTEGRATION_XDG_DIR"), integ.to_str());
        assert_eq!(
            val(&p, "XDG_DATA_DIRS").map(String::from),
            Some(format!("{}:/usr/local/share:/usr/share", integ.display()))
        );
    }

    #[test]
    fn candidates_are_absolute_path_entries_or_the_path_given() {
        let path = OsStr::new("relative:/usr/bin::/bin:.");
        assert_eq!(candidates(OsStr::new("zsh"), path), [Path::new("/usr/bin/zsh"), Path::new("/bin/zsh")]);
        assert_eq!(candidates(OsStr::new("./zsh"), path), [Path::new("./zsh")]);
        assert!(candidates(OsStr::new("zsh"), OsStr::new("a:b")).is_empty());
    }
}
