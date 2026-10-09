//! kmux: the command line for the kmux terminal multiplexer. Each command
//! sends one control-protocol request (docs/kmux-spec.md §7) to the running
//! app over its Unix socket.

mod commands;
mod output;

use commands::{Command, COMMANDS, GROUPS};
use kmux_client::{exit, fail, valid_instance, Args, Failure, Kmux, Target};
use serde_json::{json, Value};
use std::process::ExitCode;

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            eprintln!("kmux: {}", failure.message);
            ExitCode::from(failure.code)
        }
    }
}

fn run(args: Vec<String>) -> Result<(), Failure> {
    let mut args = Args::new(args);
    let json_output = args.flag("--json");
    let wants_help = args.flag("--help") || args.flag("-h");
    let background = args.flag("--bg");
    let mut target = match args.option("--instance")? {
        Some(name) if !valid_instance(&name) => {
            return Err(fail(exit::USAGE, format!("bad instance name \"{name}\": use letters, digits, - and _ (up to 32)")))
        }
        Some(name) => Target::named(&name),
        None => Target::from_env(),
    };
    target.background |= background;
    // If kmux has to be started, start it with a new window, not the saved layout.
    if args.flag("--fresh") {
        std::env::set_var(target.brand.variable("FRESH"), "1");
    }
    let Some(name) = args.positional() else {
        if args.flag("--version") {
            println!("kmux {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        if let Some(extra) = args.first() {
            return Err(fail(exit::USAGE, format!("unknown option \"{extra}\"\n\n{}", overview(&target))));
        }
        println!("{}", overview(&target));
        return Ok(());
    };

    match name.as_str() {
        "help" => {
            match args.positional() {
                None => println!("{}", overview(&target)),
                Some(topic) if topic == "instances" => println!("{INSTANCES_HELP}"),
                Some(topic) => println!("{}", help(lookup(&topic)?, &target)),
            }
            return Ok(());
        }
        "commands" => {
            let supported = supported(&target);
            if json_output {
                println!("{}", serde_json::to_string_pretty(&describe(supported.as_deref())).unwrap());
            } else {
                for command in COMMANDS {
                    println!("{:<13} {}{}", command.name, command.summary, mark(command, supported.as_deref()));
                }
            }
            return Ok(());
        }
        "instances" => {
            if wants_help {
                println!("{INSTANCES_HELP}");
                return Ok(());
            }
            return instances(&target, json_output);
        }
        "raw" => {
            if wants_help {
                println!("usage: kmux raw REQUEST\n\nSends one control-protocol request as JSON and prints the reply, e.g.\n  kmux raw '{{\"cmd\":\"list\"}}'\n  kmux raw '{{\"cmd\":\"open\",\"args\":{{\"type\":\"term\",\"name\":\"a\"}}}}'");
                return Ok(());
            }
            let text = args.positional().ok_or_else(|| fail(exit::USAGE, "raw: missing REQUEST, e.g. kmux raw '{\"cmd\":\"list\"}'"))?;
            let request: Value = serde_json::from_str(&text).map_err(|e| fail(exit::USAGE, format!("raw: REQUEST is not valid JSON: {e}")))?;
            let cmd = request["cmd"].as_str().ok_or_else(|| fail(exit::USAGE, "raw: REQUEST needs a \"cmd\""))?;
            let reply = Kmux::connect_to(&target)?.call(cmd, request.get("args").cloned().unwrap_or(json!({})))?;
            println!("{reply}");
            return Ok(());
        }
        _ => {}
    }

    let command = lookup(&name)?;
    if wants_help {
        println!("{}", help(command, &target));
        return Ok(());
    }
    let request = (command.parse)(&mut args)?;
    if let Some(extra) = args.first() {
        return Err(commands::usage(command.name, &format!("unexpected argument \"{extra}\"")));
    }
    let mut mux = Kmux::connect_to(&target)?;
    let reply = match mux.call(command.name, request.clone()) {
        Ok(reply) => reply,
        Err(error) => return Err(explain(command, Failure::from(error), &mut mux)),
    };
    if json_output {
        println!("{reply}");
    } else {
        let text = (command.show)(&request, &reply);
        if !text.is_empty() {
            println!("{text}");
        }
    }
    Ok(())
}

fn lookup(name: &str) -> Result<&'static Command, Failure> {
    commands::find(name).ok_or_else(|| {
        let similar = commands::similar(name);
        let hint = if similar.is_empty() { String::new() } else { format!(" Did you mean: {}?", similar.join(", ")) };
        fail(exit::USAGE, format!("unknown command \"{name}\".{hint} Run `kmux --help` for all commands."))
    })
}

/// Adds the next step to an error from kmux.
fn explain(command: &Command, mut failure: Failure, mux: &mut Kmux) -> Failure {
    match failure.code {
        exit::NOT_FOUND => failure.message += "\nRun `kmux list` to see pane names and IDs, tabs (t1, …) and windows (w1, …).",
        exit::UNSUPPORTED => {
            let supported = mux.call("capabilities", json!({})).map(|c| output::join(&c["commands"])).unwrap_or_default();
            failure.message = format!("the running kmux doesn't support `{}` yet. It supports: {supported}", command.name);
        }
        exit::USAGE => failure.message += &format!("\nusage: {}\nmore: kmux help {}", command.usage, command.name),
        _ => {}
    }
    failure
}

/// The commands the running instance supports, if it is running (help never
/// starts kmux).
fn supported(target: &Target) -> Option<Vec<String>> {
    let reply = Kmux::try_connect(target)?.call("capabilities", json!({})).ok()?;
    Some(reply["commands"].as_array()?.iter().filter_map(|c| c.as_str().map(String::from)).collect())
}

const UNSUPPORTED: &str = "  [not in the running kmux]";

fn mark(command: &Command, supported: Option<&[String]>) -> &'static str {
    match supported {
        Some(names) if !names.iter().any(|n| n == command.name) => UNSUPPORTED,
        _ => "",
    }
}

const INSTANCES_HELP: &str = "kmux instances — List the running kmux instances.\n\
\n\
usage: kmux instances [--json]\n\
\n\
Each instance is a separate kmux app with its own windows and socket. The default\n\
instance listens on kmux.sock; one named work listens on kmux-work.sock beside it.\n\
\n\
Commands go to:\n  \
  1. the instance named by --instance NAME, if given;\n  \
  2. else $KMUX_SOCKET (kmux sets it, with $KMUX_INSTANCE and $KMUX_PANE, in its\n     \
     terminals, so kmux run inside a pane controls that pane's kmux);\n  \
  3. else the instance named by $KMUX_INSTANCE;\n  \
  4. else the default instance.\n\
Commands start the instance if it isn't running. In the app, kmux → New Instance\n\
starts one named 2, 3, ….\n\
\n\
examples:\n  \
  kmux instances\n      \
      Which instances are running, and which one commands go to.\n  \
  kmux --instance work open --name server --cmd \"npm run dev\"\n      \
      Start (or use) the instance named work, and open a pane in it.\n  \
  KMUX_INSTANCE=work kmux list\n      \
      The same choice, for every command in a script.";

fn instances(target: &Target, json_output: bool) -> Result<(), Failure> {
    let running: Vec<(Target, Value)> = Target::running()
        .into_iter()
        .map(|t| {
            let list = Kmux::try_connect(&t).and_then(|mut m| m.call("list", json!({})).ok()).unwrap_or(json!({}));
            (t, list)
        })
        .collect();
    let count = |list: &Value, key: &str| list[key].as_array().map_or(0, Vec::len);
    if json_output {
        let rows: Vec<Value> = running
            .iter()
            .map(|(t, list)| json!({
                "name": t.instance, "socket": t.socket, "current": t.socket == target.socket,
                "windows": count(list, "windows"), "panes": count(list, "panes"),
            }))
            .collect();
        println!("{}", json!({ "instances": rows, "current": { "name": target.instance, "socket": target.socket } }));
        return Ok(());
    }
    if running.is_empty() {
        println!("No kmux instances are running. Any command starts one, e.g. kmux open (default) or kmux --instance work open.");
        return Ok(());
    }
    let plural = |n: usize, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
    let width = running.iter().map(|(t, _)| t.instance.len()).max().unwrap_or(0);
    for (t, list) in &running {
        let star = if t.socket == target.socket { "*" } else { " " };
        println!("{star} {:<width$}  {}, {}", t.instance, plural(count(list, "windows"), "window"), plural(count(list, "panes"), "pane"));
    }
    println!("\n* the instance this shell's kmux commands talk to ({}). Pick another with --instance NAME or KMUX_INSTANCE.", target.socket.display());
    Ok(())
}

fn overview(target: &Target) -> String {
    let supported = supported(target);
    let mut out = String::from(
        "kmux — control the kmux terminal multiplexer (windows → tabs → split panes).\n\
         \n\
         usage: kmux [--instance NAME] [--bg] [--fresh] COMMAND [ARGS] [--json]\n",
    );
    for group in GROUPS {
        out += &format!("\n{group}:\n");
        for command in COMMANDS.iter().filter(|c| c.group == *group) {
            out += &format!("  {:<13} {}{}\n", command.name, command.summary, mark(command, supported.as_deref()));
        }
    }
    out += &format!(
        "\nMore:\n  {:<13} {}\n  {:<13} {}\n  {:<13} {}\n  {:<13} {}\n",
        "help COMMAND", "Details, options and examples for one command (or: kmux COMMAND --help).",
        "commands", "All commands with summaries; `kmux commands --json` describes them for scripts.",
        "raw REQUEST", "Send a control-protocol request as JSON, e.g. kmux raw '{\"cmd\":\"list\"}'.",
        "instances", "List the running kmux instances (separate apps, each with its own windows).",
    );
    out += &format!(
        "\nReferring to things:\n  \
         PANE    a pane's name (set with `open --name`) or its ID, e.g. p1\n  \
         TAB     a tab ID, e.g. t1;  WINDOW  a window ID, e.g. w1  (all shown by `kmux list`)\n  \
         FRACTION  1/3, 25% or 0.25\n\
         \n\
         Quick start:\n  \
         kmux open --name server --cmd \"npm run dev\"\n  \
         kmux open --name logs --split right --size 1/3 --cmd \"tail -f app.log\"\n  \
         kmux list\n  \
         kmux open --window new --name scratch      (a new window)\n\
         \n\
         {}\n\
         --json prints the reply from kmux as JSON. kmux must be running; the CLI starts it if it isn't\n\
         (in front, or behind your other windows with --bg or KMUX_BG=1). A started kmux brings back the layout\n\
         it saved when it last quit; --fresh (or KMUX_FRESH=1) opens a new window instead.\n\
         Instance: {} at {} (--instance NAME for another; kmux help instances).\n\
         Exit codes: 0 ok, 1 failed, 2 bad usage, 3 kmux not reachable, 4 not found, 5 not supported by the running kmux.",
        match &supported {
            None => "kmux isn't running, so help can't check which commands it supports.".to_string(),
            Some(_) if COMMANDS.iter().any(|c| !mark(c, supported.as_deref()).is_empty()) =>
                format!("Commands marked {} aren't in the running kmux yet.", UNSUPPORTED.trim()),
            Some(_) => "The running kmux supports every command.".to_string(),
        },
        target.instance,
        target.socket.display()
    );
    out
}

fn help(command: &Command, target: &Target) -> String {
    let mut out = format!("kmux {} — {}\n\nusage: {}\n", command.name, command.summary, command.usage);
    if !mark(command, supported(target).as_deref()).is_empty() {
        out += &format!("\nThe running kmux ({}) doesn't support {} yet.\n", target.instance, command.name);
    }
    let options: Vec<(&str, &str)> = command.options.iter().copied().chain([("--json", "Print the reply as JSON.")]).collect();
    out += "\noptions:\n";
    let width = options.iter().map(|(flag, _)| flag.len()).max().unwrap_or(0);
    for (flag, text) in options {
        out += &format!("  {flag:<width$}  {text}\n");
    }
    if !command.notes.is_empty() {
        out += &format!("\n{}\n", command.notes);
    }
    out += "\nexamples:\n";
    for (example, text) in command.examples {
        out += &format!("  {example}\n      {text}\n");
    }
    out.trim_end().to_string()
}

fn describe(supported: Option<&[String]>) -> Value {
    json!({
        "usage": "kmux [--instance NAME] [--bg] [--fresh] COMMAND [ARGS] [--json]",
        "globalOptions": [
            { "flag": "--instance NAME", "description": "Talk to the kmux instance NAME (default: $KMUX_SOCKET, else $KMUX_INSTANCE, else default). See kmux help instances." },
            { "flag": "--bg", "description": "If kmux has to be started, keep it behind your other windows (also KMUX_BG=1)." },
            { "flag": "--fresh", "description": "If kmux has to be started, open a new window instead of the layout it saved when it last quit (also KMUX_FRESH=1)." },
            { "flag": "--json", "description": "Print the reply as JSON." }
        ],
        "running": supported.is_some(),
        "refs": {
            "PANE": "a pane name (open --name) or ID like p1",
            "TAB": "a tab ID like t1",
            "WINDOW": "a window ID like w1",
            "FRACTION": "1/3, 25% or 0.25"
        },
        "exitCodes": { "0": "ok", "1": "failed", "2": "bad usage", "3": "kmux not reachable", "4": "not found", "5": "not supported by the running kmux" },
        "commands": COMMANDS.iter().map(|c| json!({
            "name": c.name,
            "group": c.group,
            "summary": c.summary,
            "supported": supported.map(|names| names.iter().any(|n| n == c.name)),
            "usage": c.usage,
            "options": c.options.iter().map(|(flag, text)| json!({ "flag": flag, "description": text })).collect::<Vec<_>>(),
            "notes": c.notes,
            "examples": c.examples.iter().map(|(example, text)| json!({ "command": example, "description": text })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Splits an example command line the way a POSIX shell would (enough for
    /// the quoting the examples use).
    fn shell_words(line: &str) -> Vec<String> {
        let mut words = vec![];
        let mut word = String::new();
        let mut quote: Option<char> = None;
        let mut started = false;
        for c in line.chars() {
            match (quote, c) {
                (Some(q), c) if c == q => quote = None,
                (Some(_), c) => word.push(c),
                (None, '"' | '\'') => {
                    quote = Some(c);
                    started = true;
                }
                (None, ' ') => {
                    if started || !word.is_empty() {
                        words.push(std::mem::take(&mut word));
                    }
                    started = false;
                }
                (None, c) => word.push(c),
            }
        }
        if started || !word.is_empty() {
            words.push(word);
        }
        words
    }

    fn parse(line: &str) -> Result<(String, Value), Failure> {
        let words = shell_words(line);
        assert_eq!(words[0], "kmux");
        let mut args = Args::new(words[1..].to_vec());
        args.flag("--json");
        let name = args.positional().unwrap();
        let command = lookup(&name)?;
        let request = (command.parse)(&mut args)?;
        if let Some(extra) = args.first() {
            return Err(commands::usage(command.name, &format!("unexpected argument \"{extra}\"")));
        }
        Ok((command.name.to_string(), request))
    }

    #[test]
    fn every_documented_example_parses() {
        for command in COMMANDS {
            assert!(!command.examples.is_empty(), "{} has no examples", command.name);
            for (example, _) in command.examples {
                let (name, _) = parse(example).unwrap_or_else(|f| panic!("{example}: {}", f.message));
                assert_eq!(name, command.name, "{example}");
            }
        }
    }

    #[test]
    fn requests() {
        let cases = [
            ("kmux open", json!({ "type": "term" })),
            ("kmux open --name logs --split right --size 1/3 --cmd 'tail -f a.log'", json!({ "type": "term", "name": "logs", "split": "right", "size": "1/3", "cmd": "tail -f a.log" })),
            ("kmux open web localhost:3000 --split down", json!({ "type": "web", "url": "localhost:3000", "split": "down" })),
            ("kmux open --tab --no-wait", json!({ "type": "term", "tab": true, "wait": false })),
            ("kmux close logs", json!({ "pane": "logs" })),
            ("kmux close --window w2", json!({ "window": "w2" })),
            ("kmux rename-tab t1 my servers", json!({ "tab": "t1", "title": "my servers" })),
            ("kmux send server npm test", json!({ "pane": "server", "text": "npm test" })),
            ("kmux move logs --to server --side bottom", json!({ "pane": "logs", "to": "server", "side": "bottom" })),
            ("kmux move-tab t2 --index 0", json!({ "tab": "t2", "index": 0 })),
            ("kmux open web a.test --history", json!({ "type": "web", "url": "a.test", "history": true })),
            ("kmux navigate site --back", json!({ "pane": "site", "back": true })),
            ("kmux open md /tmp/a.md", json!({ "type": "md", "path": "/tmp/a.md" })),
            ("kmux navigate spec /tmp/b.md", json!({ "pane": "spec", "path": "/tmp/b.md" })),
        ];
        for (line, expected) in cases {
            let (_, request) = parse(line).unwrap_or_else(|f| panic!("{line}: {}", f.message));
            assert_eq!(request, expected, "{line}");
        }
    }

    #[test]
    fn usage_errors_say_what_to_do() {
        for (line, hint) in [
            ("kmux close", "PANE, --tab TAB or --window WINDOW"),
            ("kmux zoom", "missing PANE"),
            ("kmux open top", "--cmd \"top\""),
            ("kmux open web", "need a URL"),
            ("kmux move logs", "say where"),
            ("kmux zoom a b", "unexpected argument \"b\""),
            ("kmux open --history", "only for web panes"),
            ("kmux open md", "need a file"),
            ("kmux open notes.md", "kmux open md notes.md"),
            ("kmux navigate site --back x", "one of URL"),
            ("kmux lsit", "Did you mean: list"),
            ("kmux rename t1 x", "Did you mean: rename-tab"),
        ] {
            let failure = parse(line).expect_err(line);
            assert_eq!(failure.code, exit::USAGE, "{line}");
            assert!(failure.message.contains(hint), "{line}: {}", failure.message);
        }
    }

    #[test]
    fn layouts_print_as_expressions() {
        let layout = json!({ "split": "row", "children": [
            { "pane": "a", "size": "2/3" },
            { "split": "column", "size": "1/3", "children": [{ "pane": "b", "size": "1/2" }, { "pane": "c", "size": "1/2" }] }
        ] });
        assert_eq!(output::expression(&layout, true), "a:2/3 | (b:1/2 / c:1/2):1/3");
    }
}
