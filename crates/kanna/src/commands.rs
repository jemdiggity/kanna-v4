use crate::args::Argv;
use crate::output;
use kanna_client::{exit, fail, Failure};
use serde_json::{json, Map, Value};

pub struct Command {
    pub name: &'static str,
    pub group: &'static str,
    pub summary: &'static str,
    pub usage: &'static str,
    pub options: &'static [(&'static str, &'static str)],
    pub notes: &'static str,
    pub examples: &'static [(&'static str, &'static str)],
    pub parse: fn(&mut Argv) -> Result<Value, Failure>,
    pub show: fn(&Value, &Value) -> String,
}

pub const GROUPS: &[&str] = &["Panes", "Tabs and windows", "Information"];

pub static COMMANDS: &[Command] = &[
    Command {
        name: "open",
        group: "Panes",
        summary: "Open a terminal, web, iOS or markdown pane and wait until it is running.",
        usage: "kanna open [term] [--cmd CMD] [--cwd DIR] [--name NAME] [--split right|down|auto] [--size FRACTION] [--tab] [--window ID|new] [--no-wait]\n       kanna open [TYPE …] --dock [left|right] [--window ID|new]\n       kanna open web URL [--history] [same placement options]\n       kanna open md FILE [same placement options]\n       kanna open ios --app APP [--device DEVICE] [same placement options]",
        options: &[
            ("--cmd CMD", "Shell command line to run in your login shell, e.g. \"npm run dev\". Put it in single quotes if it uses $VARS or ;, so your own shell leaves it alone. Default: an interactive shell."),
            ("--cwd DIR", "Working directory."),
            ("--name NAME", "Optional. A name to refer to the pane by in later commands; without one, use the ID that open prints (e.g. p3)."),
            ("--split right|down|auto", "Where the new pane goes relative to the focused pane. auto (default) splits along the longer side."),
            ("--size FRACTION", "Share of the focused pane's space the new pane takes: 1/3, 25% or 0.25. Default 1/2."),
            ("--tab", "Open in a new tab instead of splitting."),
            ("--window ID|new", "Open in that window, or a new one. Default: the key (front) window."),
            ("--no-wait", "Return as soon as the pane exists instead of waiting until it is running."),
            ("--app APP", "iOS panes: a .app built for the simulator (installed, then launched) or the bundle ID of an installed app, e.g. com.apple.Preferences."),
            ("--device DEVICE", "iOS panes: a simulator device name or UDID, e.g. \"iPhone 16\". Default: a booted iPhone, else the newest one. The device is booted if needed (this can take a minute) and stays booted after the pane closes."),
            ("--history", "Web panes only: keep back/forward history (off by default), for `kanna navigate PANE --back` and the Pane menu's Back and Forward."),
            ("--dock [left|right]", "Make the pane the window's docked pane: along its left (default) or right edge, outside its tabs, shown whichever tab is. Not with --split, --size or --tab. A pane already docked there moves to a new tab."),
        ],
        notes: "Prints the new pane's ID and name. A pane whose command exits stays on screen as `exited`. A docked pane closes only with its window.\nA terminal opened without --split, --size, --tab, --window or --dock opens even with Kanna.app closed, and takes its place when the app next opens; anything else starts the app.",
        examples: &[
            ("kanna open --name server --cmd \"npm run dev\"", "Run a dev server in a pane named server."),
            ("kanna open --name logs --split right --size 1/3 --cmd \"tail -f app.log\"", "Put a log tail in the right third."),
            ("kanna open --tab", "Open a shell in a new tab."),
            ("kanna open --name build --split down --size 1/4 --cmd 'for i in 1 2 3; do echo step $i; done'", "A quarter-height pane below, running a loop."),
            ("kanna open --window new --name scratch", "Open a shell in a new window."),
            ("kanna open ios --name phone --app build/MyApp.app --split right --size 1/3", "Run your app in the simulator, in the right third."),
            ("kanna open md docs/spec.md --name spec --split right", "Show a markdown file (with mermaid diagrams) beside this pane; it reloads when the file changes."),
            ("kanna open --name agent --cmd claude --dock", "Dock an agent on the window's left edge; it stays while you switch tabs."),
        ],
        parse: parse_open,
        show: output::opened,
    },
    Command {
        name: "close",
        group: "Panes",
        summary: "Close a pane, or a whole tab or window with everything in it.",
        usage: "kanna close PANE\n       kanna close --tab TAB\n       kanna close --window WINDOW",
        options: &[("--tab TAB", "Close a tab, e.g. t2."), ("--window WINDOW", "Close a window, e.g. w2.")],
        notes: "Closing a tab's last pane closes the tab; closing a window's last tab closes the window, unless it has a docked pane. A docked pane closes only with its window (`kanna close --window`); undock it first with `kanna move PANE --tab` to close it alone.",
        examples: &[("kanna close logs", "Close the pane named logs."), ("kanna close --tab t2", "Close tab t2 and its panes.")],
        parse: |args| target(args, "close"),
        show: output::closed,
    },
    Command {
        name: "focus",
        group: "Panes",
        summary: "Bring a pane, tab or window to the front and give it the keyboard.",
        usage: "kanna focus PANE\n       kanna focus --tab TAB\n       kanna focus --window WINDOW",
        options: &[("--tab TAB", "Focus a tab."), ("--window WINDOW", "Focus a window.")],
        notes: "Focusing a pane in another tab or window switches to it. Focusing another pane cancels zoom.",
        examples: &[("kanna focus server", "Focus the pane named server."), ("kanna focus --tab t1", "Switch to tab t1.")],
        parse: |args| target(args, "focus"),
        show: output::focused,
    },
    Command {
        name: "zoom",
        group: "Panes",
        summary: "Toggle zoom: the pane fills its tab until you zoom again or focus another pane.",
        usage: "kanna zoom PANE",
        options: &[],
        notes: "The pane doesn't need to be focused first: zooming focuses it, switching tab or window if needed.",
        examples: &[("kanna zoom logs", "Zoom the logs pane (run again to unzoom).")],
        parse: |args| Ok(json!({ "pane": pane(args, "zoom")? })),
        show: output::zoomed,
    },
    Command {
        name: "restart",
        group: "Panes",
        summary: "Restart what runs in a pane, and wait until it is running again.",
        usage: "kanna restart PANE",
        options: &[],
        notes: "",
        examples: &[("kanna restart server", "Restart the server pane's command.")],
        parse: |args| Ok(json!({ "pane": pane(args, "restart")? })),
        show: |_, reply| output::pane_state(reply),
    },
    Command {
        name: "send",
        group: "Panes",
        summary: "Type a line of text into a terminal pane and press Return.",
        usage: "kanna send PANE TEXT...",
        options: &[],
        notes: "Everything after PANE is the text. Quote it to keep your shell from interpreting it.\n-- ends the options: `kanna send p1 -- --json` types --json.",
        examples: &[("kanna send server \"npm test\"", "Run npm test in the server pane.")],
        parse: |args| {
            let pane = pane(args, "send")?;
            let text = args.rest().ok_or_else(|| usage("send", "missing TEXT"))?;
            Ok(json!({ "pane": pane, "text": text }))
        },
        show: |_, _| String::new(),
    },
    Command {
        name: "read",
        group: "Panes",
        summary: "Print a terminal pane's screen as text.",
        usage: "kanna read PANE [--lines N]",
        options: &[("--lines N", "The last N lines instead of the screen, counting scrollback.")],
        notes: "Terminal panes only. Works with Kanna.app closed: the daemon keeps every terminal's screen.",
        examples: &[("kanna read server", "What the server pane shows now."), ("kanna read server --lines 200", "Its last 200 lines, scrollback included.")],
        parse: |args| {
            let lines = match args.option("--lines")? {
                Some(n) => Some(n.parse::<u64>().ok().filter(|n| *n > 0).ok_or_else(|| usage("read", "--lines needs a whole number above 0"))?),
                None => None,
            };
            let mut out = json!({ "pane": pane(args, "read")? });
            if let Some(lines) = lines {
                out["lines"] = json!(lines);
            }
            Ok(out)
        },
        show: |_, reply| reply["text"].as_str().unwrap_or_default().to_string(),
    },
    Command {
        name: "resize",
        group: "Panes",
        summary: "Change a pane's share of its split. Its siblings share the rest in proportion.",
        usage: "kanna resize PANE FRACTION",
        options: &[],
        notes: "FRACTION is 1/3, 25% or 0.25. Fails if the pane fills its tab. On a docked pane, sets its width, a fraction of the window.",
        examples: &[("kanna resize logs 1/4", "Make logs a quarter of its split.")],
        parse: |args| {
            let pane = pane(args, "resize")?;
            let size = args.positional().ok_or_else(|| usage("resize", "missing FRACTION"))?;
            Ok(json!({ "pane": pane, "size": size }))
        },
        show: |_, reply| output::moved(reply),
    },
    Command {
        name: "move",
        group: "Panes",
        summary: "Move a pane next to another pane, swap two panes, move a pane to another tab or window, or dock it.",
        usage: "kanna move PANE --to OTHER [--side left|right|top|bottom|swap]\n       kanna move PANE --tab [TAB|new] [--window WINDOW]\n       kanna move PANE --window WINDOW|new\n       kanna move PANE --dock [left|right] [--window WINDOW|new]",
        options: &[
            ("--to OTHER", "The pane to move next to."),
            ("--side SIDE", "left, right, top or bottom of OTHER (takes half its space), or swap (default)."),
            ("--tab [TAB|new]", "Move into that tab, or a new tab (the default). Undocks a docked pane."),
            ("--window WINDOW|new", "Move into that window's active tab, or a new window."),
            ("--dock [left|right]", "Make it its window's docked pane (or that window's), on the left (default) or right edge; or move the docked pane to that edge. A pane already docked there moves to a new tab."),
        ],
        notes: "Moving a docked pane to a tab, a window or beside a pane undocks it. A docked pane can't be split: --to it only swaps.",
        examples: &[
            ("kanna move logs --to server --side bottom", "Put logs below server."),
            ("kanna move logs --window new", "Move logs into a new window."),
            ("kanna move agent --dock right", "Dock agent on the right edge (or move it there if it is docked)."),
            ("kanna move agent --tab", "Undock agent into a new tab."),
        ],
        parse: |args| {
            let pane = pane(args, "move")?;
            let mut out = Map::new();
            out.insert("pane".into(), json!(pane));
            for (flag, key) in [("--to", "to"), ("--side", "side"), ("--window", "window")] {
                if let Some(value) = args.option(flag)? {
                    out.insert(key.into(), json!(value));
                }
            }
            if let Some(tab) = args.optional("--tab", |v| !v.starts_with("--")) {
                out.insert("tab".into(), json!(tab.unwrap_or_else(|| "new".into())));
            }
            if let Some(edge) = args.optional("--dock", is_edge) {
                if out.contains_key("to") || out.contains_key("tab") {
                    return Err(usage("move", "--dock goes on its own (with --window at most)"));
                }
                out.insert("dock".into(), json!(edge.unwrap_or_else(|| "left".into())));
            }
            if !["to", "tab", "window", "dock"].iter().any(|k| out.contains_key(*k)) {
                return Err(usage("move", "say where: --to OTHER, --tab [TAB|new], --window WINDOW|new or --dock [left|right]"));
            }
            Ok(Value::Object(out))
        },
        show: |_, reply| output::moved(reply),
    },
    Command {
        name: "arrange",
        group: "Tabs and windows",
        summary: "Lay out existing panes in one go. Panes left out move to a new tab.",
        usage: "kanna arrange LAYOUT [--window WINDOW]",
        options: &[("--window WINDOW", "Arrange in that window's active tab. Default: the key window.")],
        notes: "Arranges the shown tab; a docked pane isn't part of it. LAYOUT is a JSON tree: {\"split\": \"row\"|\"column\", \"children\": [...]} where each child is {\"pane\": NAME_OR_ID, \"size\": FRACTION?} or another split. FRACTION is a string like \"2/3\" or \"25%\", or a number like 0.25. Unsized children share what is left. row = side by side, column = stacked.",
        examples: &[(
            "kanna arrange '{\"split\":\"row\",\"children\":[{\"pane\":\"server\",\"size\":\"2/3\"},{\"pane\":\"logs\"}]}'",
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
        show: |_, reply| output::layout(reply),
    },
    Command {
        name: "rename-tab",
        group: "Tabs and windows",
        summary: "Rename a tab. (In the app: double-click the tab.)",
        usage: "kanna rename-tab TAB TITLE...",
        options: &[],
        notes: "TAB is a tab ID such as t1 (see `kanna list`). Everything after TAB is the title.",
        examples: &[("kanna rename-tab t1 servers", "Call tab t1 \"servers\".")],
        parse: |args| {
            let tab = args.positional().ok_or_else(|| usage("rename-tab", "missing TAB (an ID like t1; see `kanna list`)"))?;
            let title = args.rest().ok_or_else(|| usage("rename-tab", "missing TITLE"))?;
            Ok(json!({ "tab": tab, "title": title }))
        },
        show: output::renamed,
    },
    Command {
        name: "move-tab",
        group: "Tabs and windows",
        summary: "Reorder a tab, or move it to another window or a new one.",
        usage: "kanna move-tab TAB [--window WINDOW|new] [--index N]",
        options: &[("--window WINDOW|new", "Destination window. Default: the tab's own window."), ("--index N", "Position in the tab bar, from 0. Default: last.")],
        notes: "",
        examples: &[("kanna move-tab t2 --window new", "Move tab t2 into a new window."), ("kanna move-tab t3 --index 0", "Make t3 the first tab.")],
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
        show: output::tabs,
    },
    Command {
        name: "navigate",
        group: "Panes",
        summary: "Point a web pane at a new URL (or go back or forward), or a markdown pane at another file.",
        usage: "kanna navigate PANE URL\n       kanna navigate PANE --back|--forward\n       kanna navigate PANE FILE.md",
        options: &[
            ("--back", "Go back a page: in a markdown pane, or a web pane opened with `open web URL --history`."),
            ("--forward", "Go forward a page (after --back)."),
        ],
        notes: "Markdown panes always keep history of the files they showed. Web panes keep none unless opened with --history.",
        examples: &[
            ("kanna navigate site localhost:5173", "Show localhost:5173 in the site pane."),
            ("kanna navigate site --back", "Go back to the previous page (the pane was opened with --history)."),
            ("kanna navigate spec docs/roadmap.md", "Show another markdown file in the spec pane."),
        ],
        parse: |args| {
            let pane = pane(args, "navigate")?;
            let (back, forward) = (args.flag("--back"), args.flag("--forward"));
            match (back, forward, args.positional()) {
                (true, false, None) => Ok(json!({ "pane": pane, "back": true })),
                (false, true, None) => Ok(json!({ "pane": pane, "forward": true })),
                (false, false, Some(target)) if is_markdown(&target) => Ok(json!({ "pane": pane, "path": absolute(&target) })),
                (false, false, Some(url)) => Ok(json!({ "pane": pane, "url": url })),
                (false, false, None) => Err(usage("navigate", "missing URL (or --back / --forward)")),
                _ => Err(usage("navigate", "give one of URL, --back or --forward")),
            }
        },
        show: |_, reply| output::pane_state(reply),
    },
    Command {
        name: "list",
        group: "Information",
        summary: "Show windows, tabs, layouts and panes, with their IDs.",
        usage: "kanna list",
        options: &[],
        notes: "Layouts are written as `a:2/3 | b` (side by side) and `a / b` (stacked). `*` marks the key window, the active tab and the focused pane. A window's docked pane has its own line: `dock: p1 (agent)  left 2/5`.\nWith Kanna.app closed there are no windows, and the terminals, which keep running, are listed alone.",
        examples: &[("kanna list", "See everything that is open."), ("kanna list --json", "The same, as JSON.")],
        parse: |_| Ok(json!({})),
        show: |_, reply| output::list(reply),
    },
    Command {
        name: "capabilities",
        group: "Information",
        summary: "Show what the running Kanna supports: pane types and commands.",
        usage: "kanna capabilities",
        options: &[],
        notes: "",
        examples: &[("kanna capabilities", "See which commands this Kanna supports.")],
        parse: |_| Ok(json!({})),
        show: |_, reply| output::capabilities(reply),
    },
    Command {
        name: "status",
        group: "Information",
        summary: "Explain an agent pane's status, or check the rules site for new status rules now.",
        usage: "kanna status explain PANE\n       kanna status explain --file SCREEN --agent AGENT\n       kanna status update",
        options: &[
            ("--file SCREEN", "A saved screen: text (as `kanna read` prints it) or a corpus case (JSON with rows, title, progress and cliVersion)."),
            ("--agent AGENT", "Whose rules judge the saved screen: claude, codex, …"),
        ],
        notes: "An agent pane's status (working, needs-you, idle, exited) is in `kanna list`. explain shows the agent and its version, the rules file and where it came from, the rule that matched and the text it matched.\nRules: bundled in Kanna, from the rules site (checked every 30 minutes), or your own in ~/.config/kanna/agent-rules/<agent>.json, which always wins and reloads when you save it.",
        examples: &[
            ("kanna status explain p3", "Why p3's agent shows the status it does."),
            ("kanna status explain --file screen.txt --agent claude", "How Claude's rules judge a saved screen."),
            ("kanna status update", "Fetch new rules from the rules site now."),
        ],
        parse: parse_status,
        show: output::status,
    },
    Command {
        name: "markup",
        group: "Panes",
        summary: "Open a local page (a mockup) in a web pane you can comment on; the comments come to this pane.",
        usage: "kanna markup FILE.html [--name NAME] [--split right|down|auto] [--size FRACTION] [--tab] [--window ID|new] [--dock [left|right]] [--no-wait]",
        options: &[
            ("--name NAME", "A name to refer to the pane by in later commands."),
            ("--split right|down|auto", "Where the pane goes relative to the focused pane, as for open."),
            ("--size FRACTION", "Share of the focused pane's space it takes, as for open."),
            ("--tab", "Open in a new tab instead of splitting."),
            ("--window ID|new", "Open in that window, or a new one."),
            ("--dock [left|right]", "Make it the window's docked pane, as for open."),
            ("--no-wait", "Return as soon as the pane exists."),
        ],
        notes: "A web pane like `kanna open web FILE`, with a thread list and a comment box: ⌘-click a part of the page or ⌘-drag a box to comment. Each comment is typed into the terminal that ran markup, with the part or box and a screenshot with it marked up. `kanna open web` shows the page without comments.",
        examples: &[
            ("kanna markup docs/mockups/desk.html --split right", "Review a mockup beside this pane."),
            ("kanna markup docs/mockups/desk.html --tab", "The same, in a new tab."),
        ],
        parse: parse_markup,
        show: output::opened,
    },
];

fn parse_markup(args: &mut Argv) -> Result<Value, Failure> {
    let mut out = Map::new();
    for (flag, key) in [("--name", "name"), ("--split", "split"), ("--size", "size"), ("--window", "window")] {
        if let Some(value) = args.option(flag)? {
            out.insert(key.into(), json!(value));
        }
    }
    if let Some(edge) = args.optional("--dock", is_edge) {
        out.insert("dock".into(), json!(edge.unwrap_or_else(|| "left".into())));
    }
    if args.flag("--tab") {
        out.insert("tab".into(), json!(true));
    }
    if out.contains_key("dock") && ["split", "size", "tab"].iter().any(|k| out.contains_key(*k)) {
        return Err(usage("markup", "--dock goes on its own: no --split, --size or --tab"));
    }
    if args.flag("--no-wait") {
        out.insert("wait".into(), json!(false));
    }
    let file = args.positional().ok_or_else(|| usage("markup", "say which page: kanna markup FILE.html"))?;
    let lower = file.to_lowercase();
    if !(lower.ends_with(".html") || lower.ends_with(".htm")) {
        return Err(usage("markup", &format!("{file} is not an HTML page (.html or .htm)")));
    }
    out.insert("type".into(), json!("web"));
    out.insert("url".into(), json!(file_url(&absolute(&file))));
    out.insert("markup".into(), json!(true));
    if let Some(agent) = std::env::var("KANNA_PANE").ok().filter(|pane| !pane.is_empty()) {
        out.insert("agent".into(), json!(agent));
    }
    Ok(Value::Object(out))
}

fn parse_status(args: &mut Argv) -> Result<Value, Failure> {
    let file = args.option("--file")?;
    let agent = args.option("--agent")?;
    match args.positional().as_deref() {
        Some("update") => Ok(json!({ "action": "update" })),
        Some("explain") => match (file, agent) {
            (Some(file), Some(agent)) => Ok(json!({ "action": "explain", "agent": agent, "file": absolute(&file) })),
            (Some(_), None) => Err(usage("status", "a saved screen needs --agent (claude, codex, …)")),
            (None, _) => Ok(json!({ "action": "explain", "pane": pane(args, "status")? })),
        },
        Some(other) => Err(usage("status", &format!("unknown action \"{other}\" (explain or update)"))),
        None => Err(usage("status", "say what: explain PANE, explain --file SCREEN --agent AGENT, or update")),
    }
}

pub fn find(name: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.name == name)
}

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
    fail(exit::USAGE, format!("{command}: {problem}\nusage: {usage}\nmore: kanna help {command}"))
}

fn pane(args: &mut Argv, command: &str) -> Result<String, Failure> {
    args.positional().ok_or_else(|| usage(command, "missing PANE (a pane name, or an ID like p1; see `kanna list`)"))
}

fn target(args: &mut Argv, command: &str) -> Result<Value, Failure> {
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

fn is_edge(value: &str) -> bool {
    value == "left" || value == "right"
}

fn is_markdown(target: &str) -> bool {
    let lower = target.to_lowercase();
    [".md", ".markdown", ".mdown"].iter().any(|ext| lower.ends_with(ext)) && !lower.contains("://")
}

fn local_page(target: &str) -> Option<String> {
    if target.contains("://") {
        return None;
    }
    let path = absolute(target);
    std::path::Path::new(&path).is_file().then(|| file_url(&path))
}

fn file_url(path: &str) -> String {
    let encoded: String = path
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect();
    format!("file://{encoded}")
}

pub fn absolute(path: &str) -> String {
    let expanded = match path.strip_prefix("~/") {
        Some(rest) => match std::env::var("HOME") {
            Ok(home) if !home.is_empty() => format!("{}/{rest}", home.trim_end_matches('/')),
            _ => path.to_string(),
        },
        None => path.to_string(),
    };
    let path = std::path::Path::new(&expanded);
    if path.is_absolute() || path.starts_with("~") {
        return expanded;
    }
    std::env::current_dir()
        .map(|dir| dir.join(path))
        .ok()
        .and_then(|p| std::fs::canonicalize(&p).ok().or(Some(p)))
        .map(|p| p.to_string_lossy().into())
        .unwrap_or(expanded)
}

fn parse_open(args: &mut Argv) -> Result<Value, Failure> {
    let mut out = Map::new();
    for (flag, key) in [("--cmd", "cmd"), ("--cwd", "cwd"), ("--name", "name"), ("--split", "split"), ("--size", "size"), ("--window", "window"), ("--app", "app"), ("--device", "device")] {
        if let Some(value) = args.option(flag)? {
            out.insert(key.into(), json!(value));
        }
    }
    if let Some(Value::String(cwd)) = out.get("cwd") {
        let cwd = absolute(cwd);
        out.insert("cwd".into(), json!(cwd));
    }
    if let Some(Value::String(app)) = out.get("app") {
        let path = std::path::Path::new(app);
        if path.is_relative() && path.exists() {
            if let Ok(full) = std::fs::canonicalize(path) {
                out.insert("app".into(), json!(full.to_string_lossy()));
            }
        }
    }
    if let Some(edge) = args.optional("--dock", is_edge) {
        out.insert("dock".into(), json!(edge.unwrap_or_else(|| "left".into())));
    }
    if args.flag("--tab") {
        out.insert("tab".into(), json!(true));
    }
    if out.contains_key("dock") && ["split", "size", "tab"].iter().any(|k| out.contains_key(*k)) {
        return Err(usage("open", "--dock goes on its own: no --split, --size or --tab"));
    }
    if args.flag("--no-wait") {
        out.insert("wait".into(), json!(false));
    }
    let history = args.flag("--history");
    let kind = match args.positional() {
        None => "term".to_string(),
        Some(kind) if ["term", "web", "ios", "md"].contains(&kind.as_str()) => kind,
        Some(other) if is_markdown(&other) => {
            return Err(usage("open", &format!("to show a markdown file use: kanna open md {other}")));
        }
        Some(other) => {
            return Err(usage("open", &format!("unknown pane type \"{other}\" (term, web, ios or md). To run a command use --cmd \"{other}\"")));
        }
    };
    if kind == "web" {
        let url = args.positional().ok_or_else(|| usage("open", "web panes need a URL: kanna open web URL"))?;
        out.insert("url".into(), json!(local_page(&url).unwrap_or(url)));
    }
    if kind == "md" {
        let file = args.positional().ok_or_else(|| usage("open", "markdown panes need a file: kanna open md FILE"))?;
        out.insert("path".into(), json!(absolute(&file)));
        if let Some(agent) = std::env::var("KANNA_PANE").ok().filter(|pane| !pane.is_empty()) {
            out.insert("agent".into(), json!(agent));
        }
    }
    if kind == "ios" && !out.contains_key("app") {
        return Err(usage("open", "iOS panes need an app: kanna open ios --app MyApp.app (or a bundle ID)"));
    }
    if kind != "ios" && (out.contains_key("app") || out.contains_key("device")) {
        return Err(usage("open", "--app and --device are only for iOS panes: kanna open ios --app APP"));
    }
    if history {
        if kind != "web" {
            return Err(usage("open", "--history is only for web panes: kanna open web URL --history"));
        }
        out.insert("history".into(), json!(true));
    }
    out.insert("type".into(), json!(kind));
    Ok(Value::Object(out))
}
