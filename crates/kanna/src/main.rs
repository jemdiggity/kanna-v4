//! kanna: drives a mux from the command line (docs/kanna-spec.md).

use kmux_client::{exit, fail, Args, Failure, Kmux};
use serde_json::{json, Map, Value};
use std::process::ExitCode;

const USAGE: &str = "usage: kanna [--mux kmux] [--json] <command>

commands:
  open term [--cmd CMD] [--cwd DIR] [--name NAME] [--split right|down] [--size FRACTION] [--tab] [--window ID|new] [--no-wait]
  list
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
            let mut request = Map::new();
            for (flag, key) in [("--cmd", "cmd"), ("--cwd", "cwd"), ("--name", "name"), ("--split", "split"), ("--size", "size"), ("--window", "window")] {
                if let Some(value) = args.option(flag)? {
                    request.insert(key.into(), json!(value));
                }
            }
            if args.flag("--tab") {
                request.insert("tab".into(), json!(true));
            }
            if args.flag("--no-wait") {
                request.insert("wait".into(), json!(false));
            }
            let kind = args.positional().ok_or_else(|| fail(exit::USAGE, "open needs a pane type: term, web or ios"))?;
            request.insert("type".into(), json!(kind));
            finish(&args)?;
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
        "list" => {
            finish(&args)?;
            let reply = Kmux::connect()?.call("list", json!({}))?;
            print(&reply, format_list(&reply));
        }
        "close" => {
            let pane = args.positional().ok_or_else(|| fail(exit::USAGE, "close needs a pane"))?;
            finish(&args)?;
            let reply = Kmux::connect()?.call("close", json!({ "pane": pane }))?;
            print(&reply, String::new());
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
