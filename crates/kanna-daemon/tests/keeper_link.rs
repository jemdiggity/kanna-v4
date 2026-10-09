mod common;

use common::{alive, wait_for, Instance};
use serde_json::json;

#[test]
fn terminals_come_from_the_keeper_and_the_daemon_stops_when_it_goes() {
    let mut instance = Instance::start();

    let opened = instance.call("open", json!({"type": "term", "name": "code", "cmd": "exit 3"}));
    assert_eq!(opened["ok"], true, "{opened}");
    wait_for("code to exit", || instance.pane("code")["state"] == "exited");
    let code = instance.pane("code");
    assert_eq!(code["exitCode"], 3, "{code}");
    assert!(code["pid"].as_i64().unwrap() > 0, "{code}");

    let opened = instance.call("open", json!({"type": "term", "name": "shell"}));
    let pid = opened["pane"]["pid"].as_i64().unwrap_or_else(|| panic!("no pid in {opened}"));
    assert!(alive(pid));
    assert_eq!(instance.pane("shell")["pid"], pid);
    instance.call("send", json!({"pane": "shell", "text": "echo keeper-$((40+2))"}));
    wait_for("the shell's output", || instance.call("read", json!({"pane": "shell"}))["text"].as_str().unwrap_or("").contains("keeper-42"));

    let restarted = instance.call("restart", json!({"pane": "shell"}));
    let new_pid = restarted["pane"]["pid"].as_i64().unwrap_or_else(|| panic!("no pid in {restarted}"));
    assert_ne!(new_pid, pid);
    wait_for("the old shell to end", || !alive(pid));

    instance.call("close", json!({"pane": "shell"}));
    wait_for("the closed shell to end", || !alive(new_pid));

    let keeper = instance.keeper_pid();
    unsafe { libc::kill(keeper, libc::SIGTERM) };
    wait_for("the daemon to stop", || instance.daemon.try_wait().unwrap().is_some());
    assert!(!instance.daemon.try_wait().unwrap().unwrap().success());
    assert!(instance.log().contains("went away"), "{}", instance.log());
}
