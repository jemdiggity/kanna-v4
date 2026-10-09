//! kanna: drives a mux from the command line (docs/kanna-spec.md).

use kmux_client::{exit, fail, Args, Failure, Kmux};
use serde_json::{json, Map, Value};
use std::process::ExitCode;

const USAGE: &str = "usage: kanna [--mux kmux] [--json] <command>

commands:
  open term [--cmd CMD] [--cwd DIR]   a terminal
  open web URL [--history]            a web page
  open md FILE                        a markdown file, rendered (with mermaid diagrams)
  open ios --app APP [--device NAME]  an iOS app in the simulator
     open options: [--name NAME] [--split right|down] [--size FRACTION] [--tab] [--window ID|new] [--no-wait]
  list
  focus PANE
  navigate PANE URL|FILE|--back|--forward
  send PANE TEXT
  restart PANE
  close PANE
  mux";

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
    let mut args = Args::new(args);
    let json_output = args.flag("--json");
    if let Some(mux) = args.option("--mux")? {
        if mux != "kmux" {
            return Err(fail(exit::UNSUPPORTED, format!("the {mux} backend isn't supported yet; use --mux kmux")));
        }
    } else if let Ok(mux) = std::env::var("KANNA_MUX") {
        if !mux.is_empty() && mux != "kmux" {
            return Err(fail(exit::UNSUPPORTED, format!("the {mux} backend (from KANNA_MUX) isn't supported yet")));
        }
    }
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
            let mut mux = Kmux::connect()?;
            let capabilities = mux.call("capabilities", json!({}))?;
            let supported = capabilities["paneTypes"].as_array().is_some_and(|types| types.contains(&json!(kind)));
            if !supported {
                return Err(fail(exit::UNSUPPORTED, format!("kmux can't show {kind} panes yet")));
            }
            let reply = mux.call("open", Value::Object(request))?;
            let pane = &reply["pane"];
            let label = pane["name"].as_str().or(pane["id"].as_str()).unwrap_or_default().to_string();
            print(&reply, label);
        }
        "navigate" => {
            let request = navigate_request(&mut args)?;
            let reply = Kmux::connect()?.call("navigate", Value::Object(request))?;
            print(&reply, String::new());
        }
        "focus" | "restart" | "close" => {
            let pane = args.positional().ok_or_else(|| fail(exit::USAGE, format!("{command} needs a pane")))?;
            finish(&args)?;
            let reply = Kmux::connect()?.call(&command, json!({ "pane": pane }))?;
            print(&reply, String::new());
        }
        "send" => {
            let pane = args.positional().ok_or_else(|| fail(exit::USAGE, "send needs a pane and text"))?;
            let text = args.positional().ok_or_else(|| fail(exit::USAGE, "send needs text"))?;
            finish(&args)?;
            let reply = Kmux::connect()?.call("send", json!({ "pane": pane, "text": text }))?;
            print(&reply, String::new());
        }
        "list" => {
            finish(&args)?;
            let reply = Kmux::connect()?.call("list", json!({}))?;
            print(&reply, format_list(&reply));
        }
        "mux" => {
            finish(&args)?;
            let reply = Kmux::connect()?.call("capabilities", json!({}))?;
            let list = |key: &str| {
                reply[key].as_array().map(|v| v.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).unwrap_or_default()
            };
            print(&reply, format!("mux: kmux\npane types: {}\nfeatures: {}", list("paneTypes"), list("features")));
        }
        "help" | "--help" | "-h" => println!("{USAGE}"),
        other => return Err(fail(exit::USAGE, format!("unknown command \"{other}\"\n{USAGE}"))),
    }
    Ok(())
}

/// `open TYPE …` as a kmux `open` request.
fn open_request(args: &mut Args) -> Result<Map<String, Value>, Failure> {
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
    let kind = args.positional().ok_or_else(|| fail(exit::USAGE, "open needs a pane type: term, web, md or ios"))?;
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
        _ => {}
    }
    if let Some(cwd) = request.get("cwd").and_then(Value::as_str).map(absolute) {
        request.insert("cwd".into(), json!(cwd));
    }
    request.insert("type".into(), json!(kind));
    finish(args)?;
    Ok(request)
}

/// `navigate PANE URL|FILE|--back|--forward`: a `.md` target is a file for a markdown pane.
fn navigate_request(args: &mut Args) -> Result<Map<String, Value>, Failure> {
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

fn finish(args: &Args) -> Result<(), Failure> {
    match args.first() {
        None => Ok(()),
        Some(extra) => Err(fail(exit::USAGE, format!("unexpected argument \"{extra}\""))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Args {
        Args::new(list.iter().map(|s| s.to_string()).collect())
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
    fn navigate_tells_files_from_urls() {
        assert_eq!(navigate_request(&mut args(&["spec", "/tmp/b.md"])).unwrap()["path"], "/tmp/b.md");
        assert_eq!(navigate_request(&mut args(&["site", "https://x.dev/a.md"])).unwrap()["url"], "https://x.dev/a.md");
        assert_eq!(navigate_request(&mut args(&["site", "--back"])).unwrap()["back"], true);
    }
}
