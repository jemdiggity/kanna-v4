//! kanna: drives a mux from the command line (docs/kanna-spec.md).

use kmux_client::{exit, fail, Args, Brand, Error, Failure, Kmux, Target};
use std::collections::VecDeque;
use serde_json::{json, Map, Value};
use std::process::ExitCode;

/// Kanna's own copy of the kmux app (built from ../kmux by scripts/build-kanna.sh).
const KANNA: Brand = Brand { id: "kanna", name: "Kanna", bundle_id: "dev.kanna.kanna" };

const USAGE: &str = "usage: kanna [--mux kanna|kmux] [--json] <command>

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

--mux kanna (the default) is Kanna.app, Kanna's own copy of kmux; --mux kmux is a kmux you run.";

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
    let brand = match args.option("--mux")? {
        Some(mux) => backend(&mux, "")?,
        None => match std::env::var("KANNA_MUX") {
            Ok(mux) if !mux.is_empty() => backend(&mux, " (from KANNA_MUX)")?,
            _ => KANNA,
        },
    };
    let connect = || connect(brand, pinned_socket(brand, |name| std::env::var(name).ok()));
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
            let capabilities = call(&mut mux, "capabilities", json!({}))?;
            let supported = capabilities["paneTypes"].as_array().is_some_and(|types| types.contains(&json!(kind)));
            if !supported {
                return Err(fail(exit::UNSUPPORTED, format!("{} can't show {kind} panes yet", brand.name)));
            }
            let reply = call(&mut mux, "open", Value::Object(request))?;
            let pane = &reply["pane"];
            let label = pane["name"].as_str().or(pane["id"].as_str()).unwrap_or_default().to_string();
            print(&reply, label);
        }
        "navigate" => {
            let request = navigate_request(&mut args)?;
            let reply = call(&mut connect()?, "navigate", Value::Object(request))?;
            print(&reply, String::new());
        }
        "focus" | "restart" | "close" => {
            let pane = args.positional().ok_or_else(|| fail(exit::USAGE, format!("{command} needs a pane")))?;
            finish(&args)?;
            let reply = call(&mut connect()?, &command, json!({ "pane": pane }))?;
            print(&reply, String::new());
        }
        "send" => {
            let pane = args.positional().ok_or_else(|| fail(exit::USAGE, "send needs a pane and text"))?;
            let text = args.positional().ok_or_else(|| fail(exit::USAGE, "send needs text"))?;
            finish(&args)?;
            let reply = call(&mut connect()?, "send", json!({ "pane": pane, "text": text }))?;
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
            let reply = call(&mut connect()?, "read", request)?;
            print(&reply, reply["text"].as_str().unwrap_or_default().to_string());
        }
        "list" => {
            finish(&args)?;
            let reply = call(&mut connect()?, "list", json!({}))?;
            print(&reply, format_list(&reply));
        }
        "mux" => {
            finish(&args)?;
            let reply = call(&mut connect()?, "capabilities", json!({}))?;
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

fn connect(brand: Brand, pinned: Option<String>) -> Result<Kmux, Failure> {
    let target = Target::from_env_for(brand);
    let Some(variable) = pinned else {
        return Kmux::connect_to(&target).map_err(Failure::from);
    };
    Kmux::try_connect(&target).ok_or_else(|| {
        fail(exit::UNREACHABLE, format!("nothing answers on {} ({variable} is set, so kanna doesn't start an app for it)", target.socket.display()))
    })
}

fn pinned_socket(brand: Brand, env: impl Fn(&str) -> Option<String>) -> Option<String> {
    let variable = brand.variable("SOCKET");
    env(&variable).is_some_and(|value| !value.is_empty()).then_some(variable)
}

fn call(mux: &mut Kmux, cmd: &str, args: Value) -> Result<Value, Failure> {
    mux.call(cmd, args).map_err(|error| match error {
        Error::Mux { code, message } if code == "unsupported" => fail(exit::UNSUPPORTED, format!("{message} [{code}]")),
        error => Failure::from(error),
    })
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

/// The backend `--mux NAME` picks: Kanna.app, or a kmux the user runs.
fn backend(mux: &str, from: &str) -> Result<Brand, Failure> {
    match mux {
        "kanna" => Ok(KANNA),
        "kmux" => Ok(Brand::KMUX),
        other => Err(fail(exit::UNSUPPORTED, format!("the {other} backend{from} isn't supported yet; use kanna or kmux"))),
    }
}

/// `open TYPE …` as a kmux `open` request.
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
    // As kmux: a terminal unless a type is given.
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

/// `navigate PANE URL|FILE|--back|--forward`: a `.md` target is a file for a markdown pane.
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

/// Paths are sent absolute: kmux doesn't share kanna's working directory.
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

/// Layout trees name panes by name when they have one, else by ID.
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
