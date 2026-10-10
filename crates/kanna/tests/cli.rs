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

    let capabilities = daemon.kanna(&["capabilities"]).ok();
    assert!(capabilities.starts_with("pane types: term, web, ios, md\ncommands: arrange, capabilities, close, focus, list, move, move-tab, navigate, open, read, rename-tab, resize, restart, send, status, zoom"), "{capabilities}");
    assert!(capabilities.ends_with("Kanna.app: attached"), "{capabilities}");
    let raw = daemon.kanna(&["raw", r#"{"cmd":"debug.anything","args":{"pane":"a"}}"#]).json();
    assert_eq!(raw, json!({ "cmd": "debug.anything", "args": { "pane": "p1" } }));
    assert_eq!(daemon.kanna(&["raw", r#"{"cmd":"list"}"#]).json()["panes"].as_array().unwrap().len(), 6);

    assert_eq!(daemon.kanna(&["close", "--window", "w3"]).ok(), "closed window w3 (panes p3)");
    assert_eq!(daemon.kanna(&["close", "--tab", "t5"]).ok(), "closed tab t5 (panes p6)");
    assert_eq!(daemon.kanna(&["close", "site"]).ok(), "closed p5");
    let reply = daemon.kanna(&["--json", "close", "d"]).json();
    assert_eq!(reply, json!({ "closed": ["p4"] }));
    assert_eq!(layout(&daemon, 0, 0), "a:2/3 | b:1/3");
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
    assert!(overview.starts_with("kanna — control the Kanna terminal multiplexer (windows → tabs → split panes).\n\nusage: kanna [--mux kanna|kmux] [--instance NAME] [--bg] [--fresh] COMMAND [ARGS] [--json] [-- TEXT]\n\nPanes:\n  open "), "{overview}");
    assert!(overview.contains("Kanna isn't running, so help can't check which commands it supports."), "{overview}");
    assert!(overview.contains(&format!("Instance: default at {}", nowhere.display())), "{overview}");
    assert!(overview.contains("6 needs Kanna.app, which isn't attached and wasn't started."), "{overview}");
    assert_eq!(kanna_at(&dir, Some(&nowhere), &["help"]).ok(), overview);
    assert_eq!(kanna_at(&dir, Some(&nowhere), &["--version"]).ok(), format!("kanna {}", env!("CARGO_PKG_VERSION")));
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
    assert_eq!(commands.lines().count(), 16);
    assert!(commands.starts_with("open          Open a terminal"), "{commands}");
    let described = kanna_at(&dir, Some(&nowhere), &["commands", "--json"]).json();
    assert_eq!(described["running"], false);
    assert_eq!(described["commands"].as_array().unwrap().len(), 16);
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
    kanna_at(&dir, None, &["instances"]).ok();
    assert!(kanna_at(&dir, None, &["instances"]).ok().starts_with("No Kanna instances are running."));
    let mut listeners = vec![];
    for folder in ["kanna-dev", "kanna-dev-work", "kanna-dev-gone"] {
        std::fs::create_dir_all(support.join(folder)).unwrap();
    }
    for folder in ["kanna-dev", "kanna-dev-work"] {
        let socket = support.join(folder).join("kanna.sock");
        listeners.push(serve(&socket, |request| match request["cmd"].as_str() {
            Some("list") => json!({ "ok": true, "windows": [{ "id": "w1" }], "panes": [{ "id": "p1" }, { "id": "p2" }] }),
            _ => json!({ "ok": false, "error": { "code": "bad_request", "message": "no" } }),
        }));
    }
    std::os::unix::net::UnixListener::bind(support.join("kanna-dev-gone/kanna.sock")).map(drop).unwrap();
    let text = kanna_at(&dir, None, &["instances"]).ok();
    let default_socket = support.join("kanna-dev/kanna.sock");
    assert_eq!(text, format!("* default  1 window, 2 panes\n  work     1 window, 2 panes\n\n* the instance this shell's kanna commands talk to ({}). Pick another with --instance NAME or KANNA_INSTANCE.", default_socket.display()));
    let text = kanna_at(&dir, None, &["--instance", "work", "instances"]).ok();
    assert!(text.starts_with("  default  1 window, 2 panes\n* work "), "{text}");
    let mut command = cli(&dir);
    command.env("KANNA_INSTANCE", "work");
    let described = Run::from(&["instances", "--json"], command.args(["instances", "--json"]).output().unwrap()).json();
    assert_eq!(described["current"]["name"], "work");
    assert_eq!(described["instances"][1], json!({ "name": "work", "socket": support.join("kanna-dev-work/kanna.sock"), "current": true, "windows": 1, "panes": 2 }));
    assert_eq!(kanna_at(&dir, None, &["--instance", "work", "list", "--json"]).json()["panes"].as_array().unwrap().len(), 2);
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
