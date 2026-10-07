//! Every kmux command: how it is documented, how its arguments become a
//! control-protocol request (docs/kmux-spec.md §7), and how its reply prints.
//! Help, `kmux commands --json` and parsing all come from this table.

use crate::output;
use kmux_client::{exit, fail, Args, Failure};
use serde_json::{json, Map, Value};

/// A CLI command. Each sends the protocol command of the same name.
pub struct Command {
    pub name: &'static str,
    pub group: &'static str,
    pub summary: &'static str,
    pub usage: &'static str,
    pub options: &'static [(&'static str, &'static str)],
    pub notes: &'static str,
    pub examples: &'static [(&'static str, &'static str)],
    /// Turns the command's arguments into a protocol request's `args`.
    pub parse: fn(&mut Args) -> Result<Value, Failure>,
    /// Prints a successful reply for people (`--json` prints it as is).
    pub show: fn(&Value) -> String,
}

pub const GROUPS: &[&str] = &["Panes", "Tabs and windows", "Information"];

pub static COMMANDS: &[Command] = &[
    Command {
        name: "open",
        group: "Panes",
        summary: "Open a terminal pane (or a web pane) and wait until it is running.",
        usage: "kmux open [term] [--cmd CMD] [--cwd DIR] [--name NAME] [--split right|down|auto] [--size FRACTION] [--tab] [--window ID|new] [--no-wait]\n       kmux open web URL [same placement options]",
        options: &[
            ("--cmd CMD", "Shell command line to run, e.g. \"npm run dev\". Default: an interactive shell."),
            ("--cwd DIR", "Working directory."),
            ("--name NAME", "A name to refer to the pane by in later commands, instead of its ID."),
            ("--split right|down|auto", "Where the new pane goes relative to the focused pane. auto (default) splits along the longer side."),
            ("--size FRACTION", "Share of the focused pane's space the new pane takes: 1/3, 25% or 0.25. Default 1/2."),
            ("--tab", "Open in a new tab instead of splitting."),
            ("--window ID|new", "Open in that window, or a new one. Default: the key (front) window."),
            ("--no-wait", "Return as soon as the pane exists instead of waiting until it is running."),
        ],
        notes: "Prints the new pane's ID and name. A pane whose command exits stays on screen as `exited`.",
        examples: &[
            ("kmux open --name server --cmd \"npm run dev\"", "Run a dev server in a pane named server."),
            ("kmux open --name logs --split right --size 1/3 --cmd \"tail -f app.log\"", "Put a log tail in the right third."),
            ("kmux open --tab", "Open a shell in a new tab."),
            ("kmux open --window new --name scratch", "Open a shell in a new window."),
        ],
        parse: parse_open,
        show: output::opened,
    },
    Command {
        name: "close",
        group: "Panes",
        summary: "Close a pane, or a whole tab or window with everything in it.",
        usage: "kmux close PANE\n       kmux close --tab TAB\n       kmux close --window WINDOW",
        options: &[("--tab TAB", "Close a tab, e.g. t2."), ("--window WINDOW", "Close a window, e.g. w2.")],
        notes: "Closing a tab's last pane closes the tab; closing a window's last tab closes the window.",
        examples: &[("kmux close logs", "Close the pane named logs."), ("kmux close --tab t2", "Close tab t2 and its panes.")],
        parse: |args| target(args, "close"),
        show: output::closed,
    },
    Command {
        name: "focus",
        group: "Panes",
        summary: "Bring a pane, tab or window to the front and give it the keyboard.",
        usage: "kmux focus PANE\n       kmux focus --tab TAB\n       kmux focus --window WINDOW",
        options: &[("--tab TAB", "Focus a tab."), ("--window WINDOW", "Focus a window.")],
        notes: "",
        examples: &[("kmux focus server", "Focus the pane named server.")],
        parse: |args| target(args, "focus"),
        show: |reply| format!("key window is now {}", reply["window"].as_str().unwrap_or("?")),
    },
    Command {
        name: "zoom",
        group: "Panes",
        summary: "Toggle zoom: the pane fills its tab until you zoom again or focus another pane.",
        usage: "kmux zoom PANE",
        options: &[],
        notes: "",
        examples: &[("kmux zoom logs", "Zoom the logs pane (run again to unzoom).")],
        parse: |args| Ok(json!({ "pane": pane(args, "zoom")? })),
        show: |reply| if reply["zoomed"] == json!(true) { "zoomed".into() } else { "unzoomed".into() },
    },
    Command {
        name: "restart",
        group: "Panes",
        summary: "Restart what runs in a pane, and wait until it is running again.",
        usage: "kmux restart PANE",
        options: &[],
        notes: "",
        examples: &[("kmux restart server", "Restart the server pane's command.")],
        parse: |args| Ok(json!({ "pane": pane(args, "restart")? })),
        show: output::pane_state,
    },
    Command {
        name: "send",
        group: "Panes",
        summary: "Type a line of text into a terminal pane and press Return.",
        usage: "kmux send PANE TEXT...",
        options: &[],
        notes: "Everything after PANE is the text. Quote it to keep your shell from interpreting it.",
        examples: &[("kmux send server \"npm test\"", "Run npm test in the server pane.")],
        parse: |args| {
            let pane = pane(args, "send")?;
            let text = args.rest().ok_or_else(|| usage("send", "missing TEXT"))?;
            Ok(json!({ "pane": pane, "text": text }))
        },
        show: |_| String::new(),
    },
    Command {
        name: "resize",
        group: "Panes",
        summary: "Change a pane's share of its split. Its siblings share the rest in proportion.",
        usage: "kmux resize PANE FRACTION",
        options: &[],
        notes: "FRACTION is 1/3, 25% or 0.25. Fails if the pane fills its tab.",
        examples: &[("kmux resize logs 1/4", "Make logs a quarter of its split.")],
        parse: |args| {
            let pane = pane(args, "resize")?;
            let size = args.positional().ok_or_else(|| usage("resize", "missing FRACTION"))?;
            Ok(json!({ "pane": pane, "size": size }))
        },
        show: output::layout,
    },
    Command {
        name: "move",
        group: "Panes",
        summary: "Move a pane next to another pane, swap two panes, or move a pane to another tab or window.",
        usage: "kmux move PANE --to OTHER [--side left|right|top|bottom|swap]\n       kmux move PANE --tab TAB|new [--window WINDOW]\n       kmux move PANE --window WINDOW|new",
        options: &[
            ("--to OTHER", "The pane to move next to."),
            ("--side SIDE", "left, right, top or bottom of OTHER (takes half its space), or swap (default)."),
            ("--tab TAB|new", "Move into that tab, or a new tab."),
            ("--window WINDOW|new", "Move into that window's active tab, or a new window."),
        ],
        notes: "",
        examples: &[
            ("kmux move logs --to server --side bottom", "Put logs below server."),
            ("kmux move logs --window new", "Move logs into a new window."),
        ],
        parse: |args| {
            let pane = pane(args, "move")?;
            let mut out = Map::new();
            out.insert("pane".into(), json!(pane));
            for (flag, key) in [("--to", "to"), ("--side", "side"), ("--tab", "tab"), ("--window", "window")] {
                if let Some(value) = args.option(flag)? {
                    out.insert(key.into(), json!(value));
                }
            }
            if !["to", "tab", "window"].iter().any(|k| out.contains_key(*k)) {
                return Err(usage("move", "say where: --to OTHER, --tab TAB|new or --window WINDOW|new"));
            }
            Ok(Value::Object(out))
        },
        show: output::layout,
    },
    Command {
        name: "arrange",
        group: "Tabs and windows",
        summary: "Lay out existing panes in one go. Panes left out move to a new tab.",
        usage: "kmux arrange LAYOUT [--window WINDOW]",
        options: &[("--window WINDOW", "Arrange in that window's active tab. Default: the key window.")],
        notes: "LAYOUT is a JSON tree: {\"split\": \"row\"|\"column\", \"children\": [...]} where each child is {\"pane\": NAME_OR_ID, \"size\": FRACTION?} or another split. Unsized children share what is left. row = side by side, column = stacked.",
        examples: &[(
            "kmux arrange '{\"split\":\"row\",\"children\":[{\"pane\":\"server\",\"size\":\"2/3\"},{\"pane\":\"logs\"}]}'",
            "server on the left two thirds, logs on the right.",
        )],
        parse: |args| {
            let window = args.option("--window")?;
            let text = args.positional().ok_or_else(|| usage("arrange", "missing LAYOUT"))?;
            let layout: Value = serde_json::from_str(&text).map_err(|e| usage("arrange", &format!("LAYOUT is not valid JSON: {e}")))?;
            let mut out = json!({ "layout": layout });
            if let Some(window) = window {
                out["window"] = json!(window);
            }
            Ok(out)
        },
        show: output::layout,
    },
    Command {
        name: "rename-tab",
        group: "Tabs and windows",
        summary: "Rename a tab. (In the app: double-click the tab.)",
        usage: "kmux rename-tab TAB TITLE...",
        options: &[],
        notes: "TAB is a tab ID such as t1 (see `kmux list`). Everything after TAB is the title.",
        examples: &[("kmux rename-tab t1 servers", "Call tab t1 \"servers\".")],
        parse: |args| {
            let tab = args.positional().ok_or_else(|| usage("rename-tab", "missing TAB (an ID like t1; see `kmux list`)"))?;
            let title = args.rest().ok_or_else(|| usage("rename-tab", "missing TITLE"))?;
            Ok(json!({ "tab": tab, "title": title }))
        },
        show: |reply| format!("{} is now \"{}\"", reply["tab"].as_str().unwrap_or("?"), reply["title"].as_str().unwrap_or("")),
    },
    Command {
        name: "move-tab",
        group: "Tabs and windows",
        summary: "Reorder a tab, or move it to another window or a new one.",
        usage: "kmux move-tab TAB [--window WINDOW|new] [--index N]",
        options: &[("--window WINDOW|new", "Destination window. Default: the tab's own window."), ("--index N", "Position in the tab bar, from 0. Default: last.")],
        notes: "",
        examples: &[("kmux move-tab t2 --window new", "Move tab t2 into a new window."), ("kmux move-tab t3 --index 0", "Make t3 the first tab.")],
        parse: |args| {
            let tab = args.positional().ok_or_else(|| usage("move-tab", "missing TAB"))?;
            let mut out = json!({ "tab": tab });
            if let Some(window) = args.option("--window")? {
                out["window"] = json!(window);
            }
            if let Some(index) = args.option("--index")? {
                let index: u64 = index.parse().map_err(|_| usage("move-tab", "--index needs a whole number"))?;
                out["index"] = json!(index);
            }
            Ok(out)
        },
        show: |reply| format!("{}: {}", reply["window"].as_str().unwrap_or("?"), output::join(&reply["tabs"])),
    },
    Command {
        name: "navigate",
        group: "Panes",
        summary: "Point a web pane at a new URL.",
        usage: "kmux navigate PANE URL",
        options: &[],
        notes: "",
        examples: &[("kmux navigate site localhost:5173", "Show localhost:5173 in the site pane.")],
        parse: |args| {
            let pane = pane(args, "navigate")?;
            let url = args.positional().ok_or_else(|| usage("navigate", "missing URL"))?;
            Ok(json!({ "pane": pane, "url": url }))
        },
        show: output::pane_state,
    },
    Command {
        name: "list",
        group: "Information",
        summary: "Show windows, tabs, layouts and panes, with their IDs.",
        usage: "kmux list",
        options: &[],
        notes: "Layouts are written as `a:2/3 | b` (side by side) and `a / b` (stacked). `*` marks the key window, the active tab and the focused pane.",
        examples: &[("kmux list", "See everything that is open."), ("kmux list --json", "The same, as JSON.")],
        parse: |_| Ok(json!({})),
        show: output::list,
    },
    Command {
        name: "capabilities",
        group: "Information",
        summary: "Show what the running kmux supports: pane types and commands.",
        usage: "kmux capabilities",
        options: &[],
        notes: "",
        examples: &[("kmux capabilities", "See which commands this kmux supports.")],
        parse: |_| Ok(json!({})),
        show: |reply| format!("pane types: {}\ncommands: {}", output::join(&reply["paneTypes"]), output::join(&reply["commands"])),
    },
];

pub fn find(name: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.name == name)
}

/// The closest command names, for "did you mean".
pub fn similar(name: &str) -> Vec<&'static str> {
    let mut names: Vec<(usize, &str)> =
        COMMANDS.iter().map(|c| (distance(name, c.name), c.name)).filter(|(d, n)| *d <= 2 || n.starts_with(name) || name.starts_with(n)).collect();
    names.sort();
    names.into_iter().map(|(_, n)| n).take(3).collect()
}

fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for j in 0..b.len() {
            let next = (row[j + 1] + 1).min(row[j] + 1).min(prev + usize::from(ca != b[j]));
            prev = row[j + 1];
            row[j + 1] = next;
        }
    }
    row[b.len()]
}

pub fn usage(command: &str, problem: &str) -> Failure {
    let usage = find(command).map(|c| c.usage).unwrap_or("");
    fail(exit::USAGE, format!("{command}: {problem}\nusage: {usage}\nmore: kmux help {command}"))
}

fn pane(args: &mut Args, command: &str) -> Result<String, Failure> {
    args.positional().ok_or_else(|| usage(command, "missing PANE (a pane name, or an ID like p1; see `kmux list`)"))
}

/// PANE, --tab TAB or --window WINDOW.
fn target(args: &mut Args, command: &str) -> Result<Value, Failure> {
    if let Some(tab) = args.option("--tab")? {
        return Ok(json!({ "tab": tab }));
    }
    if let Some(window) = args.option("--window")? {
        return Ok(json!({ "window": window }));
    }
    match args.positional() {
        Some(pane) => Ok(json!({ "pane": pane })),
        None => Err(usage(command, "say what: PANE, --tab TAB or --window WINDOW")),
    }
}

fn parse_open(args: &mut Args) -> Result<Value, Failure> {
    let mut out = Map::new();
    for (flag, key) in [("--cmd", "cmd"), ("--cwd", "cwd"), ("--name", "name"), ("--split", "split"), ("--size", "size"), ("--window", "window"), ("--app", "app"), ("--device", "device")] {
        if let Some(value) = args.option(flag)? {
            out.insert(key.into(), json!(value));
        }
    }
    if args.flag("--tab") {
        out.insert("tab".into(), json!(true));
    }
    if args.flag("--no-wait") {
        out.insert("wait".into(), json!(false));
    }
    let kind = match args.positional() {
        None => "term".to_string(),
        Some(kind) if ["term", "web", "ios"].contains(&kind.as_str()) => kind,
        Some(other) => {
            return Err(usage("open", &format!("unknown pane type \"{other}\" (term or web). To run a command use --cmd \"{other}\"")));
        }
    };
    if kind == "web" {
        let url = args.positional().ok_or_else(|| usage("open", "web panes need a URL: kmux open web URL"))?;
        out.insert("url".into(), json!(url));
    }
    out.insert("type".into(), json!(kind));
    Ok(Value::Object(out))
}
