mod common;

use common::{App, Instance};
use serde_json::json;

#[test]
fn with_no_app_terminals_open_unplaced_and_layout_commands_fail_with_no_app() {
    let instance = Instance::start();
    let opened = instance.call("open", json!({"type": "term", "name": "plain"}));
    assert_eq!(opened["ok"], true, "{opened}");
    assert_eq!((opened["window"].clone(), opened["tab"].clone()), (json!(null), json!(null)), "{opened}");

    for (cmd, args) in [
        ("open", json!({"type": "term", "name": "split", "split": "right"})),
        ("open", json!({"type": "term", "name": "tabbed", "tab": true})),
        ("focus", json!({"pane": "plain"})),
        ("arrange", json!({"layout": {"pane": "plain"}})),
        ("close", json!({"tab": "t1"})),
        ("debug.saveState", json!({})),
    ] {
        let reply = instance.call(cmd, args.clone());
        assert_eq!(reply["error"]["code"], "no_app", "{cmd} {args}: {reply}");
    }
    let list = instance.call("list", json!({}));
    assert_eq!(list["windows"], json!([]), "{list}");
    let names: Vec<&str> = list["panes"].as_array().unwrap().iter().filter_map(|p| p["name"].as_str()).collect();
    assert_eq!(names, vec!["plain"], "{list}");
    assert_eq!(instance.call("close", json!({"pane": "plain"}))["ok"], true);
}

#[test]
fn with_the_app_attached_it_places_panes_and_answers_for_the_layout() {
    let instance = Instance::start();
    instance.open("early", Some("cat"));
    let app = instance.app();
    assert_eq!(app.owner.view()["windows"][0]["root"], json!({"pane": "p1", "type": "term"}));
    let _answering = app.answering();
    let opened = instance.call("open", json!({"type": "term", "name": "right", "cmd": "cat", "split": "right", "size": "1/3"}));
    assert_eq!((opened["ok"].clone(), opened["window"].clone(), opened["tab"].clone()), (json!(true), json!("w1"), json!("t1")), "{opened}");
    let focused = instance.call("focus", json!({"pane": "early"}));
    assert_eq!(focused["ok"], true, "{focused}");
    let list = instance.call("list", json!({}));
    assert_eq!(list["windows"][0]["focused"], "early", "{list}");
    assert_eq!(list["windows"][0]["tabs"][0]["layout"], json!({"split": "row", "children": [{"pane": "early", "size": "2/3"}, {"pane": "right", "size": "1/3"}]}), "{list}");
    assert_eq!(instance.call("close", json!({"pane": "right"}))["ok"], true);
    let list = instance.call("list", json!({}));
    assert_eq!(list["windows"][0]["tabs"][0]["layout"], json!({"pane": "early"}), "{list}");
}

#[test]
fn an_app_that_goes_away_mid_request_leaves_an_unplaced_open_working() {
    let instance = Instance::start();
    let app = App::connect(&instance.socket());
    let socket = instance.socket();
    let caller = std::thread::spawn(move || common::call(&socket, "open", json!({"type": "term", "name": "orphan", "cmd": "cat"})));
    std::thread::sleep(std::time::Duration::from_millis(300));
    drop(app);
    let opened = caller.join().unwrap();
    assert_eq!(opened["ok"], true, "{opened}");
    assert_eq!(opened["window"], json!(null), "{opened}");
}
