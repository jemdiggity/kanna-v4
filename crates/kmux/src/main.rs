//! kmux: the command line for the kmux terminal multiplexer. Each command
//! sends one control-protocol request (docs/kmux-spec.md §7) to the running
//! app over its Unix socket.

mod commands;
mod output;

use commands::{Command, COMMANDS, GROUPS};
use kmux_client::{exit, fail, socket_path, Args, Failure, Kmux};
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
    let Some(name) = args.positional() else {
        if args.flag("--version") {
            println!("kmux {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        if let Some(extra) = args.first() {
            return Err(fail(exit::USAGE, format!("unknown option \"{extra}\"\n\n{}", overview())));
        }
        println!("{}", overview());
        return Ok(());
    };

    match name.as_str() {
        "help" => {
            match args.positional() {
                None => println!("{}", overview()),
                Some(topic) => println!("{}", help(lookup(&topic)?)),
            }
            return Ok(());
        }
        "commands" => {
            if json_output {
                println!("{}", serde_json::to_string_pretty(&describe()).unwrap());
            } else {
                for command in COMMANDS {
                    println!("{:<13} {}", command.name, command.summary);
                }
            }
            return Ok(());
        }
        "raw" => {
            if wants_help {
                println!("usage: kmux raw REQUEST\n\nSends one control-protocol request as JSON and prints the reply, e.g.\n  kmux raw '{{\"cmd\":\"list\"}}'\n  kmux raw '{{\"cmd\":\"open\",\"args\":{{\"type\":\"term\",\"name\":\"a\"}}}}'");
                return Ok(());
            }
            let text = args.positional().ok_or_else(|| fail(exit::USAGE, "raw: missing REQUEST, e.g. kmux raw '{\"cmd\":\"list\"}'"))?;
            let request: Value = serde_json::from_str(&text).map_err(|e| fail(exit::USAGE, format!("raw: REQUEST is not valid JSON: {e}")))?;
            let cmd = request["cmd"].as_str().ok_or_else(|| fail(exit::USAGE, "raw: REQUEST needs a \"cmd\""))?;
            let reply = Kmux::connect()?.call(cmd, request.get("args").cloned().unwrap_or(json!({})))?;
            println!("{reply}");
            return Ok(());
        }
        _ => {}
    }

    let command = lookup(&name)?;
    if wants_help {
        println!("{}", help(command));
        return Ok(());
    }
    let request = (command.parse)(&mut args)?;
    if let Some(extra) = args.first() {
        return Err(commands::usage(command.name, &format!("unexpected argument \"{extra}\"")));
    }
    let mut mux = Kmux::connect()?;
    let reply = match mux.call(command.name, request) {
        Ok(reply) => reply,
        Err(error) => return Err(explain(command, Failure::from(error), &mut mux)),
    };
    if json_output {
        println!("{reply}");
    } else {
        let text = (command.show)(&reply);
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

fn overview() -> String {
    let mut out = String::from(
        "kmux — control the kmux terminal multiplexer (windows → tabs → split panes).\n\
         \n\
         usage: kmux COMMAND [ARGS] [--json]\n",
    );
    for group in GROUPS {
        out += &format!("\n{group}:\n");
        for command in COMMANDS.iter().filter(|c| c.group == *group) {
            out += &format!("  {:<13} {}\n", command.name, command.summary);
        }
    }
    out += &format!(
        "\nMore:\n  {:<13} {}\n  {:<13} {}\n  {:<13} {}\n",
        "help COMMAND", "Details, options and examples for one command (or: kmux COMMAND --help).",
        "commands", "All commands with summaries; `kmux commands --json` describes them for scripts.",
        "raw REQUEST", "Send a control-protocol request as JSON, e.g. kmux raw '{\"cmd\":\"list\"}'.",
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
         kmux list\n\
         \n\
         Not every kmux build has every command yet: `kmux capabilities` lists what the running one supports.\n\
         --json prints the reply from kmux as JSON. kmux must be running; the CLI starts it if it isn't.\n\
         Socket: {} (set KMUX_SOCKET to use another).\n\
         Exit codes: 0 ok, 1 failed, 2 bad usage, 3 kmux not reachable, 4 not found, 5 not supported by the running kmux.",
        socket_path().display()
    );
    out
}

fn help(command: &Command) -> String {
    let mut out = format!("kmux {} — {}\n\nusage: {}\n", command.name, command.summary, command.usage);
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

fn describe() -> Value {
    json!({
        "usage": "kmux COMMAND [ARGS] [--json]",
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
