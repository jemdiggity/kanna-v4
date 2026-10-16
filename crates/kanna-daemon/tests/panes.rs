mod common;

use common::{App, Instance};
use serde_json::{json, Value};

fn names(list: &Value) -> Vec<String> {
    list["panes"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap_or("-").to_string()).collect()
}

#[test]
fn with_no_app_only_terminals_are_known_and_other_panes_need_the_app() {
    let instance = Instance::start();
    instance.open("sh", Some("cat"));
    for (cmd, args) in [
        ("open", json!({"type": "web", "name": "site", "url": "example.com"})),
        ("open", json!({"type": "md", "name": "doc", "path": "/tmp/a.md"})),
        ("open", json!({"type": "code", "name": "src", "path": "/tmp/a.rs", "line": 3})),
        ("open", json!({"type": "ios", "name": "phone", "app": "com.example.App", "wait": false})),
        ("navigate", json!({"pane": "site", "url": "example.org"})),
        ("close", json!({"pane": "site"})),
        ("restart", json!({"pane": "doc"})),
        ("debug.web", json!({"pane": "site"})),
        ("comment", json!({"path": "/tmp/a.rs", "line": 3, "text": "why"})),
    ] {
        let reply = instance.call(cmd, args.clone());
        assert_eq!(reply["error"]["code"], "no_app", "{cmd} {args}: {reply}");
    }
    for cmd in ["send", "read"] {
        let reply = instance.call(cmd, json!({"pane": "site", "text": "x"}));
        assert_eq!(reply["error"]["code"], "not_found", "{cmd}: {reply}");
    }
    let wrong = instance.call("navigate", json!({"pane": "sh", "url": "example.com"}));
    assert_eq!(wrong["error"]["code"], "wrong_type", "{wrong}");
    let list = instance.call("list", json!({}));
    assert_eq!(names(&list), vec!["sh"]);
    let capabilities = instance.call("capabilities", json!({}));
    assert_eq!(capabilities["paneTypes"], json!(["term", "web", "ios", "md", "code"]), "{capabilities}");
}

#[test]
fn the_app_lends_its_panes_while_it_is_attached() {
    let instance = Instance::start();
    instance.open("sh", Some("cat"));
    let lent = json!([
        {"id": "p7", "name": "site", "type": "web", "url": "http://localhost:1/", "history": true, "state": "running"},
        {"id": "p8", "name": "doc", "type": "md", "path": "/tmp/a.md", "zoom": 1.5, "state": "starting"},
        {"id": "p1", "name": "clash", "type": "web", "url": "http://localhost:2/"},
        {"id": "p9", "name": "sh", "type": "md", "path": "/tmp/b.md"},
        {"id": "p10", "name": "term", "type": "term"}
    ]);
    let app = App::connect_with(&instance.socket(), lent);
    let stream = app.stream.try_clone().unwrap();
    let answering = app.answering();
    let list = instance.call("list", json!({}));
    assert_eq!(names(&list), vec!["sh", "site", "doc"], "{list}");
    assert_eq!(list["panes"][1]["url"], "http://localhost:1/", "{list}");
    assert_eq!(list["panes"][1]["state"], "running", "{list}");
    assert_eq!(list["panes"][2]["state"], "starting", "{list}");
    let opened = instance.call("open", json!({"type": "term", "name": "next", "cmd": "cat"}));
    assert_eq!(opened["pane"]["id"], "p9", "{opened}");
    let navigated = instance.call("navigate", json!({"pane": "site", "url": "localhost:3"}));
    assert_eq!(navigated["pane"]["url"], "http://localhost:3", "{navigated}");
    let back = instance.call("navigate", json!({"pane": "site", "back": true}));
    assert_eq!(back["pane"]["url"], "http://localhost:1/", "{back}");
    let marked = instance.call("markup", json!({"pane": "doc", "agent": "next"}));
    assert_eq!((marked["pane"]["markup"].clone(), marked["pane"]["agent"].clone()), (json!(true), json!("p9")), "{marked}");
    let nowhere = instance.call("markup", json!({"pane": "doc", "agent": "nobody"}));
    assert_eq!(nowhere["error"]["code"], "not_found", "{nowhere}");
    let plain = instance.call("markup", json!({"pane": "doc", "off": true}));
    assert!(plain["pane"]["markup"].is_null(), "{plain}");
    let wrong = instance.call("markup", json!({"pane": "next"}));
    assert_eq!(wrong["error"]["code"], "wrong_type", "{wrong}");

    stream.shutdown(std::net::Shutdown::Both).unwrap();
    answering.join().unwrap();
    common::wait_for("the app's panes to go", || names(&instance.call("list", json!({}))) == vec!["sh", "next"]);
}

#[test]
fn updates_from_the_app_change_the_record_and_come_back_to_it() {
    let instance = Instance::start();
    let mut app = App::connect_with(&instance.socket(), json!([{"id": "p1", "name": "site", "type": "web", "url": "http://a.test/", "history": true, "state": "running"}]));
    app.tell("update", json!({"pane": "p1", "url": "http://b.test/"}));
    let echoed = app.until("the record", |m| m["cmd"] == "pane");
    assert_eq!(echoed["args"]["pane"]["url"], "http://b.test/", "{echoed}");
    app.tell("update", json!({"pane": "p1", "title": "Page B"}));
    let titled = app.until("the titled record", |m| m["cmd"] == "pane");
    assert_eq!((titled["args"]["pane"]["url"].clone(), titled["args"]["pane"]["title"].clone()), (json!("http://b.test/"), json!("Page B")), "{titled}");
    app.tell("update", json!({"pane": "p1", "title": "\u{1b}[31mRed\u{7}"}));
    let cleaned = app.until("the cleaned record", |m| m["cmd"] == "pane");
    assert_eq!(cleaned["args"]["pane"]["title"], "[31mRed", "{cleaned}");
    app.tell("update", json!({"pane": "p1", "title": ""}));
    let untitled = app.until("the untitled record", |m| m["cmd"] == "pane");
    assert!(untitled["args"]["pane"]["title"].is_null(), "{untitled}");
    app.send(json!({"id": 5, "cmd": "navigate", "args": {"pane": "site", "back": true}}));
    let stop = app.until("stop", |m| m["cmd"] == "stop");
    assert_eq!(stop["args"]["pane"], "p1");
    let start = app.until("start", |m| m["cmd"] == "start");
    assert_eq!(start["args"]["pane"]["url"], "http://a.test/", "{start}");
    let reply = app.until("the reply", |m| m["id"] == 5);
    assert_eq!(reply["pane"]["url"], "http://a.test/", "{reply}");
    assert_eq!(reply["pane"]["state"], "running", "{reply}");
    app.tell("update", json!({"pane": "p1", "state": "failed", "error": "gone"}));
    app.until("the record", |m| m["cmd"] == "pane");
    let _answering = app.answering();
    let site = instance.pane("site");
    assert_eq!((site["state"].clone(), site["error"].clone()), (json!("failed"), json!("gone")), "{site}");
}

#[test]
fn when_the_app_goes_away_its_panes_go_and_a_waiting_open_fails() {
    let instance = Instance::start();
    instance.open("sh", Some("cat"));
    let mut app = App::connect(&instance.socket());
    app.shows = false;
    let socket = instance.socket();
    let caller = std::thread::spawn(move || common::call(&socket, "open", json!({"type": "web", "name": "site", "url": "a.test"})));
    app.until("start", |m| m["cmd"] == "start");
    drop(app);
    let opened = caller.join().unwrap();
    assert_eq!(opened["error"]["code"], "start_failed", "{opened}");
    let list = instance.call("list", json!({}));
    assert_eq!(names(&list), vec!["sh"], "{list}");
    assert_eq!(list["windows"], json!([]), "{list}");
}

#[test]
fn a_comment_opens_its_file_once_and_stays_on_the_pane() {
    let instance = Instance::start();
    let app = App::connect(&instance.socket());
    let _answering = app.answering();
    let first = instance.call("comment", json!({"path": "/tmp/tour.rs", "line": 5, "text": "Queues the message."}));
    assert_eq!((first["opened"].clone(), first["first"].clone(), first["last"].clone()), (json!(true), json!(5), json!(7)), "{first}");
    let id = first["pane"]["id"].as_str().unwrap().to_string();
    assert_eq!(first["pane"]["type"], "code", "{first}");
    assert_eq!(first["pane"]["line"], 5, "{first}");
    assert_eq!(first["pane"]["comment"], json!({"path": "/tmp/tour.rs", "first": 5, "last": 7, "text": "Queues the message."}), "{first}");

    let again = instance.call("comment", json!({"path": "/tmp/tour.rs", "line": 20, "last": 24, "text": "Sends it."}));
    assert_eq!((again["opened"].clone(), again["pane"]["id"].clone()), (json!(false), json!(id)), "{again}");
    assert_eq!((again["first"].clone(), again["last"].clone(), again["pane"]["line"].clone()), (json!(20), json!(24), json!(20)), "{again}");
    assert_eq!(again["pane"]["comment"]["text"], "Sends it.", "{again}");

    let other = instance.call("comment", json!({"path": "/tmp/other.rs", "line": 1, "text": "The other side."}));
    assert_eq!(other["opened"], true, "{other}");
    assert_ne!(other["pane"]["id"], json!(id), "{other}");
    let placed = instance.call("comment", json!({"path": "/tmp/tour.rs", "line": 1, "text": "Again.", "tab": true}));
    assert_eq!(placed["opened"], true, "{placed}");

    let list = instance.call("list", json!({}));
    let tour = list["panes"].as_array().unwrap().iter().find(|p| p["id"] == json!(id)).unwrap().clone();
    assert_eq!(tour["comment"]["first"], 20, "{list}");
    for (args, code) in [
        (json!({"path": "/tmp/tour.rs", "line": 5}), "bad_request"),
        (json!({"path": "/tmp/tour.rs", "line": 5, "text": "  "}), "bad_request"),
        (json!({"path": "/tmp/tour.rs", "line": 9, "last": 3, "text": "x"}), "bad_request"),
        (json!({"line": 5, "text": "x"}), "bad_request"),
    ] {
        let reply = instance.call("comment", args.clone());
        assert_eq!(reply["error"]["code"], code, "{args}: {reply}");
    }
}
