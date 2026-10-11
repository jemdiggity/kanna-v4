pub mod args;
pub mod commands;
pub mod complete;
pub mod output;
pub mod target;
pub mod threads;

use args::Argv;
use commands::{Command, COMMANDS, GROUPS};
use kanna_client::{exit, fail, Failure};
use serde_json::{json, Value};
use target::{process_env, Backend, Choice, Env, Session, Target};

pub fn main() -> std::process::ExitCode {
    match run(std::env::args().skip(1).collect(), &process_env) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(failure) => {
            eprintln!("kanna: {}", failure.message);
            std::process::ExitCode::from(failure.code)
        }
    }
}

pub fn run(args: Vec<String>, env: Env) -> Result<(), Failure> {
    if args.first().is_some_and(|a| a == "__complete") {
        let start = if args.get(1).is_some_and(|a| a == "--") { 2 } else { 1 };
        let words = if args.len() > start { args[start..].to_vec() } else { vec![String::new()] };
        for line in complete::lines(&complete::wants(&words), &words, env) {
            println!("{line}");
        }
        return Ok(());
    }
    let mut args = Argv::new(args);
    let json_output = args.flag("--json");
    let wants_help = args.flag("--help") || args.flag("-h");
    let background = args.flag("--bg");
    let fresh = args.flag("--fresh");
    let instance = args.option("--instance")?;
    let mux = args.option("--mux")?;
    let target = Target::resolve(&kanna_build::Build::current(), Choice { mux, instance, background, fresh }, env)?;
    let Some(name) = args.positional() else {
        if args.flag("--version") {
            println!("kanna {} ({})", env!("CARGO_PKG_VERSION"), target.build.app_name());
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
                Some(topic) if topic == "raw" => println!("{RAW_HELP}"),
                Some(topic) if topic == "completions" => println!("{COMPLETIONS_HELP}"),
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
                    println!("{:<13} {}{}", command.name, command.summary, mark(command, supported.as_deref(), &target));
                }
            }
            return Ok(());
        }
        "completions" => {
            match args.positional().as_deref() {
                Some("zsh") => print!("{}", complete::ZSH),
                Some("bash") => print!("{}", complete::BASH),
                _ => println!("{COMPLETIONS_HELP}"),
            }
            return Ok(());
        }
        "instances" => {
            if wants_help {
                println!("{INSTANCES_HELP}");
                return Ok(());
            }
            if args.positional().as_deref() == Some("prune") {
                return prune(env);
            }
            return instances(&target, json_output, env);
        }
        "raw" => {
            if wants_help {
                println!("{RAW_HELP}");
                return Ok(());
            }
            let text = args.positional().ok_or_else(|| fail(exit::USAGE, "raw: missing REQUEST, e.g. kanna raw '{\"cmd\":\"list\"}'"))?;
            let request: Value = serde_json::from_str(&text).map_err(|e| fail(exit::USAGE, format!("raw: REQUEST is not valid JSON: {e}")))?;
            let cmd = request["cmd"].as_str().ok_or_else(|| fail(exit::USAGE, "raw: REQUEST needs a \"cmd\""))?;
            let reply = Session::connect(&target, env)?.call(cmd, request.get("args").cloned().unwrap_or(json!({})))?;
            println!("{reply}");
            return Ok(());
        }
        _ => {}
    }

    let (command, request) = match project_folder(&name) {
        Some(folder) => (lookup("task")?, json!({ "action": "desk", "project": folder })),
        None => {
            let command = lookup(&name)?;
            if wants_help {
                println!("{}", help(command, &target));
                return Ok(());
            }
            (command, (command.parse)(&mut args)?)
        }
    };
    if let Some(extra) = args.first() {
        return Err(commands::usage(command.name, &format!("unexpected argument \"{extra}\"")));
    }
    if request.get("local").is_some() {
        return threads::run(&request, json_output);
    }
    let mut mux = Session::connect(&target, env)?;
    let mut reply = match mux.call(wire(command, &request), request.clone()) {
        Ok(reply) => reply,
        Err(failure) => return Err(explain(command, failure, &mut mux, &target)),
    };
    if command.name == "task" && request["action"] == "list" {
        if let Ok(backlog) = mux.call("backlog", serde_json::json!({ "action": "list" })) {
            reply["backlog"] = backlog["tasks"].clone();
        }
    }
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

/// `kanna PATH`: a word that isn't a command but is a folder opens a Desk on its project.
fn project_folder(name: &str) -> Option<String> {
    if commands::find(name).is_some() {
        return None;
    }
    let folder = std::fs::canonicalize(commands::absolute(name)).ok().filter(|p| p.is_dir())?;
    Some(folder.to_string_lossy().into_owned())
}

fn lookup(name: &str) -> Result<&'static Command, Failure> {
    commands::find(name).ok_or_else(|| {
        let similar = commands::similar(name);
        let hint = if similar.is_empty() { String::new() } else { format!(" Did you mean: {}?", similar.join(", ")) };
        fail(exit::USAGE, format!("unknown command \"{name}\".{hint} Run `kanna --help` for all commands."))
    })
}

fn explain(command: &Command, mut failure: Failure, mux: &mut Session, target: &Target) -> Failure {
    match failure.code {
        exit::NOT_FOUND => failure.message += "\nRun `kanna list` to see pane names and IDs, tabs (t1, …) and windows (w1, …).",
        exit::UNSUPPORTED => {
            let supported = mux.call("capabilities", json!({})).map(|c| output::join(&c["commands"])).unwrap_or_default();
            failure.message = format!("the running {} doesn't support `{}` yet. It supports: {supported}", target.name(), command.name);
        }
        exit::USAGE => failure.message += &format!("\nusage: {}\nmore: kanna help {}", command.usage, command.name),
        _ => {}
    }
    failure
}

fn wire(command: &Command, request: &Value) -> &'static str {
    match command.name {
        "markup" if request.get("type").is_some() => "open",
        "task" if ["new", "start", "defer"].contains(&request["action"].as_str().unwrap_or("")) => "backlog",
        "task" if request["action"] == "done" && request["task"].as_str().is_some_and(commands::is_backlog_task) => "backlog",
        "task" if request["action"] == "done" => "close",
        "task" if request["action"] == "name" => "rename-pane",
        name => name,
    }
}

fn supported(target: &Target) -> Option<Vec<String>> {
    let reply = target::capabilities(target)?;
    Some(reply["commands"].as_array()?.iter().filter_map(|c| c.as_str().map(String::from)).collect())
}

fn unsupported_mark(target: &Target) -> String {
    format!("  [not in the running {}]", target.name())
}

fn mark(command: &Command, supported: Option<&[String]>, target: &Target) -> String {
    match supported {
        Some(names) if !names.iter().any(|n| n == command.name) => unsupported_mark(target),
        _ => String::new(),
    }
}

const COMPLETIONS_HELP: &str = "usage: kanna completions zsh|bash\n\nPrints the tab-completion script for your shell. Load it from your shell's startup file:\n  zsh:  source <(kanna completions zsh)     in ~/.zshrc\n  bash: eval \"$(kanna completions bash)\"  in ~/.bashrc\n\nTab then completes commands, options, pane names and IDs, tabs, windows, instances,\nand files of the kind a command takes (markdown for open md, HTML for markup).";

const RAW_HELP: &str = "usage: kanna raw REQUEST\n\nSends one control-protocol request as JSON and prints the reply, e.g.\n  kanna raw '{\"cmd\":\"list\"}'\n  kanna raw '{\"cmd\":\"open\",\"args\":{\"type\":\"term\",\"name\":\"a\"}}'";

const INSTANCES_HELP: &str = "kanna instances — List the running Kanna instances.\n\
\n\
usage: kanna instances [--json]\n\
       kanna instances prune\n\
\n\
Each instance is a separate Kanna: its own daemon, keeper, terminals, Kanna.app and saved\n\
layout, in its own folder in your application support folder. Kanna comes in three channels,\n\
each a separate app that can run beside the others: production (Kanna, folder kanna-prod),\n\
staging (Kanna Staging, kanna-staging) and dev (Kanna Dev, kanna-dev). An instance named\n\
work uses the folder kanna-dev-work, with its socket at kanna-dev-work/kanna.sock. A dev\n\
build made in a git worktree is an app of its own whose default instance is named after the\n\
worktree: built in .kanna-worktrees/task-3f9a1c2e, it is Kanna Dev task-3f9a1c2e, keeping\n\
its files in kanna-dev-task-3f9a1c2e.\n\
\n\
Commands go to an instance of this kanna's channel:\n  \
  1. the instance named by --instance NAME, if given;\n  \
  2. else $KANNA_SOCKET (Kanna sets it, with $KANNA_INSTANCE and $KANNA_PANE, in its\n     \
     terminals, so kanna run inside a pane controls that pane's Kanna, whatever its channel);\n  \
  3. else the instance named by $KANNA_INSTANCE;\n  \
  4. else this build's default instance (its worktree's, for a dev build in a worktree).\n\
Commands start the instance's Kanna.app if nothing answers (the app starts its daemon),\n\
but never for $KANNA_SOCKET.\n\
\n\
kanna instances lists the running instances of every channel, with the worktree a dev\n\
build came from. kanna instances prune deletes the folders of dev instances whose worktree\n\
is gone, once nothing runs in them.\n\
\n\
examples:\n  \
  kanna instances\n      \
      Which instances are running, and which one commands go to.\n  \
  kanna --instance work open --name server --cmd \"npm run dev\"\n      \
      Start (or use) the instance named work, and open a pane in it.\n  \
  KANNA_INSTANCE=work kanna list\n      \
      The same choice, for every command in a script.\n  \
  kanna instances prune\n      \
      Clean up after worktrees you've removed.";

fn instances(target: &Target, json_output: bool, env: Env) -> Result<(), Failure> {
    let running: Vec<(target::Instance, Value)> = target::instances(env)
        .into_iter()
        .filter(target::Instance::answers)
        .map(|i| {
            let socket = i.socket.clone();
            let probe = Target { instance: i.name.clone(), socket, pinned: None, ..target.clone() };
            let list = probe.try_connect().and_then(|mut m| m.call("list", json!({})).ok()).unwrap_or(json!({}));
            (i, list)
        })
        .collect();
    let count = |list: &Value, key: &str| list[key].as_array().map_or(0, Vec::len);
    if json_output {
        let rows: Vec<Value> = running
            .iter()
            .map(|(i, list)| json!({
                "name": i.name, "channel": i.channel.id(), "worktree": i.worktree, "socket": i.socket, "current": i.socket == target.socket,
                "windows": count(list, "windows"), "panes": count(list, "panes"),
            }))
            .collect();
        println!("{}", json!({ "instances": rows, "current": { "name": target.instance, "channel": target.build.channel.id(), "socket": target.socket } }));
        return Ok(());
    }
    if running.is_empty() {
        println!("No Kanna instances are running. Any command starts one, e.g. kanna open or kanna --instance work open.");
        return Ok(());
    }
    let plural = |n: usize, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
    let home = env("HOME").unwrap_or_default();
    let rows: Vec<[String; 5]> = running
        .iter()
        .map(|(i, list)| {
            let star = if i.socket == target.socket { "*" } else { " " };
            let worktree = i.worktree.as_ref().map_or(String::new(), |w| {
                let shown = w.display().to_string();
                let shown = if !home.is_empty() && shown.starts_with(&home) { format!("~{}", &shown[home.len()..]) } else { shown };
                if w.exists() { shown } else { format!("{shown} (gone)") }
            });
            [star.into(), i.name.clone(), i.channel.id().into(), format!("{}, {}", plural(count(list, "windows"), "window"), plural(count(list, "panes"), "pane")), worktree]
        })
        .collect();
    let width = |n: usize| rows.iter().map(|r| r[n].len()).max().unwrap_or(0);
    let (name, channel, counts) = (width(1), width(2), width(3));
    for row in &rows {
        println!("{}", format!("{} {:<name$}  {:<channel$}  {:<counts$}  {}", row[0], row[1], row[2], row[3], row[4]).trim_end());
    }
    let variable = target.brand().variable("INSTANCE");
    println!("\n* the instance this shell's kanna commands talk to ({}). Pick another with --instance NAME or {variable}.", target.socket.display());
    Ok(())
}

fn prune(env: Env) -> Result<(), Failure> {
    let (mut pruned, mut kept) = (0, 0);
    for instance in target::instances(env) {
        let Some(worktree) = &instance.worktree else { continue };
        if worktree.exists() {
            continue;
        }
        let folder = instance.folder.display();
        if instance.answers() {
            println!("kept {folder}: its worktree {} is gone, but it's running (quit it first)", worktree.display());
            kept += 1;
            continue;
        }
        if let Some(pid) = target::keeper_pid(&instance.folder) {
            println!("kept {folder}: its worktree {} is gone, but its keeper (pid {pid}) still holds terminals (kill {pid} first)", worktree.display());
            kept += 1;
            continue;
        }
        std::fs::remove_dir_all(&instance.folder).map_err(|e| fail(exit::FAILED, format!("can't delete {folder}: {e}")))?;
        println!("deleted {folder} (worktree {} is gone)", worktree.display());
        pruned += 1;
    }
    if pruned + kept == 0 {
        println!("Nothing to prune: every dev instance's worktree is still there, or it's still running.");
    }
    Ok(())
}

fn overview(target: &Target) -> String {
    let supported = supported(target);
    let mut out = String::from(
        "kanna — control the Kanna terminal multiplexer (windows → tabs → split panes).\n\
         \n\
         usage: kanna [--mux kanna|kmux] [--instance NAME] [--bg] [--fresh] COMMAND [ARGS] [--json] [-- TEXT]\n\
         \x20      kanna PATH                           (a new Desk on the project in folder PATH)\n",
    );
    for group in GROUPS {
        out += &format!("\n{group}:\n");
        for command in COMMANDS.iter().filter(|c| c.group == *group) {
            out += &format!("  {:<13} {}{}\n", command.name, command.summary, mark(command, supported.as_deref(), target));
        }
    }
    out += &format!(
        "\nMore:\n  {:<13} {}\n  {:<13} {}\n  {:<13} {}\n  {:<13} {}\n  {:<13} {}\n",
        "help COMMAND", "Details, options and examples for one command (or: kanna COMMAND --help).",
        "commands", "All commands with summaries; `kanna commands --json` describes them for scripts.",
        "raw REQUEST", "Send a control-protocol request as JSON, e.g. kanna raw '{\"cmd\":\"list\"}'.",
        "instances", "List the running Kanna instances (separate daemons and apps, each with its own panes).",
        "completions", "Tab completion for zsh or bash: source <(kanna completions zsh) in ~/.zshrc.",
    );
    let name = target.name();
    let running = match &supported {
        None => format!("{name} isn't running, so help can't check which commands it supports."),
        Some(_) if COMMANDS.iter().any(|c| !mark(c, supported.as_deref(), target).is_empty()) => {
            format!("Commands marked {} aren't in the running {name} yet.", unsupported_mark(target).trim())
        }
        Some(_) => format!("The running {name} supports every command."),
    };
    let backend = match target.backend {
        Backend::Kanna => String::new(),
        Backend::Kmux => "Backend: kmux (--mux kmux), not Kanna.\n".into(),
    };
    out += &format!(
        "\nReferring to things:\n  \
         PANE    a pane's name (set with `open --name`) or its ID, e.g. p1\n  \
         TAB     a tab ID, e.g. t1;  WINDOW  a window ID, e.g. w1  (all shown by `kanna list`)\n  \
         FRACTION  1/3, 25% or 0.25\n\
         \n\
         Quick start:\n  \
         kanna open --name server --cmd \"npm run dev\"\n  \
         kanna open --name logs --split right --size 1/3 --cmd \"tail -f app.log\"\n  \
         kanna list\n  \
         kanna read server\n  \
         kanna open --window new --name scratch      (a new window)\n\
         \n\
         {running}\n\
         --json prints the reply from Kanna as JSON; -- ends the options (kanna send p1 -- --json types --json).\n\
         Terminals live in Kanna's daemon and keep running with Kanna.app closed: list, send, read, close, restart\n\
         and open (unplaced) work without the app. Commands that need it start it if it isn't running (in front,\n\
         or behind your other windows with --bg or KANNA_BG=1), as does any command when nothing answers. A started\n\
         Kanna.app brings back the layout it saved; --fresh (or KANNA_FRESH=1) opens a new window instead.\n\
         With KANNA_SOCKET set, kanna never starts the app.\n\
         {backend}\
         Instance: {} of {} at {} (--instance NAME for another; kanna help instances).\n\
         Exit codes: 0 ok, 1 failed, 2 bad usage, 3 Kanna not reachable, 4 not found, 5 not supported by the running Kanna,\n\
         6 needs Kanna.app, which isn't attached and wasn't started.",
        target.instance,
        target.build.app_name(),
        target.socket.display()
    );
    out
}

fn help(command: &Command, target: &Target) -> String {
    let mut out = format!("kanna {} — {}\n\nusage: {}\n", command.name, command.summary, command.usage);
    if !mark(command, supported(target).as_deref(), target).is_empty() {
        out += &format!("\nThe running {} ({}) doesn't support {} yet.\n", target.name(), target.instance, command.name);
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

pub fn describe(supported: Option<&[String]>) -> Value {
    json!({
        "usage": "kanna [--mux kanna|kmux] [--instance NAME] [--bg] [--fresh] COMMAND [ARGS] [--json] [-- TEXT]",
        "globalOptions": [
            { "flag": "--mux kanna|kmux", "description": "Talk to Kanna (default; also KANNA_MUX) or to a kmux you run." },
            { "flag": "--instance NAME", "description": "Talk to the Kanna instance NAME (default: $KANNA_SOCKET, else $KANNA_INSTANCE, else default). See kanna help instances." },
            { "flag": "--bg", "description": "If Kanna.app has to be started, keep it behind your other windows (also KANNA_BG=1)." },
            { "flag": "--fresh", "description": "If Kanna.app has to be started, open a new window instead of the layout it saved (also KANNA_FRESH=1)." },
            { "flag": "--json", "description": "Print the reply as JSON." },
            { "flag": "--", "description": "End the options: what follows is text, e.g. kanna send p1 -- --json." }
        ],
        "running": supported.is_some(),
        "refs": {
            "PANE": "a pane name (open --name) or ID like p1",
            "TAB": "a tab ID like t1",
            "WINDOW": "a window ID like w1",
            "FRACTION": "1/3, 25% or 0.25"
        },
        "exitCodes": {
            "0": "ok", "1": "failed", "2": "bad usage", "3": "Kanna not reachable", "4": "not found", "5": "not supported by the running Kanna",
            "6": "needs Kanna.app, which isn't attached and wasn't started"
        },
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

    pub fn shell_words(line: &str) -> Vec<String> {
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
        // Run from a Kanna terminal, a command would name its pane as the caller.
        std::env::remove_var("KANNA_PANE");
        let words = shell_words(line);
        assert_eq!(words[0], "kanna");
        let mut args = Argv::new(words[1..].to_vec());
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
    fn a_local_page_opens_as_a_file_url_and_markup_opens_or_switches_comments() {
        let dir = std::env::temp_dir().join(format!("kanna-page-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let page = dir.join("a page.html");
        std::fs::write(&page, "<p>hi</p>").unwrap();
        let (_, request) = parse(&format!("kanna open web '{}'", page.display())).unwrap();
        assert_eq!(request["url"], json!(format!("file://{}", page.display().to_string().replace(' ', "%20"))));
        let (_, request) = parse("kanna open web a.test").unwrap();
        assert_eq!(request["url"], json!("a.test"));
        assert!(request.get("markup").is_none());
        let (name, request) = parse(&format!("kanna markup '{}' --tab", page.display())).unwrap();
        assert_eq!(name, "markup");
        assert_eq!(request["type"], json!("web"));
        assert_eq!(request["markup"], json!(true));
        assert_eq!(request["tab"], json!(true));
        let (_, request) = parse("kanna markup /tmp/notes.md --split right").unwrap();
        assert_eq!((request["type"].clone(), request["path"].clone(), request["markup"].clone()), (json!("md"), json!("/tmp/notes.md"), json!(true)));
        assert_eq!(parse("kanna markup spec --off").unwrap().1, json!({ "pane": "spec", "off": true }));
        assert_eq!(parse("kanna markup spec").unwrap().1["pane"], json!("spec"));
        assert!(parse("kanna markup spec --tab").is_err());
        assert_eq!(parse("kanna markup spec --to agent").unwrap().1, json!({ "pane": "spec", "agent": "agent" }));
        assert_eq!(parse("kanna markup /tmp/notes.md --to p4").unwrap().1["agent"], json!("p4"));
        assert!(parse("kanna markup spec --to agent --off").is_err());
        assert!(parse("kanna markup /tmp/notes.md --off").is_err());
        let (_, request) = parse("kanna open md /tmp/notes.md").unwrap();
        assert!(request.get("markup").is_none() && request.get("agent").is_none());
        assert_eq!(parse("kanna markup /tmp/m.html").unwrap().1["url"], json!("file:///tmp/m.html"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn requests() {
        let cases = [
            ("kanna open", json!({ "type": "term" })),
            ("kanna open --name logs --split right --size 1/3 --cmd 'tail -f a.log'", json!({ "type": "term", "name": "logs", "split": "right", "size": "1/3", "cmd": "tail -f a.log" })),
            ("kanna open web localhost:3000 --split down", json!({ "type": "web", "url": "localhost:3000", "split": "down" })),
            ("kanna open --tab --no-wait", json!({ "type": "term", "tab": true, "wait": false })),
            ("kanna close logs", json!({ "pane": "logs" })),
            ("kanna close --window w2", json!({ "window": "w2" })),
            ("kanna rename-tab t1 my servers", json!({ "tab": "t1", "title": "my servers" })),
            ("kanna send server npm test", json!({ "pane": "server", "text": "npm test" })),
            ("kanna move logs --to server --side bottom", json!({ "pane": "logs", "to": "server", "side": "bottom" })),
            ("kanna move-tab t2 --index 0", json!({ "tab": "t2", "index": 0 })),
            ("kanna open web a.test --history", json!({ "type": "web", "url": "a.test", "history": true })),
            ("kanna navigate site --back", json!({ "pane": "site", "back": true })),
            ("kanna open md /tmp/a.md", json!({ "type": "md", "path": "/tmp/a.md" })),
            ("kanna navigate spec /tmp/b.md", json!({ "pane": "spec", "path": "/tmp/b.md" })),
            ("kanna read s1", json!({ "pane": "s1" })),
            ("kanna read s1 --lines 5", json!({ "pane": "s1", "lines": 5 })),
            ("kanna send p1 -- --json", json!({ "pane": "p1", "text": "--json" })),
            ("kanna send p1 echo -- --mux -x", json!({ "pane": "p1", "text": "echo --mux -x" })),
            ("kanna open --cwd /tmp", json!({ "type": "term", "cwd": "/tmp" })),
            ("kanna open --name agent --dock", json!({ "type": "term", "name": "agent", "dock": "left" })),
            ("kanna open --dock right --cmd claude", json!({ "type": "term", "dock": "right", "cmd": "claude" })),
            ("kanna open --dock md /tmp/a.md", json!({ "type": "md", "dock": "left", "path": "/tmp/a.md" })),
            ("kanna open web a.test --dock=right --window new", json!({ "type": "web", "url": "a.test", "dock": "right", "window": "new" })),
            ("kanna move agent --dock", json!({ "pane": "agent", "dock": "left" })),
            ("kanna move agent --dock right --window w2", json!({ "pane": "agent", "dock": "right", "window": "w2" })),
            ("kanna move agent --tab", json!({ "pane": "agent", "tab": "new" })),
            ("kanna move agent --tab t3", json!({ "pane": "agent", "tab": "t3" })),
            ("kanna move agent --tab --window w2", json!({ "pane": "agent", "tab": "new", "window": "w2" })),
        ];
        for (line, expected) in cases {
            let (_, request) = parse(line).unwrap_or_else(|f| panic!("{line}: {}", f.message));
            assert_eq!(request, expected, "{line}");
        }
    }

    #[test]
    fn usage_errors_say_what_to_do() {
        for (line, hint) in [
            ("kanna close", "PANE, --tab TAB or --window WINDOW"),
            ("kanna zoom", "missing PANE"),
            ("kanna open top", "--cmd \"top\""),
            ("kanna open web", "need a URL"),
            ("kanna move logs", "say where"),
            ("kanna zoom a b", "unexpected argument \"b\""),
            ("kanna open --history", "only for web panes"),
            ("kanna open md", "need a file"),
            ("kanna open notes.md", "kanna open md notes.md"),
            ("kanna navigate site --back x", "one of URL"),
            ("kanna lsit", "Did you mean: list"),
            ("kanna rename t1 x", "Did you mean: rename-tab"),
            ("kanna read", "missing PANE"),
            ("kanna read p1 --lines 0", "--lines needs a whole number"),
            ("kanna open ios", "need an app"),
            ("kanna open --app x", "only for iOS panes"),
            ("kanna open --dock --split right", "--dock goes on its own"),
            ("kanna open --dock --tab", "--dock goes on its own"),
            ("kanna move agent --dock --tab", "--dock goes on its own"),
            ("kanna move agent --dock --to b", "--dock goes on its own"),
        ] {
            let failure = parse(line).expect_err(line);
            assert_eq!(failure.code, exit::USAGE, "{line}");
            assert!(failure.message.contains(hint), "{line}: {}", failure.message);
        }
    }

    #[test]
    fn relative_paths_are_made_absolute_here() {
        let (_, request) = parse("kanna open md docs/a.md --cwd sub").unwrap();
        assert!(request["path"].as_str().unwrap().starts_with('/'));
        assert!(request["cwd"].as_str().unwrap().starts_with('/'));
    }

    #[test]
    fn layouts_print_as_expressions() {
        let layout = json!({ "split": "row", "children": [
            { "pane": "a", "size": "2/3" },
            { "split": "column", "size": "1/3", "children": [{ "pane": "b", "size": "1/2" }, { "pane": "c", "size": "1/2" }] }
        ] });
        assert_eq!(output::expression(&layout, true), "a:2/3 | (b:1/2 / c:1/2):1/3");
    }

    #[test]
    fn docked_panes_print_on_their_own_line() {
        let reply = json!({
            "windows": [{ "id": "w1", "key": true, "focused": "agent", "dock": { "pane": "agent", "edge": "left", "width": "2/5" },
                          "tabs": [{ "id": "t1", "title": "Tab 1", "active": true, "layout": { "pane": "p2" } }] }],
            "panes": [{ "id": "p1", "name": "agent", "type": "term", "state": "running", "cmd": "claude" }, { "id": "p2", "name": null, "type": "term", "state": "running" }]
        });
        let text = output::list(&reply);
        assert!(text.contains("\n  dock: p1 (agent)  left 2/5\n  *t1 \"Tab 1\"  p2"), "{text}");
        let opened = json!({ "pane": { "id": "p1", "name": "agent", "state": "running" }, "window": "w1", "tab": null, "dock": "left" });
        assert_eq!(output::opened(&json!({}), &opened), "opened p1 (agent) docked left in window w1: running");
        let moved = json!({ "window": "w1", "dock": { "pane": "agent", "edge": "right", "width": "1/3" } });
        assert_eq!(output::moved(&moved), "agent is docked right in w1 (width 1/3)");
    }

    #[test]
    fn with_the_app_closed_open_and_list_say_so() {
        let opened = output::opened(&json!({}), &json!({ "pane": { "id": "p3", "name": "logs", "state": "running" }, "window": null, "tab": null }));
        assert_eq!(opened, "opened p3 (logs), unplaced until Kanna.app opens: running");
        let list = output::list(&json!({ "windows": [], "panes": [{ "id": "p1", "type": "term", "state": "running" }] }));
        assert!(list.starts_with("no windows: Kanna.app isn't open"), "{list}");
        assert!(list.contains("p1    -     term  running  (shell)"), "{list}");
        assert_eq!(output::list(&json!({ "windows": [], "panes": [] })), "no windows (open one with `kanna open`)");
    }
}
