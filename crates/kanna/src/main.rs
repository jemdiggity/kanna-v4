use kmux_client::{exit, fail, Args, Brand, Error, Failure, Kmux, Target};
use std::collections::VecDeque;
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

const KANNA: Brand = Brand { id: "kanna", name: "Kanna", bundle_id: "dev.kanna.kanna" };

const KANNA_APP_BUNDLE: &str = "dev.kanna.app.dev";

const NO_APP: u8 = 6;

const APP_START: Duration = Duration::from_secs(20);

#[derive(Clone, Copy, Debug, PartialEq)]
enum Backend {
    Kanna,
    KannaDev,
    Kmux,
}

impl Backend {
    fn brand(self) -> Brand {
        match self {
            Backend::Kmux => Brand::KMUX,
            Backend::Kanna | Backend::KannaDev => KANNA,
        }
    }
}

const USAGE: &str = "usage: kanna [--mux kanna|kanna-dev|kmux] [--json] <command>

commands:
  open [term] [--cmd CMD] [--cwd DIR] a terminal
  open web URL [--history]            a web page
  open md FILE                        a markdown file, rendered (with mermaid diagrams)
  open ios --app APP [--device NAME]  an iOS app in the simulator
     open options: [--name NAME] [--split right|down] [--size FRACTION] [--tab] [--window ID|new] [--no-wait]
  list
  focus PANE
  navigate PANE URL|FILE|--back|--forward
  send PANE TEXT
  read PANE [--lines N]               a terminal's screen as text (the last N lines, with scrollback)
  restart PANE
  close PANE
  mux

--mux kanna (the default) is Kanna.app, Kanna's own copy of kmux; --mux kmux is a kmux you run.
--mux kanna-dev is Kanna's daemon (kanna-dev/kanna.sock in the app support folder). When it needs
the app for a command (a split, focus, arrange…) and none is attached, kanna starts $KANNA_APP (or
the app with bundle ID dev.kanna.app.dev) and waits for it; never when KANNA_SOCKET is set (exit 6).";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            eprintln!("kanna: {}", failure.message);
            ExitCode::from(failure.code)
        }
    }
}

fn run(args: Vec<String>) -> Result<(), Failure> {
    let mut args = Argv::new(args);
    let json_output = args.flag("--json");
    let backend = match args.option("--mux")? {
        Some(mux) => backend(&mux, "")?,
        None => match std::env::var("KANNA_MUX") {
            Ok(mux) if !mux.is_empty() => backend(&mux, " (from KANNA_MUX)")?,
            _ => Backend::Kanna,
        },
    };
    let brand = backend.brand();
    let connect = || Session::connect(backend);
    let command = args.positional().ok_or_else(|| fail(exit::USAGE, USAGE))?;
    let print = |reply: &Value, human: String| {
        if json_output {
            println!("{reply}");
        } else if !human.is_empty() {
            println!("{human}");
        }
    };

    match command.as_str() {
        "open" => {
            let request = open_request(&mut args)?;
            let kind = request["type"].as_str().unwrap_or_default().to_string();
            let mut mux = connect()?;
            let capabilities = mux.call("capabilities", json!({}))?;
            let supported = capabilities["paneTypes"].as_array().is_some_and(|types| types.contains(&json!(kind)));
            if !supported {
                return Err(fail(exit::UNSUPPORTED, format!("{} can't show {kind} panes yet", brand.name)));
            }
            let reply = mux.call("open", Value::Object(request))?;
            let pane = &reply["pane"];
            let label = pane["name"].as_str().or(pane["id"].as_str()).unwrap_or_default().to_string();
            print(&reply, label);
        }
        "navigate" => {
            let request = navigate_request(&mut args)?;
            let reply = connect()?.call("navigate", Value::Object(request))?;
            print(&reply, String::new());
        }
        "focus" | "restart" | "close" => {
            let pane = args.positional().ok_or_else(|| fail(exit::USAGE, format!("{command} needs a pane")))?;
            finish(&args)?;
            let reply = connect()?.call(&command, json!({ "pane": pane }))?;
            print(&reply, String::new());
        }
        "send" => {
            let pane = args.positional().ok_or_else(|| fail(exit::USAGE, "send needs a pane and text"))?;
            let text = args.positional().ok_or_else(|| fail(exit::USAGE, "send needs text"))?;
            finish(&args)?;
            let reply = connect()?.call("send", json!({ "pane": pane, "text": text }))?;
            print(&reply, String::new());
        }
        "read" => {
            let lines = match args.option("--lines")? {
                Some(n) => Some(n.parse::<u64>().ok().filter(|n| *n > 0).ok_or_else(|| fail(exit::USAGE, "--lines needs a number above 0"))?),
                None => None,
            };
            let pane = args.positional().ok_or_else(|| fail(exit::USAGE, "read needs a pane"))?;
            finish(&args)?;
            let mut request = json!({ "pane": pane });
            if let Some(lines) = lines {
                request["lines"] = json!(lines);
            }
            let reply = connect()?.call("read", request)?;
            print(&reply, reply["text"].as_str().unwrap_or_default().to_string());
        }
        "list" => {
            finish(&args)?;
            let reply = connect()?.call("list", json!({}))?;
            print(&reply, format_list(&reply));
        }
        "mux" => {
            finish(&args)?;
            let reply = connect()?.call("capabilities", json!({}))?;
            let list = |key: &str| {
                reply[key].as_array().map(|v| v.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).unwrap_or_default()
            };
            print(&reply, format!("mux: {}\npane types: {}\nfeatures: {}", brand.name, list("paneTypes"), list("features")));
        }
        "help" | "--help" | "-h" => println!("{USAGE}"),
        other => return Err(fail(exit::USAGE, format!("unknown command \"{other}\"\n{USAGE}"))),
    }
    Ok(())
}

struct Session {
    mux: Kmux,
    target: Target,
    pinned: Option<String>,
}

impl Session {
    fn connect(backend: Backend) -> Result<Session, Failure> {
        let brand = backend.brand();
        let mut target = Target::from_env_for(brand);
        let pinned = pinned_socket(brand, |name| std::env::var(name).ok());
        if backend == Backend::KannaDev && pinned.is_none() {
            target.socket = daemon_socket(|name| std::env::var_os(name));
        }
        let mux = match (&pinned, backend) {
            (Some(variable), _) => Kmux::try_connect(&target).ok_or_else(|| {
                fail(exit::UNREACHABLE, format!("nothing answers on {} ({variable} is set, so kanna doesn't start an app for it)", target.socket.display()))
            })?,
            (None, Backend::KannaDev) => Kmux::try_connect(&target)
                .ok_or_else(|| fail(exit::UNREACHABLE, format!("Kanna's daemon isn't running on {} (start kanna-daemon)", target.socket.display())))?,
            (None, _) => Kmux::connect_to(&target).map_err(Failure::from)?,
        };
        Ok(Session { mux, target, pinned })
    }

    fn call(&mut self, cmd: &str, args: Value) -> Result<Value, Failure> {
        let mut started: Option<Instant> = None;
        loop {
            match self.mux.call(cmd, args.clone()) {
                Err(Error::Mux { code, message }) if code == "no_app" => {
                    if let Some(variable) = &self.pinned {
                        return Err(fail(NO_APP, format!("{message} [no_app] ({variable} is set, so kanna doesn't start Kanna.app for it)")));
                    }
                    match started {
                        None => {
                            launch_app(&self.target, |name| std::env::var(name).ok().filter(|v| !v.is_empty()))?;
                            started = Some(Instant::now());
                        }
                        Some(at) if at.elapsed() > APP_START => {
                            return Err(fail(NO_APP, format!("Kanna.app was started but didn't attach to the daemon on {} [no_app]", self.target.socket.display())));
                        }
                        Some(_) => {}
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
                Err(Error::Mux { code, message }) if code == "unsupported" => return Err(fail(exit::UNSUPPORTED, format!("{message} [{code}]"))),
                other => return other.map_err(Failure::from),
            }
        }
    }
}

fn daemon_socket(env: impl Fn(&str) -> Option<std::ffi::OsString>) -> PathBuf {
    let home = PathBuf::from(env("HOME").unwrap_or_default());
    let support = if cfg!(target_os = "macos") {
        home.join("Library/Application Support")
    } else {
        env("XDG_DATA_HOME").filter(|d| !d.is_empty()).map(PathBuf::from).unwrap_or_else(|| home.join(".local/share"))
    };
    support.join("kanna-dev/kanna.sock")
}

fn launch_command(target: &Target, env: impl Fn(&str) -> Option<String>) -> Command {
    let mut open = Command::new("/usr/bin/open");
    open.arg("-n");
    if target.background {
        open.arg("-g");
    }
    open.arg("--env").arg(format!("KANNA_SOCKET={}", target.socket.display()));
    if let Some(folder) = env("KANNA_STATE_DIR") {
        open.arg("--env").arg(format!("KANNA_STATE_DIR={folder}"));
    }
    match env("KANNA_APP") {
        Some(app) => open.arg("-a").arg(app),
        None => open.arg("-b").arg(KANNA_APP_BUNDLE),
    };
    if target.background {
        open.args(["--args", "--bg"]);
    }
    open
}

fn launch_app(target: &Target, env: impl Fn(&str) -> Option<String>) -> Result<(), Failure> {
    let status = launch_command(target, env).status().map_err(|e| fail(NO_APP, format!("could not start Kanna.app: {e}")))?;
    if status.success() {
        return Ok(());
    }
    Err(fail(NO_APP, format!("this needs Kanna.app, which isn't running and couldn't be started (set KANNA_APP to it, or install {KANNA_APP_BUNDLE})")))
}

fn pinned_socket(brand: Brand, env: impl Fn(&str) -> Option<String>) -> Option<String> {
    let variable = brand.variable("SOCKET");
    env(&variable).is_some_and(|value| !value.is_empty()).then_some(variable)
}

struct Argv {
    options: Args,
    rest: VecDeque<String>,
}

impl Argv {
    fn new(mut words: Vec<String>) -> Argv {
        let rest = match words.iter().position(|word| word == "--") {
            Some(i) => words.split_off(i).into_iter().skip(1).collect(),
            None => VecDeque::new(),
        };
        Argv { options: Args::new(words), rest }
    }

    fn flag(&mut self, name: &str) -> bool {
        self.options.flag(name)
    }

    fn option(&mut self, name: &str) -> Result<Option<String>, Failure> {
        self.options.option(name)
    }

    fn positional(&mut self) -> Option<String> {
        self.options.positional().or_else(|| self.rest.pop_front())
    }

    fn first(&self) -> Option<&str> {
        self.options.first().or(self.rest.front().map(String::as_str))
    }
}

fn backend(mux: &str, from: &str) -> Result<Backend, Failure> {
    match mux {
        "kanna" => Ok(Backend::Kanna),
        "kanna-dev" => Ok(Backend::KannaDev),
        "kmux" => Ok(Backend::Kmux),
        other => Err(fail(exit::UNSUPPORTED, format!("the {other} backend{from} isn't supported yet; use kanna, kanna-dev or kmux"))),
    }
}

fn open_request(args: &mut Argv) -> Result<Map<String, Value>, Failure> {
    let mut request = Map::new();
    for (flag, key) in [
        ("--cmd", "cmd"), ("--cwd", "cwd"), ("--name", "name"), ("--split", "split"), ("--size", "size"),
        ("--window", "window"), ("--app", "app"), ("--device", "device"),
    ] {
        if let Some(value) = args.option(flag)? {
            request.insert(key.into(), json!(value));
        }
    }
    for (flag, key, value) in [("--tab", "tab", true), ("--history", "history", true), ("--no-wait", "wait", false)] {
        if args.flag(flag) {
            request.insert(key.into(), json!(value));
        }
    }
    let kind = args.positional().unwrap_or_else(|| "term".into());
    match kind.as_str() {
        "web" => {
            let url = args.positional().ok_or_else(|| fail(exit::USAGE, "open web needs a URL"))?;
            request.insert("url".into(), json!(url));
        }
        "md" => {
            let file = args.positional().ok_or_else(|| fail(exit::USAGE, "open md needs a file"))?;
            request.insert("path".into(), json!(absolute(&file)));
        }
        "ios" if !request.contains_key("app") => return Err(fail(exit::USAGE, "open ios needs --app APP (a .app or a bundle ID)")),
        "term" | "ios" => {}
        other => return Err(fail(exit::USAGE, format!("unknown pane type \"{other}\" (term, web, md or ios)"))),
    }
    if let Some(cwd) = request.get("cwd").and_then(Value::as_str).map(absolute) {
        request.insert("cwd".into(), json!(cwd));
    }
    request.insert("type".into(), json!(kind));
    finish(args)?;
    Ok(request)
}

fn navigate_request(args: &mut Argv) -> Result<Map<String, Value>, Failure> {
    let back = args.flag("--back");
    let forward = args.flag("--forward");
    let pane = args.positional().ok_or_else(|| fail(exit::USAGE, "navigate needs a pane"))?;
    let mut request = Map::new();
    request.insert("pane".into(), json!(pane));
    if back || forward {
        request.insert(if back { "back" } else { "forward" }.into(), json!(true));
    } else {
        let target = args.positional().ok_or_else(|| fail(exit::USAGE, "navigate needs a URL, a file, --back or --forward"))?;
        if is_markdown(&target) {
            request.insert("path".into(), json!(absolute(&target)));
        } else {
            request.insert("url".into(), json!(target));
        }
    }
    finish(args)?;
    Ok(request)
}

fn is_markdown(target: &str) -> bool {
    !target.contains("://") && [".md", ".markdown", ".mdown"].iter().any(|ext| target.to_lowercase().ends_with(ext))
}

fn absolute(path: &str) -> String {
    let expanded = match path.strip_prefix("~/") {
        Some(rest) => std::env::var("HOME").map(|home| format!("{home}/{rest}")).unwrap_or_else(|_| path.to_string()),
        None => path.to_string(),
    };
    let p = std::path::Path::new(&expanded);
    if p.is_absolute() {
        return expanded;
    }
    std::env::current_dir().map(|dir| dir.join(p).to_string_lossy().into_owned()).unwrap_or(expanded)
}

fn format_list(reply: &Value) -> String {
    let empty = vec![];
    let panes = reply["panes"].as_array().unwrap_or(&empty);
    let mut rows = vec![["ID".to_string(), "NAME".into(), "TYPE".into(), "STATE".into(), "WINDOW".into()]];
    for pane in panes {
        let id = pane["id"].as_str().unwrap_or_default();
        let window = reply["windows"]
            .as_array()
            .unwrap_or(&empty)
            .iter()
            .find(|w| w["tabs"].as_array().unwrap_or(&empty).iter().any(|t| layout_has(&t["layout"], pane)))
            .and_then(|w| w["id"].as_str())
            .unwrap_or("-");
        let state = match pane["exitCode"].as_i64() {
            Some(code) => format!("{} ({code})", pane["state"].as_str().unwrap_or_default()),
            None => pane["state"].as_str().unwrap_or_default().to_string(),
        };
        rows.push([id.into(), pane["name"].as_str().unwrap_or("-").into(), pane["type"].as_str().unwrap_or_default().into(), state, window.into()]);
    }
    let widths: Vec<usize> = (0..5).map(|i| rows.iter().map(|r| r[i].len()).max().unwrap_or(0)).collect();
    rows.iter()
        .map(|row| row.iter().zip(&widths).map(|(cell, w)| format!("{cell:w$}")).collect::<Vec<_>>().join("  ").trim_end().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn layout_has(node: &Value, pane: &Value) -> bool {
    if let Some(name) = node["pane"].as_str() {
        return Some(name) == pane["name"].as_str().or(pane["id"].as_str());
    }
    node["children"].as_array().is_some_and(|kids| kids.iter().any(|kid| layout_has(kid, pane)))
}

fn finish(args: &Argv) -> Result<(), Failure> {
    match args.first() {
        None => Ok(()),
        Some(extra) => Err(fail(exit::USAGE, format!("unexpected argument \"{extra}\""))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Argv {
        Argv::new(list.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn the_backends_own_socket_variable_pins_it() {
        let env = |set: &'static str| move |name: &str| (name == set).then(|| "/tmp/x.sock".to_string());
        assert_eq!(pinned_socket(KANNA, env("KANNA_SOCKET")).as_deref(), Some("KANNA_SOCKET"));
        assert_eq!(pinned_socket(KANNA, env("KMUX_SOCKET")), None);
        assert_eq!(pinned_socket(Brand::KMUX, env("KMUX_SOCKET")).as_deref(), Some("KMUX_SOCKET"));
        assert_eq!(pinned_socket(Brand::KMUX, env("KANNA_SOCKET")), None);
        assert_eq!(pinned_socket(KANNA, |_| Some(String::new())), None);
    }

    #[test]
    fn the_dev_daemon_lives_in_the_app_support_folder() {
        let home = |name: &str| (name == "HOME").then(|| std::ffi::OsString::from("/Users/x"));
        let expected = if cfg!(target_os = "macos") { "/Users/x/Library/Application Support/kanna-dev/kanna.sock" } else { "/Users/x/.local/share/kanna-dev/kanna.sock" };
        assert_eq!(daemon_socket(home), PathBuf::from(expected));
    }

    #[test]
    fn the_app_started_for_the_daemon_is_the_new_app_never_the_branded_kmux() {
        let target = Target { brand: KANNA, instance: "default".into(), socket: "/tmp/d/kanna.sock".into(), background: true };
        let args = |env: &dyn Fn(&str) -> Option<String>| -> Vec<String> {
            launch_command(&target, env).get_args().map(|a| a.to_string_lossy().into_owned()).collect()
        };
        let none = args(&|_| None);
        assert_eq!(none, ["-n", "-g", "--env", "KANNA_SOCKET=/tmp/d/kanna.sock", "-b", "dev.kanna.app.dev", "--args", "--bg"]);
        assert!(!none.iter().any(|a| a == KANNA.bundle_id));
        let app = args(&|name| match name {
            "KANNA_APP" => Some("/x/Kanna.app".into()),
            "KANNA_STATE_DIR" => Some("/tmp/d".into()),
            _ => None,
        });
        assert_eq!(app, ["-n", "-g", "--env", "KANNA_SOCKET=/tmp/d/kanna.sock", "--env", "KANNA_STATE_DIR=/tmp/d", "-a", "/x/Kanna.app", "--args", "--bg"]);
    }

    #[test]
    fn a_double_dash_ends_options() {
        let mut argv = args(&["--json", "send", "p1", "--", "--json", "--mux"]);
        assert!(argv.flag("--json"));
        assert!(!argv.flag("--json"));
        assert_eq!(argv.option("--mux").unwrap(), None);
        assert_eq!(argv.positional().as_deref(), Some("send"));
        assert_eq!(argv.positional().as_deref(), Some("p1"));
        assert_eq!(argv.first(), Some("--json"));
        assert_eq!(argv.positional().as_deref(), Some("--json"));
        assert_eq!(argv.positional().as_deref(), Some("--mux"));
        assert_eq!(argv.positional(), None);
    }

    #[test]
    fn open_refuses_unknown_types() {
        let failure = open_request(&mut args(&["foo"])).unwrap_err();
        assert_eq!(failure.code, exit::USAGE);
    }

    #[test]
    fn open_md_sends_an_absolute_path() {
        let request = open_request(&mut args(&["md", "/tmp/a.md", "--name", "spec"])).unwrap();
        assert_eq!(request["type"], "md");
        assert_eq!(request["path"], "/tmp/a.md");
        assert_eq!(request["name"], "spec");
        let relative = open_request(&mut args(&["md", "docs/a.md"])).unwrap();
        assert!(relative["path"].as_str().unwrap().starts_with('/'));
    }

    #[test]
    fn open_web_and_ios_pass_their_options() {
        let web = open_request(&mut args(&["web", "http://localhost:3000", "--history"])).unwrap();
        assert_eq!(web["url"], "http://localhost:3000");
        assert_eq!(web["history"], true);
        let ios = open_request(&mut args(&["ios", "--app", "com.apple.Preferences", "--device", "iPhone 16"])).unwrap();
        assert_eq!(ios["app"], "com.apple.Preferences");
        assert_eq!(ios["device"], "iPhone 16");
        assert!(open_request(&mut args(&["ios"])).is_err());
        assert!(open_request(&mut args(&["md"])).is_err());
    }

    #[test]
    fn open_is_a_terminal_by_default() {
        let request = open_request(&mut args(&["--name", "s1"])).unwrap();
        assert_eq!(request["type"], "term");
        assert_eq!(request["name"], "s1");
    }

    #[test]
    fn navigate_tells_files_from_urls() {
        assert_eq!(navigate_request(&mut args(&["spec", "/tmp/b.md"])).unwrap()["path"], "/tmp/b.md");
        assert_eq!(navigate_request(&mut args(&["site", "https://x.dev/a.md"])).unwrap()["url"], "https://x.dev/a.md");
        assert_eq!(navigate_request(&mut args(&["site", "--back"])).unwrap()["back"], true);
    }
}
