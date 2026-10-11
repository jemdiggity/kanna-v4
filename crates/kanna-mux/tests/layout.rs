mod support;

use kanna_mux::json::{json, Value};
use kanna_mux::Core;
use support::{make_core, send};

fn two_panes() -> Core {
    let (core, _) = make_core();
    for name in ["a", "b"] {
        let reply = send(&core, json!({ "cmd": "open", "args": { "type": "term", "name": name } }));
        assert_eq!(reply["ok"], true, "{reply}");
    }
    core
}

#[test]
fn arrange_with_no_space_left_names_the_unsized_panes() {
    let core = two_panes();
    let reply = send(&core, json!({ "cmd": "arrange", "args": { "layout": { "split": "row", "children": [{ "pane": "a", "size": 1 }, { "pane": "b" }] } } }));
    assert_eq!(reply["error"]["code"], "layout_invalid");
    assert_eq!(reply["error"]["message"], "no space left for the unsized panes in a row");
}

#[test]
fn resizing_a_pane_that_had_the_whole_split_shares_the_rest_equally() {
    let core = two_panes();
    assert_eq!(send(&core, json!({ "cmd": "open", "args": { "type": "term", "name": "c" } }))["ok"], true);
    let mut state = send(&core, json!({ "cmd": "debug.saveState" }))["state"].clone();
    state["windows"][0]["tabs"][0]["layout"] =
        json!({ "split": "row", "children": [{ "pane": "p1", "size": 1 }, { "pane": "p2", "size": 1e-300 }, { "pane": "p3", "size": 1e-300 }] });
    let restored = send(&core, json!({ "cmd": "debug.restoreState", "args": { "state": state } }));
    assert_eq!(restored["ok"], true, "{restored}");
    let reply = send(&core, json!({ "cmd": "resize", "args": { "pane": "a", "size": "1/2" } }));
    assert_eq!(reply["ok"], true, "{reply}");
    let sizes: Vec<Value> = reply["layout"]["children"].as_array().unwrap().iter().map(|kid| kid["size"].clone()).collect();
    assert_eq!(sizes, vec![json!("1/2"), json!("1/4"), json!("1/4")], "{reply}");
    let saved = send(&core, json!({ "cmd": "debug.saveState" }));
    let kids = saved["state"]["windows"][0]["tabs"][0]["layout"]["children"].as_array().unwrap().clone();
    assert!(kids.iter().all(|kid| kid["size"].as_f64().is_some_and(f64::is_finite)), "{saved}");
}

#[test]
fn move_docks_only_a_task_and_moves_the_docked_pane_to_the_other_edge_or_another_window() {
    let (core, _) = make_core();
    for args in [json!({ "type": "term", "name": "a" }), json!({ "type": "term", "name": "agent", "dock": "left" }), json!({ "type": "term", "name": "other", "dock": "left", "window": "new" })] {
        assert_eq!(send(&core, json!({ "cmd": "open", "args": args }))["ok"], true);
    }
    let refused = send(&core, json!({ "cmd": "move", "args": { "pane": "a", "dock": "left" } }));
    assert_eq!(refused["error"]["code"], "not_a_task", "{refused}");
    assert_eq!(refused["error"]["message"], "a isn't a task, and only a task's agent docks: start one with `kanna task new` and `kanna task start`");
    let list = send(&core, json!({ "cmd": "list" }));
    assert_eq!(list["windows"][0]["dock"]["pane"], "agent", "{list}");
    assert_eq!(list["windows"][0]["tabs"][0]["layout"], json!({ "pane": "a" }), "{list}");

    let flipped = send(&core, json!({ "cmd": "move", "args": { "pane": "agent", "dock": "right" } }));
    assert_eq!(flipped["dock"], json!({ "pane": "agent", "edge": "right", "width": "2/5" }), "{flipped}");
    let moved = send(&core, json!({ "cmd": "move", "args": { "pane": "other", "dock": "left", "window": "w1" } }));
    assert_eq!((moved["window"].clone(), moved["dock"]["pane"].clone()), (json!("w1"), json!("other")), "{moved}");

    assert_eq!(send(&core, json!({ "cmd": "move", "args": { "pane": "other", "tab": "new" } }))["ok"], true);
    let again = send(&core, json!({ "cmd": "move", "args": { "pane": "other", "dock": "left" } }));
    assert_eq!(again["error"]["code"], "not_a_task", "{again}");
}
