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
        usage: "kanna open [term] [--cmd CMD] [--cwd DIR] [--name NAME] [--split right|down|auto] [--size FRACTION] [--tab] [--window ID|new] [--dock [left|right]] [--no-wait]\n       kanna open web URL [--history] [same placement options]\n       kanna open md FILE [same placement options]\n       kanna open ios --app APP [--device DEVICE] [same placement options]",
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
        notes: "Moving a docked pane to a tab, a window or beside a pane undocks it. A docked pane can't be split: --to it only swaps. A docked pane is a running task, so --dock moves only the pane that's docked already: start a task with `kanna task start`.",
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
        name: "rename-pane",
        group: "Panes",
        summary: "Give a pane a name, change it, or clear it.",
        usage: "kanna rename-pane PANE NAME",
        options: &[],
        notes: "PANE is the pane's current name or its ID (see `kanna list`). An empty NAME (\"\") clears the name, so the pane goes by its ID. No two panes share a name, and a name can't be another pane's ID.",
        examples: &[("kanna rename-pane p3 server", "Call pane p3 \"server\"."), ("kanna rename-pane server \"\"", "Clear the name; refer to it as p3 again.")],
        parse: |args| {
            let pane = args.positional().ok_or_else(|| usage("rename-pane", "missing PANE"))?;
            let name = args.positional().ok_or_else(|| usage("rename-pane", "missing NAME (\"\" to clear it)"))?;
            Ok(json!({ "pane": pane, "name": name }))
        },
        show: output::pane_renamed,
    },
    Command {
        name: "task",
        group: "Tabs and windows",
        summary: "Make, list, start, park, pull, name or end tasks: an agent docked in a Desk window with its tabs, or a task waiting in its repo's backlog.",
        usage: "kanna task new NAME [FIRST-MESSAGE] [--agent CMD]\n       kanna task start TASK [--window WINDOW]\n       kanna task list\n       kanna task park [TASK] [--window WINDOW]\n       kanna task pull TASK [--window WINDOW|new]\n       kanna task name [TASK] NAME\n       kanna task done [TASK]",
        options: &[
            ("--window WINDOW|new", "park: the Desk to clear (default: the task's, or the key window's). pull: the Desk to put it on (default: the key window if it's a Desk, else a new one). start: the Desk to put it on (default: the Shelf)."),
            ("--agent CMD", "new: the agent to start (default: claude)."),
        ],
        notes: "A task is a window's docked pane (usually an agent) and its tabs; TASK is that pane's name or ID. Parking moves the task's agent and tabs to the Shelf, still running, and leaves the Desk empty; pulling puts it back on a Desk as it was, parking whatever that Desk held. Pulling a task that's on another Desk brings that window forward instead. Done closes the task's panes, ending its agent and terminals; its Desk stays, empty. A task's name is its docked pane's name; `task name` sets it. Run from a task's terminal, `task park`, `task name` and `task done` mean that task, and `kanna open` with no placement opens a tab on its Desk.",
        examples: &[
            ("kanna task new \"Fix the S39 pinch\" \"The mark drifts when you pinch; find out why and add a test\"", "Add a task to this repo's backlog."),
            ("kanna task start t3", "Start a backlog task on the Shelf: its own worktree and branch, and its agent with the first message."),
            ("kanna task list", "Every task, and where it is: a Desk window, the Shelf or the backlog."),
            ("kanna task park", "Park this task (run from its terminal), or the key window's."),
            ("kanna task pull agent", "Put the task docked as agent back on this Desk."),
            ("kanna task name agent \"Tab bar light mode\"", "Name the task docked as agent; from its terminal, leave out agent."),
            ("kanna task done agent", "End the task docked as agent: close its agent and tabs."),
        ],
        parse: |args| {
            let action = args.positional().ok_or_else(|| usage("task", "say what: new, start, list, park, pull, name or done"))?;
            let window = args.option("--window")?;
            let agent = args.option("--agent")?;
            if agent.is_some() && action != "new" {
                return Err(usage("task", "--agent is for task new"));
            }
            let mut out = json!({ "action": action });
            match action.as_str() {
                "list" => {}
                "new" => {
                    out["name"] = json!(args.positional().ok_or_else(|| usage("task", "new needs a NAME: kanna task new NAME [FIRST-MESSAGE]"))?);
                    if let Some(message) = args.positional() {
                        out["message"] = json!(message);
                    }
                    out["repo"] = json!(repo_here()?);
                    if let Some(agent) = agent {
                        out["agent"] = json!(agent);
                    }
                    if let Some(by) = std::env::var("KANNA_PANE").ok().filter(|p| !p.is_empty()) {
                        out["by"] = json!(by);
                    }
                }
                "start" => out["task"] = json!(args.positional().ok_or_else(|| usage("task", "start needs a TASK from the backlog (see `kanna task list`)"))?),
                "park" => {
                    let task = args.positional().or_else(|| if window.is_none() { std::env::var("KANNA_PANE").ok().filter(|p| !p.is_empty()) } else { None });
                    if let Some(task) = task {
                        out["task"] = json!(task);
                    }
                }
                "pull" => out["task"] = json!(args.positional().ok_or_else(|| usage("task", "pull needs a TASK (see `kanna task list`)"))?),
                "name" => {
                    let first = args.positional().ok_or_else(|| usage("task", "name needs a NAME: kanna task name [TASK] NAME"))?;
                    let (task, name) = match args.positional() {
                        Some(name) => (first, name),
                        None => (std::env::var("KANNA_PANE").ok().filter(|p| !p.is_empty()).ok_or_else(|| usage("task", "say which task: kanna task name TASK NAME"))?, first),
                    };
                    out["pane"] = json!(task);
                    out["name"] = json!(name);
                }
                "done" => {
                    if window.is_some() {
                        return Err(usage("task", "done takes a TASK, not --window"));
                    }
                    let task = args.positional().or_else(|| std::env::var("KANNA_PANE").ok().filter(|p| !p.is_empty()));
                    out["task"] = json!(task.ok_or_else(|| usage("task", "done needs a TASK (see `kanna task list`)"))?);
                }
                other => return Err(usage("task", &format!("unknown action \"{other}\" (new, start, list, park, pull, name or done)"))),
            }
            if let Some(window) = window {
                out["window"] = json!(window);
            }
            Ok(out)
        },
        show: output::task,
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
        summary: "Open a markdown file or a local page to comment on, turn comments on or off in a pane, and answer them; the comments come to this pane.",
        usage: "kanna markup FILE.md|FILE.html [--to PANE] [--name NAME] [--split right|down|auto] [--size FRACTION] [--tab] [--window ID|new] [--dock [left|right]] [--no-wait]\n       kanna markup PANE [--to PANE | --off]\n       kanna markup comments [--all] [--doc FILE]\n       kanna markup show|resolve|reopen THREAD [--doc FILE]\n       kanna markup reply THREAD TEXT... [--doc FILE]",
        options: &[
            ("--to PANE", "The terminal comments go to, e.g. the pane an agent runs in. Default: this one."),
            ("--name NAME", "A name to refer to the pane by in later commands."),
            ("--split right|down|auto", "Where the pane goes relative to the focused pane, as for open."),
            ("--size FRACTION", "Share of the focused pane's space it takes, as for open."),
            ("--tab", "Open in a new tab instead of splitting."),
            ("--window ID|new", "Open in that window, or a new one."),
            ("--dock [left|right]", "Make it the window's docked pane, as for open."),
            ("--no-wait", "Return as soon as the pane exists."),
            ("--off", "Turn comments off in PANE: it shows the doc alone again. Its comments are kept."),
            ("--all", "comments: resolved ones too."),
            ("--doc FILE", "The doc a comment is on. Default: found from the comment's ID, the docs whose comments come to this pane first."),
        ],
        notes: "A markdown or web pane like `kanna open md FILE` or `kanna open web FILE`, with a thread list and a comment box: select text and press ⌘↵, ⌘-click a part, or ⌘-drag a box. Each comment is sent to the terminal that ran markup (or the one --to names), with the part or box and, for a page or an image, a screenshot with it marked up. `kanna open` shows the doc without comments; `kanna markup PANE` turns them on there.",
        examples: &[
            ("kanna markup references/mockups/desk.html --split right", "Review a mockup beside this pane."),
            ("kanna markup docs/specs/desk.md --tab", "Review a spec in a new tab."),
            ("kanna markup docs/specs/desk.md --to agent --tab", "Open a spec for comments in a new tab and send them to the pane named agent."),
            ("kanna markup spec", "Turn on comments in the pane named spec; they come to this pane."),
            ("kanna markup spec --to agent", "Send spec's comments to the pane named agent instead."),
            ("kanna markup spec --off", "Show spec without comments again."),
            ("kanna markup comments", "The open comments on the docs whose comments come to this pane."),
            ("kanna markup reply c4 \"Renamed it.\"", "Answer comment c4 beside the doc; leave it open for its author to read and resolve."),
        ],
        parse: parse_markup,
        show: output::marked,
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
    if let Some(action) = args.first().filter(|a| crate::threads::ACTIONS.contains(a)).map(String::from) {
        return parse_thread_action(args, &action);
    }
    let off = args.flag("--off");
    let to = args.option("--to")?;
    if off && to.is_some() {
        return Err(usage("markup", "--off sends comments nowhere, so it takes no --to"));
    }
    let target = args.positional().ok_or_else(|| usage("markup", "say what: kanna markup FILE.md, kanna markup FILE.html or kanna markup PANE"))?;
    let lower = target.to_lowercase();
    let agent = to.or_else(|| std::env::var("KANNA_PANE").ok().filter(|pane| !pane.is_empty()));
    let page = lower.ends_with(".html") || lower.ends_with(".htm");
    if !is_markdown(&target) && !page {
        if !out.is_empty() {
            return Err(usage("markup", &format!("{target} is a pane, so it stays where it is: no placement options")));
        }
        let mut pane = json!({ "pane": target });
        if off {
            pane["off"] = json!(true);
        } else if let Some(agent) = agent {
            pane["agent"] = json!(agent);
        }
        return Ok(pane);
    }
    if off {
        return Err(usage("markup", "--off is for a pane: kanna markup PANE --off"));
    }
    if page {
        out.insert("type".into(), json!("web"));
        out.insert("url".into(), json!(file_url(&absolute(&target))));
    } else {
        out.insert("type".into(), json!("md"));
        out.insert("path".into(), json!(absolute(&target)));
    }
    out.insert("markup".into(), json!(true));
    if let Some(agent) = agent {
        out.insert("agent".into(), json!(agent));
    }
    from_caller(&mut out);
    Ok(Value::Object(out))
}

pub fn is_backlog_task(task: &str) -> bool {
    task.strip_prefix('t').is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

fn repo_here() -> Result<String, Failure> {
    let here = std::env::current_dir().map_err(|e| usage("task", &format!("can't tell where you are: {e}")))?;
    let common = std::process::Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(&here)
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| std::path::PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()));
    match common {
        Some(git) if git.file_name().is_some_and(|n| n == ".git") => Ok(git.parent().unwrap_or(&git).to_string_lossy().into()),
        Some(git) => Ok(git.to_string_lossy().into()),
        None => Err(usage("task", "a task belongs to a git repo, and works in a worktree of it: run this in one, or set one up first (git init)")),
    }
}

fn from_caller(out: &mut Map<String, Value>) {
    if ["split", "size", "window", "dock", "tab"].iter().any(|k| out.contains_key(*k)) {
        return;
    }
    if let Some(pane) = std::env::var("KANNA_PANE").ok().filter(|pane| !pane.is_empty()) {
        out.insert("from".into(), json!(pane));
    }
}

fn parse_thread_action(args: &mut Argv, action: &str) -> Result<Value, Failure> {
    let all = args.flag("--all");
    let doc = args.option("--doc")?;
    args.positional();
    let mut out = json!({ "local": action });
    if let Some(doc) = doc {
        out["doc"] = json!(doc);
    }
    if action == "comments" {
        out["all"] = json!(all);
        return Ok(out);
    }
    let id = args.positional().ok_or_else(|| usage("markup", &format!("{action} which comment? e.g. kanna markup {action} c2")))?;
    out["id"] = json!(id);
    if action == "reply" {
        let text = args.rest().ok_or_else(|| usage("markup", "reply with what? e.g. kanna markup reply c2 \"Fixed.\""))?;
        out["text"] = json!(text);
    }
    Ok(out)
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
    from_caller(&mut out);
    Ok(Value::Object(out))
}
