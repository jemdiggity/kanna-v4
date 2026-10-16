mod support;

use serde_json::json;
use support::{cli, serve, wait_for, Daemon, Run};

fn kanna_at(dir: &std::path::Path, socket: Option<&std::path::Path>, args: &[&str]) -> Run {
    let mut command = cli(dir);
    if let Some(socket) = socket {
        command.env("KANNA_SOCKET", socket);
    }
    Run::from(args, command.args(args).output().unwrap())
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::path::PathBuf::from("/tmp").join(format!("kcli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("home")).unwrap();
    dir.canonicalize().unwrap()
}

fn layout(daemon: &Daemon, window: usize, tab: usize) -> String {
    let list = daemon.kanna(&["list", "--json"]).json();
    let node = &list["windows"][window]["tabs"][tab]["layout"];
    kanna::output::expression(node, true)
}

#[test]
fn a_code_pane_opens_a_file_at_a_line_and_moves_between_lines() {
    let daemon = Daemon::start();
    let _app = daemon.app();
    daemon.kanna(&["open", "--name", "a", "--cmd", "exec /bin/sh"]).ok();
    let source = daemon.dir.join("main.rs");
    std::fs::write(&source, "fn main() {}\n").unwrap();
    let opened = daemon.kanna(&["open", "code", "main.rs:12", "--name", "src", "--tab"]).ok();
    assert_eq!(opened, "opened p2 (src) in window w1, tab t2: running");
    let listed = daemon.kanna(&["list", "--json"]).json()["panes"][1].clone();
    assert_eq!((listed["type"].clone(), listed["path"].clone(), listed["line"].clone()), (json!("code"), json!(source.to_string_lossy()), json!(12)));
    daemon.kanna(&["navigate", "src", &format!("{}:40", source.display())]).ok();
    assert_eq!(daemon.kanna(&["list", "--json"]).json()["panes"][1]["line"], json!(40));
    daemon.kanna(&["navigate", "src", "--back"]).ok();
    assert_eq!(daemon.kanna(&["list", "--json"]).json()["panes"][1]["line"], json!(12));
    daemon.kanna(&["focus", "a"]).ok();
    assert_eq!(daemon.kanna(&["open", "code", "main.rs:30"]).ok(), "opened p2 (src) in window w1, tab t2: running");
    assert!(daemon.kanna(&["list"]).ok().starts_with("*w1  focused: src"));
    assert_eq!(daemon.kanna(&["open", "code", "main.rs"]).ok(), "opened p2 (src) in window w1, tab t2: running");
    let panes = daemon.kanna(&["list", "--json"]).json()["panes"].clone();
    assert_eq!((panes.as_array().unwrap().len(), panes[1]["line"].clone()), (2, json!(30)));
    assert_eq!(daemon.kanna(&["open", "code", "main.rs", "--tab"]).ok(), "opened p3 in window w1, tab t3: running");
    assert_eq!(daemon.kanna(&["close", "p3"]).ok(), "closed p3");
    assert_eq!(daemon.kanna(&["close", "src"]).ok(), "closed p2");
    daemon.kanna(&["open", "code"]).fails(2, "open: code panes need a file: kanna open code FILE[:LINE]");
    daemon.kanna(&["raw", r#"{"cmd":"open","args":{"type":"code"}}"#]).fails(2, "code panes need a path");
    daemon.kanna(&["raw", r#"{"cmd":"open","args":{"type":"md","path":"/a.md","line":3}}"#]).fails(2, "line is only for code panes");
}

#[test]
fn every_command_works_through_the_daemon_and_the_app() {
    let daemon = Daemon::start();
    let _app = daemon.app();
    assert_eq!(daemon.kanna(&["open", "--name", "a", "--cmd", "exec /bin/sh"]).ok(), "opened p1 (a) in window w1, tab t1: running");
    assert_eq!(daemon.kanna(&["open", "--name", "b", "--split", "right", "--size", "1/3"]).ok(), "opened p2 (b) in window w1, tab t1: running");
    assert_eq!(layout(&daemon, 0, 0), "a:2/3 | b:1/3");
    let list = daemon.kanna(&["list"]).ok();
    assert!(list.starts_with("*w1  focused: b\n  *t1 \"Tab 1\"  a:2/3 | b:1/3\n\nPANE  NAME  TYPE  STATE    COMMAND"), "{list}");
    assert!(list.contains("p1    a     term  running  exec /bin/sh"), "{list}");
    assert!(list.contains("p2    b     term  running  (shell)"), "{list}");

    assert_eq!(daemon.kanna(&["focus", "a"]).ok(), "focused a (key window w1)");
    assert_eq!(daemon.kanna(&["zoom", "b"]).ok(), "b is zoomed (run again to unzoom)");
    assert!(daemon.kanna(&["list"]).ok().contains("zoomed: b"));
    assert_eq!(daemon.kanna(&["zoom", "b"]).ok(), "b is no longer zoomed");
    assert_eq!(daemon.kanna(&["resize", "b", "1/4"]).ok(), "a:3/4 | b:1/4");
    assert_eq!(daemon.kanna(&["move", "b", "--to", "a", "--side", "bottom"]).ok(), "a:1/2 / b:1/2");
    assert_eq!(daemon.kanna(&["move", "b", "--to", "a"]).ok(), "b:1/2 / a:1/2");
    let arranged = daemon.kanna(&["arrange", r#"{"split":"row","children":[{"pane":"a","size":"2/3"},{"pane":"b"}]}"#]).ok();
    assert_eq!(arranged, "a:2/3 | b:1/3");

    assert_eq!(daemon.kanna(&["open", "--tab", "--name", "c"]).ok(), "opened p3 (c) in window w1, tab t2: running");
    assert_eq!(daemon.kanna(&["rename-tab", "t2", "my", "servers"]).ok(), "t2 is now \"my servers\"");
    assert_eq!(daemon.kanna(&["rename-pane", "c", "logs"]).ok(), "p3 is now \"logs\"");
    assert!(daemon.kanna(&["list"]).ok().contains("p3    logs"), "{}", daemon.kanna(&["list"]).ok());
    daemon.kanna(&["rename-pane", "logs", "a"]).fails(1, "a pane named \"a\" already exists");
    daemon.kanna(&["rename-pane", "logs", "p1"]).fails(1, "a pane named \"p1\" already exists");
    assert_eq!(daemon.kanna(&["rename-pane", "logs", ""]).ok(), "p3 has no name");
    assert_eq!(daemon.kanna(&["rename-pane", "p3", "c"]).ok(), "p3 is now \"c\"");
    assert_eq!(daemon.kanna(&["move-tab", "t2", "--index", "0"]).ok(), "w1: t2, t1");
    assert_eq!(daemon.kanna(&["focus", "--tab", "t1"]).ok(), "switched to t1 (key window w1)");
    let opened = daemon.kanna(&["open", "--window", "new", "--name", "d", "--no-wait"]).ok();
    assert!(opened.starts_with("opened p4 (d) in window w2, tab t3: "), "{opened}");
    assert_eq!(daemon.kanna(&["focus", "--window", "w1"]).ok(), "key window is now w1");
    assert_eq!(daemon.kanna(&["move", "c", "--window", "w2"]).ok(), "d:1/2 | c:1/2");
    assert_eq!(daemon.kanna(&["move", "c", "--tab", "new", "--window", "w2"]).ok(), "c");
    assert_eq!(daemon.kanna(&["move-tab", "t4", "--window", "new"]).ok(), "w3: t4");

    daemon.kanna(&["send", "a", "echo", "SENT-$((40+2))"]).ok();
    wait_for("the sent line", || daemon.kanna(&["read", "a"]).ok().contains("SENT-42"));
    assert!(daemon.kanna(&["read", "a", "--lines", "3"]).ok().lines().count() <= 3);
    daemon.kanna(&["send", "a", "--", "--json"]).ok();
    wait_for("--json typed", || daemon.kanna(&["read", "a"]).ok().contains("--json"));
    assert_eq!(daemon.kanna(&["restart", "a"]).ok(), "p1 (a) (exec /bin/sh) is running");

    assert_eq!(daemon.kanna(&["focus", "b"]).ok(), "focused b (key window w1)");
    let page = daemon.dir.join("page.html");
    std::fs::write(&page, "<p>hi</p>").unwrap();
    let url = format!("file://{}", page.display());
    assert_eq!(daemon.kanna(&["open", "web", &url, "--name", "site", "--history", "--split", "down"]).ok(), "opened p5 (site) in window w1, tab t1: running");
    assert_eq!(daemon.kanna(&["navigate", "site", "file:///tmp/other.html"]).ok(), "p5 (site) (file:///tmp/other.html) is running");
    assert_eq!(daemon.kanna(&["navigate", "site", "--back"]).ok(), format!("p5 (site) ({url}) is running"));
    assert_eq!(daemon.kanna(&["navigate", "site", "--forward"]).ok(), "p5 (site) (file:///tmp/other.html) is running");
    let doc = daemon.dir.join("doc.md");
    std::fs::write(&doc, "# hi").unwrap();
    let opened = daemon.kanna(&["open", "md", "doc.md", "--name", "doc", "--tab"]).ok();
    assert_eq!(opened, "opened p6 (doc) in window w1, tab t5: running");
    assert_eq!(daemon.kanna(&["list", "--json"]).json()["panes"][5]["path"], json!(doc.to_string_lossy()));
    assert!(daemon.kanna(&["list", "--json"]).json()["panes"][5].get("markup").is_none());
    assert_eq!(daemon.kanna(&["markup", "doc"]).ok(), "comments on in p6 (doc)");
    assert_eq!(daemon.kanna(&["list", "--json"]).json()["panes"][5]["markup"], json!(true));
    assert_eq!(daemon.kanna(&["markup", "doc", "--to", "p1"]).ok(), "comments on in p6 (doc), sent to p1");
    assert_eq!(daemon.kanna(&["markup", "doc", "--off"]).ok(), "comments off in p6 (doc)");

    let capabilities = daemon.kanna(&["capabilities"]).ok();
    assert!(capabilities.starts_with("pane types: term, web, ios, md, code\ncommands: agents, arrange, backlog, capabilities, close, comment, focus, list, markup, move, move-tab, navigate, open, quiz, read, rename-pane, rename-tab, resize, restart, screenshot, send, send-key, status, task, zoom"), "{capabilities}");
    assert!(capabilities.ends_with("Kanna.app: attached"), "{capabilities}");
    let raw = daemon.kanna(&["raw", r#"{"cmd":"debug.anything","args":{"pane":"a"}}"#]).json();
    assert_eq!(raw, json!({ "cmd": "debug.anything", "args": { "pane": "p1" } }));
    let shot = daemon.kanna(&["screenshot", "a", "-o", "shot.png", "--force", "--json"]).json();
    assert_eq!(shot, json!({ "cmd": "screenshot", "args": { "pane": "p1", "path": daemon.dir.join("shot.png").to_string_lossy(), "force": true } }));
    assert_eq!(daemon.kanna(&["screenshot", "--window", "w1", "--json"]).json()["args"], json!({ "window": "w1", "dir": std::env::temp_dir().to_string_lossy() }));
    daemon.kanna(&["screenshot", "a", "--window", "w1"]).fails(2, "give PANE or --window WINDOW, not both");
    assert_eq!(daemon.kanna(&["raw", r#"{"cmd":"list"}"#]).json()["panes"].as_array().unwrap().len(), 6);

    assert_eq!(daemon.kanna(&["close", "--window", "w3"]).ok(), "closed window w3 (panes p3)");
    assert_eq!(daemon.kanna(&["close", "--tab", "t5"]).ok(), "closed tab t5 (panes p6)");
    assert_eq!(daemon.kanna(&["close", "site"]).ok(), "closed p5");
    let reply = daemon.kanna(&["--json", "close", "d"]).json();
    assert_eq!(reply, json!({ "closed": ["p4"] }));
    assert_eq!(layout(&daemon, 0, 0), "a:2/3 | b:1/3");

    let opened = daemon.kanna(&["open", "--window", "new", "--dock", "--name", "agent", "--no-wait"]).ok();
    assert!(opened.starts_with("opened p7 (agent) docked left in window w4"), "{opened}");
    daemon.kanna(&["open", "--window", "w4", "--name", "notes", "--no-wait"]).ok();
    assert_eq!(daemon.kanna(&["task", "list"]).ok(), "TASK  NAME   WHERE    STATE\np7    agent  Desk w4  running");
    assert_eq!(daemon.kanna(&["task", "park", "notes"]).ok(), "parked p7; w4 is an empty Desk");
    let listed = daemon.kanna(&["task", "list"]).ok();
    assert_eq!(listed, "TASK  NAME   WHERE  STATE\np7    agent  Shelf  running  parked just now", "{listed}");
    assert_eq!(daemon.kanna(&["task", "list", "AGE"]).ok(), listed);
    assert_eq!(daemon.kanna(&["task", "list", "agent", "notes"]).ok(), "no task matches \"agent notes\"");
    assert_eq!(daemon.kanna(&["--json", "task", "list", "nothing"]).json()["tasks"], json!([]));
    assert_eq!(daemon.kanna(&["task", "park", "agent"]).ok(), "p7 is already on the Shelf");
    assert_eq!(daemon.kanna(&["task", "pull", "agent", "--window", "w4"]).ok(), "pulled p7 onto w4");
    assert_eq!(daemon.kanna(&["focus", "--window", "w1"]).ok(), "key window is now w1");
    assert_eq!(daemon.kanna(&["task", "pull", "agent"]).ok(), "p7 is on w4; brought it forward");
    daemon.kanna(&["task", "pull"]).fails(2, "task: pull needs a TASK (see `kanna task list`)");
    daemon.kanna(&["task", "pull", "a"]).fails(4, "a is not in a task: its window has no docked pane");
}

#[test]
fn errors_exit_as_kmux_says() {
    let daemon = Daemon::start();
    let _app = daemon.app();
    daemon.kanna(&["open", "--name", "a"]).ok();
    daemon.kanna(&["close", "nope"]).fails(4, "no pane \"nope\" [not_found]\nRun `kanna list` to see pane names and IDs");
    daemon.kanna(&["open", "--name", "a"]).fails(1, "[");
    daemon.kanna(&["zoom"]).fails(2, "zoom: missing PANE");
    let err = daemon.kanna(&["resize", "a", "2"]).fails(2, "more: kanna help resize");
    assert!(err.contains("\nusage: kanna resize PANE FRACTION\n"), "{err}");
    daemon.kanna(&["raw", r#"{"cmd":"nope"}"#]).fails(5, "unknown command \"nope\" [bad_request]");
    daemon.kanna(&["raw", "{"]).fails(2, "raw: REQUEST is not valid JSON");
    daemon.kanna(&["raw", "{}"]).fails(2, "raw: REQUEST needs a \"cmd\"");
    daemon.kanna(&["read", "nope"]).fails(4, "not_found");
    daemon.kanna(&["lsit"]).fails(2, "unknown command \"lsit\". Did you mean: list? Run `kanna --help` for all commands.");
    daemon.kanna(&["zoom", "a", "b"]).fails(2, "zoom: unexpected argument \"b\"");
    daemon.kanna(&["--bogus"]).fails(2, "unknown option \"--bogus\"\n\nkanna — control the Kanna");
    daemon.kanna(&["--instance", "a/b", "list"]).fails(2, "bad instance name \"a/b\"");
    daemon.kanna(&["--mux", "tmux", "list"]).fails(5, "the tmux backend isn't supported yet");
}

const BYTES: &str = r#"stty raw -echo; printf '\033[?1hREADY\r\n'; while :; do printf '%s ' "$(dd bs=1 count=1 2>/dev/null | od -An -tx1 | tr -d ' \n')"; done"#;

#[test]
fn send_key_presses_keys_as_the_program_expects_them_without_the_app() {
    let daemon = Daemon::start();
    daemon.kanna(&["open", "--name", "bytes", "--cmd", BYTES]).ok();
    wait_for("the program", || daemon.kanna(&["read", "bytes"]).ok().contains("READY"));
    assert_eq!(daemon.kanna(&["send-key", "bytes", "up", "ctrl+c", "return", "shift+tab", "A", "esc"]).ok(), "");
    assert!(daemon.kanna(&["read", "bytes"]).ok().contains("1b 4f 41 03 0d 1b 5b 5a 41 1b"), "{}", daemon.kanna(&["read", "bytes"]).ok());
    daemon.kanna(&["send-key", "bytes", "cmd+c"]).fails(2, "cmd+c only means something to Kanna.app, not to the program in bytes (p1)");
    daemon.kanna(&["send-key", "bytes", "up", "hello"]).fails(2, "\"hello\" isn't a key; to type text, use kanna send");
    daemon.kanna(&["send-key", "bytes", "hyper+a"]).fails(2, "hyper+a: modifiers are ctrl, alt and shift");
    daemon.kanna(&["send-key", "nope", "return"]).fails(4, "not_found");
    daemon.kanna(&["raw", r#"{"cmd":"send-key","args":{"pane":"bytes","keys":[]}}"#]).fails(2, "missing keys");
    daemon.kanna(&["open", "--name", "shell"]).ok();
    daemon.kanna(&["send", "shell", "sleep 30"]).ok();
    std::thread::sleep(std::time::Duration::from_millis(300));
    daemon.kanna(&["send-key", "shell", "ctrl+c"]).ok();
    daemon.kanna(&["send", "shell", "echo AFTER-$((6*7))"]).ok();
    wait_for("the interrupted shell", || daemon.kanna(&["read", "shell"]).ok().contains("AFTER-42"));
    assert!(daemon.kanna(&["read", "shell"]).ok().contains("sleep 30\n^C\n"));
}

#[test]
fn without_the_app_terminals_work_and_the_rest_needs_it() {
    let daemon = Daemon::start();
    assert_eq!(daemon.kanna(&["open", "--name", "a"]).ok(), "opened p1 (a), unplaced until Kanna.app opens: running");
    let list = daemon.kanna(&["list"]).ok();
    assert!(list.starts_with("no windows: Kanna.app isn't open (these terminals keep running)\n\nPANE"), "{list}");
    daemon.kanna(&["send", "a", "echo", "ALONE"]).ok();
    wait_for("the sent line", || daemon.kanna(&["read", "a"]).ok().contains("ALONE"));
    assert_eq!(daemon.kanna(&["restart", "a"]).ok(), "p1 (a) is running");
    assert!(daemon.kanna(&["capabilities"]).ok().ends_with("Kanna.app: not attached (terminals work without it; the CLI starts it for commands that need it)"));
    assert_eq!(daemon.kanna(&["raw", r#"{"cmd":"list"}"#]).json()["windows"], json!([]));
    daemon.kanna(&["open", "--name", "b"]).ok();
    for args in [
        &["open", "--split", "right"][..],
        &["open", "--tab"],
        &["open", "--window", "new"],
        &["open", "--size", "1/3"],
        &["open", "web", "http://x.test"],
        &["open", "md", "/tmp/x.md"],
        &["open", "ios", "--app", "com.apple.Preferences"],
        &["focus", "a"],
        &["focus", "--tab", "t1"],
        &["focus", "--window", "w1"],
        &["zoom", "a"],
        &["resize", "a", "1/2"],
        &["move", "a", "--to", "b"],
        &["arrange", r#"{"split":"row","children":[{"pane":"a"},{"pane":"b"}]}"#],
        &["rename-tab", "t1", "x"],
        &["move-tab", "t1", "--index", "0"],
        &["close", "--tab", "t1"],
        &["close", "--window", "w1"],
        &["navigate", "site", "http://x.test"],
        &["raw", r#"{"cmd":"debug.app"}"#],
    ] {
        daemon.kanna(args).fails(6, "[no_app] (KANNA_SOCKET is set, so kanna doesn't start Kanna.app for it)");
    }
    daemon.kanna(&["send", "nope", "x"]).fails(4, "not_found");
    assert_eq!(daemon.kanna(&["close", "b"]).ok(), "closed p2");
    let app = daemon.app();
    assert_eq!(layout(&daemon, 0, 0), "a");
    app.quit();
}

#[test]
fn help_never_starts_anything_and_marks_what_the_running_kanna_lacks() {
    let dir = scratch("help");
    let nowhere = dir.join("nothing.sock");
    let overview = kanna_at(&dir, Some(&nowhere), &[]).ok();
    assert!(overview.starts_with("kanna — control the Kanna terminal multiplexer (windows → tabs → split panes).\n\nusage: kanna [--mux kanna|kmux] [--instance NAME] [--bg] [--fresh] COMMAND [ARGS] [--json] [-- TEXT]\n       kanna PATH                           (the Desk on folder PATH's project, opening one if none is open)\n\nPanes:\n  open "), "{overview}");
    assert!(overview.contains("Kanna isn't running, so help can't check which commands it supports."), "{overview}");
    assert!(overview.contains(&format!("Instance: default of {} at {}", kanna_build::Build::current().app_name(), nowhere.display())), "{overview}");
    assert!(overview.contains("6 needs Kanna.app, which isn't attached and wasn't started."), "{overview}");
    assert_eq!(kanna_at(&dir, Some(&nowhere), &["help"]).ok(), overview);
    assert_eq!(kanna_at(&dir, Some(&nowhere), &["--version"]).ok(), format!("kanna {} ({})", env!("CARGO_PKG_VERSION"), kanna_build::Build::current().app_name()));
    let open = kanna_at(&dir, Some(&nowhere), &["help", "open"]).ok();
    assert!(open.starts_with("kanna open — Open a terminal"), "{open}");
    assert_eq!(kanna_at(&dir, Some(&nowhere), &["open", "--help"]).ok(), open);
    assert_eq!(kanna_at(&dir, Some(&nowhere), &["open", "-h"]).ok(), open);
    assert!(kanna_at(&dir, Some(&nowhere), &["help", "read"]).ok().contains("usage: kanna read PANE [--lines N]"));
    assert!(kanna_at(&dir, Some(&nowhere), &["help", "instances"]).ok().starts_with("kanna instances — List the running Kanna instances."));
    assert!(kanna_at(&dir, Some(&nowhere), &["instances", "--help"]).ok().starts_with("kanna instances — "));
    assert!(kanna_at(&dir, Some(&nowhere), &["raw", "--help"]).ok().starts_with("usage: kanna raw REQUEST"));
    kanna_at(&dir, Some(&nowhere), &["help", "nope"]).fails(2, "unknown command \"nope\"");
    let commands = kanna_at(&dir, Some(&nowhere), &["commands"]).ok();
    assert_eq!(commands.lines().count(), 24);
    assert!(commands.starts_with("open          Open a terminal"), "{commands}");
    let described = kanna_at(&dir, Some(&nowhere), &["commands", "--json"]).json();
    assert_eq!(described["running"], false);
    assert_eq!(described["commands"].as_array().unwrap().len(), 24);
    assert_eq!(described["exitCodes"]["6"], "needs Kanna.app, which isn't attached and wasn't started");
    assert!(described["globalOptions"].as_array().unwrap().iter().any(|o| o["flag"] == "--"));

    let socket = dir.join("old.sock");
    let _requests = serve(&socket, |request| match request["cmd"].as_str() {
        Some("capabilities") => json!({ "ok": true, "mux": "kmux", "paneTypes": ["term"], "commands": ["capabilities", "list", "open"] }),
        Some(cmd) if ["list", "open"].contains(&cmd) => json!({ "ok": true }),
        Some(cmd) => json!({ "ok": false, "error": { "code": "bad_request", "message": format!("unknown command \"{cmd}\"") } }),
        None => json!({ "ok": false }),
    });
    let overview = kanna_at(&dir, Some(&socket), &[]).ok();
    assert!(overview.contains("  zoom          Toggle zoom: the pane fills its tab until you zoom again or focus another pane.  [not in the running Kanna]"), "{overview}");
    assert!(overview.contains("Commands marked [not in the running Kanna] aren't in the running Kanna yet."), "{overview}");
    let help = kanna_at(&dir, Some(&socket), &["help", "zoom"]).ok();
    assert!(help.contains("The running Kanna (default) doesn't support zoom yet."), "{help}");
    let described = kanna_at(&dir, Some(&socket), &["commands", "--json"]).json();
    assert_eq!(described["running"], true);
    let zoom = described["commands"].as_array().unwrap().iter().find(|c| c["name"] == "zoom").unwrap().clone();
    assert_eq!(zoom["supported"], false);
    kanna_at(&dir, Some(&socket), &["zoom", "a"]).fails(5, "the running Kanna doesn't support `zoom` yet. It supports: capabilities, list, open");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_running_kanna_supports_every_command() {
    let daemon = Daemon::start();
    let overview = daemon.kanna(&[]).ok();
    assert!(overview.contains("The running Kanna supports every command."), "{overview}");
    assert!(!overview.contains("[not in the running"), "{overview}");
}

#[test]
fn instances_are_folders_that_answer() {
    let dir = scratch("instances");
    let support = if cfg!(target_os = "macos") { dir.join("home/Library/Application Support") } else { dir.join("home/.local/share") };
    let build = kanna_build::Build::current();
    let mine = build.channel.instance_folder(&build.default_instance());
    assert!(kanna_at(&dir, None, &["instances"]).ok().starts_with("No Kanna instances are running."));
    let mut listeners = vec![];
    for folder in [mine.as_str(), "kanna-dev-work", "kanna-dev-gone", "kanna-prod", "kanna-staging"] {
        std::fs::create_dir_all(support.join(folder)).unwrap();
    }
    std::fs::write(support.join("kanna-dev-work").join(kanna_build::RECORD), json!({ "channel": "dev", "worktree": dir.join("wt") }).to_string()).unwrap();
    for folder in [mine.as_str(), "kanna-dev-work", "kanna-prod"] {
        let socket = support.join(folder).join("kanna.sock");
        listeners.push(serve(&socket, |request| match request["cmd"].as_str() {
            Some("list") => json!({ "ok": true, "windows": [{ "id": "w1" }], "panes": [{ "id": "p1" }, { "id": "p2" }] }),
            _ => json!({ "ok": false, "error": { "code": "bad_request", "message": "no" } }),
        }));
    }
    std::os::unix::net::UnixListener::bind(support.join("kanna-dev-gone/kanna.sock")).map(drop).unwrap();
    let text = kanna_at(&dir, None, &["instances"]).ok();
    let lines: Vec<&str> = text.lines().collect();
    let name = build.default_instance();
    assert!(lines[0].starts_with("  default ") && lines[0].contains(" production  1 window, 2 panes"), "{text}");
    assert!(lines.iter().any(|l| l.starts_with(&format!("* {name} ")) && l.contains(" dev ")), "{text}");
    let work = lines.iter().find(|l| l.starts_with("  work ")).unwrap();
    assert!(work.ends_with(&format!("1 window, 2 panes  {} (gone)", dir.join("wt").display())), "{text}");
    assert!(!text.contains("gone "), "{text}");
    assert!(text.ends_with(&format!("* the instance this shell's kanna commands talk to ({}). Pick another with --instance NAME or KANNA_INSTANCE.", support.join(&mine).join("kanna.sock").display())), "{text}");
    let text = kanna_at(&dir, None, &["--instance", "work", "instances"]).ok();
    assert!(text.contains("\n* work "), "{text}");
    let mut command = cli(&dir);
    command.env("KANNA_INSTANCE", "work");
    let described = Run::from(&["instances", "--json"], command.args(["instances", "--json"]).output().unwrap()).json();
    assert_eq!(described["current"]["name"], "work");
    let row = described["instances"].as_array().unwrap().iter().find(|r| r["name"] == "work").unwrap().clone();
    assert_eq!(row, json!({ "name": "work", "channel": "dev", "worktree": dir.join("wt"), "socket": support.join("kanna-dev-work/kanna.sock"), "current": true, "windows": 1, "panes": 2 }));
    assert_eq!(kanna_at(&dir, None, &["--instance", "work", "list", "--json"]).json()["panes"].as_array().unwrap().len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn prune_deletes_dev_instances_whose_worktree_is_gone() {
    let dir = scratch("prune");
    let support = if cfg!(target_os = "macos") { dir.join("home/Library/Application Support") } else { dir.join("home/.local/share") };
    std::fs::create_dir_all(dir.join("here")).unwrap();
    let record = |folder: &str, worktree: Option<std::path::PathBuf>| {
        std::fs::create_dir_all(support.join(folder)).unwrap();
        std::fs::write(support.join(folder).join(kanna_build::RECORD), json!({ "channel": "dev", "worktree": worktree }).to_string()).unwrap();
    };
    record("kanna-dev-gone", Some(dir.join("gone")));
    record("kanna-dev-here", Some(dir.join("here")));
    record("kanna-dev", None);
    record("kanna-dev-running", Some(dir.join("gone2")));
    let _listener = serve(&support.join("kanna-dev-running/kanna.sock"), |_| json!({ "ok": true }));
    record("kanna-dev-kept", Some(dir.join("gone3")));
    std::fs::write(support.join("kanna-dev-kept/keeper.pid"), std::process::id().to_string()).unwrap();
    let text = kanna_at(&dir, None, &["instances", "prune"]).ok();
    assert!(text.contains(&format!("deleted {}", support.join("kanna-dev-gone").display())), "{text}");
    assert!(text.contains("kanna-dev-running: its worktree") && text.contains("but it's running"), "{text}");
    assert!(text.contains(&format!("its keeper (pid {}) still holds terminals", std::process::id())), "{text}");
    assert!(!support.join("kanna-dev-gone").exists());
    for kept in ["kanna-dev-here", "kanna-dev", "kanna-dev-running", "kanna-dev-kept"] {
        assert!(support.join(kept).exists(), "{kept}");
    }
    std::fs::remove_dir_all(support.join("kanna-dev-kept")).unwrap();
    std::fs::remove_dir_all(support.join("kanna-dev-running")).unwrap();
    assert!(kanna_at(&dir, None, &["instances", "prune"]).ok().starts_with("Nothing to prune"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_daemon_names_its_instance_and_tells_its_terminals() {
    let daemon = Daemon::start();
    let capabilities = daemon.kanna(&["capabilities", "--json"]).json();
    assert_eq!(capabilities["instance"], "default");
    assert_eq!(capabilities["mux"], "kmux");
    assert_eq!(capabilities["app"], false);
    daemon.kanna(&["open", "--name", "env", "--cmd", "echo \"I=$KANNA_INSTANCE P=$KANNA_PANE S=$KANNA_SOCKET\"; exec sleep 60"]).ok();
    let expected = "I=default P=p1 S=/";
    std::thread::sleep(std::time::Duration::from_secs(2));
    let screen = daemon.kanna(&["read", "env"]).ok();
    assert!(screen.contains(expected), "{screen}");
}

#[test]
fn the_kmux_backend_is_kmux_through_kannas_cli() {
    let dir = scratch("kmux");
    let socket = dir.join("kmux.sock");
    let requests = serve(&socket, |request| match request["cmd"].as_str() {
        Some("capabilities") => json!({ "ok": true, "mux": "kmux", "paneTypes": ["term"], "commands": ["capabilities", "list"] }),
        Some("list") => json!({ "ok": true, "windows": [], "panes": [] }),
        _ => json!({ "ok": false, "error": { "code": "bad_request", "message": "unknown command" } }),
    });
    let run = |args: &[&str]| Run::from(args, cli(&dir).env("KMUX_SOCKET", &socket).args(args).output().unwrap());
    assert_eq!(run(&["--mux", "kmux", "list"]).ok(), "no windows (open one with `kanna open`)");
    let overview = run(&["--mux", "kmux"]).ok();
    assert!(overview.contains("Backend: kmux (--mux kmux), not Kanna."), "{overview}");
    assert!(overview.contains("[not in the running kmux]"), "{overview}");
    assert!(requests.try_iter().any(|r| r["cmd"] == "list"));
    let mut command = cli(&dir);
    command.env("KANNA_MUX", "kmux").env("KMUX_SOCKET", &socket);
    assert_eq!(Run::from(&["list"], command.arg("list").output().unwrap()).ok(), "no windows (open one with `kanna open`)");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_folder_for_a_command_opens_a_desk_on_its_project_without_a_task() {
    let daemon = Daemon::start();
    let _app = daemon.app();
    std::fs::create_dir_all(daemon.dir.join("project")).unwrap();
    let project = daemon.dir.join("project").to_string_lossy().to_string();
    assert_eq!(daemon.kanna(&["project"]).ok(), format!("opened w1 on {project}"));
    assert_eq!(daemon.kanna(&["project"]).ok(), format!("brought forward w1 on {project}"));
    let list = daemon.kanna(&["list", "--json"]).json();
    assert_eq!(list["panes"], json!([]), "a Desk on a project docks nothing, so it uses none of its tasks: {list}");
    assert_eq!(list["windows"][0]["id"], json!("w1"), "{list}");
    daemon.kanna(&["nowhere"]).fails(2, "unknown command \"nowhere\"");
}

#[test]
fn a_backlog_task_is_renamed_and_starts_with_its_new_name() {
    let daemon = Daemon::start();
    let _app = daemon.app();
    let git = |args: &[&str]| assert!(std::process::Command::new("git").args(["-c", "user.email=e@e", "-c", "user.name=e"]).args(args).current_dir(&daemon.dir).status().unwrap().success());
    git(&["init", "-q"]);
    git(&["commit", "-q", "--allow-empty", "-m", "first"]);
    assert_eq!(daemon.kanna(&["task", "new", "Old name", "say hi", "--agent", "/bin/cat"]).ok(), format!("added t1 \"Old name\" to the backlog of {}", daemon.dir.display()));
    assert_eq!(daemon.kanna(&["task", "name", "t1", "New name"]).ok(), "named t1 \"New name\"");
    let listed = daemon.kanna(&["--json", "task", "list"]).json()["backlog"][0].clone();
    assert_eq!((listed["name"].clone(), listed["message"].clone()), (json!("New name"), json!("say hi")));
    daemon.kanna(&["task", "name", "t9", "x"]).fails(4, "no task \"t9\" in the backlog or on the Shelf");
    daemon.kanna(&["task", "name", "t1", " "]).fails(2, "a task needs a name");
    let started = daemon.kanna(&["--json", "task", "start", "t1"]).json();
    let pane = started["pane"].as_str().unwrap().to_string();
    wait_for("the started task's pane", || daemon.kanna(&["--json", "list"]).json()["panes"].as_array().unwrap().iter().any(|p| p["id"] == pane.as_str() && p["name"] == "New name"));
    assert_eq!(daemon.kanna(&["task", "name", "t1", "Newer name"]).ok(), "named t1 \"Newer name\"");
    assert!(daemon.kanna(&["--json", "list"]).json()["panes"].as_array().unwrap().iter().any(|p| p["id"] == pane.as_str() && p["name"] == "Newer name"));
    assert_eq!(daemon.kanna(&["--json", "task", "list"]).json()["backlog"][0]["name"], json!("Newer name"));
}
