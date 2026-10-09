mod support;

use kanna_mux::json::{json, Value};
use support::{make_core, send};

fn state(pane: &str, tab: &str, window: &str) -> Value {
    json!({
        "kmux": "state", "version": 1, "key": window, "counters": {},
        "windows": [{ "id": window, "active": tab, "tabCount": 1, "tabs": [{ "id": tab, "title": "Tab 1", "layout": { "pane": pane } }] }],
        "panes": [{ "id": pane, "type": "term" }]
    })
}

#[test]
fn ids_too_big_to_count_from_are_bad_state() {
    let huge = "18446744073709551615";
    for (pane, tab, window) in [
        (format!("p{huge}"), "t1".to_string(), "w1".to_string()),
        ("p1".into(), format!("t{huge}"), "w1".into()),
        ("p1".into(), "t1".into(), format!("w{huge}")),
        ("p1000000000".into(), "t1".into(), "w1".into()),
    ] {
        let (core, _) = make_core();
        let reply = send(&core, json!({ "cmd": "debug.restoreState", "args": { "state": state(&pane, &tab, &window) } }));
        assert_eq!(reply["ok"], false, "{pane} {tab} {window}: {reply}");
        assert_eq!(reply["error"]["code"], "bad_request");
        let opened = send(&core, json!({ "cmd": "open", "args": { "type": "term", "tab": true } }));
        assert_eq!(opened["ok"], true, "{opened}");
    }
}

#[test]
fn the_largest_ids_still_restore_and_count_on() {
    let (core, _) = make_core();
    let reply = send(&core, json!({ "cmd": "debug.restoreState", "args": { "state": state("p999999999", "t999999999", "w999999999") } }));
    assert_eq!(reply["ok"], true, "{reply}");
    let opened = send(&core, json!({ "cmd": "open", "args": { "type": "term", "tab": true } }));
    assert_eq!(opened["pane"]["id"], "p1000000000", "{opened}");
    assert_eq!(opened["tab"], "t1000000000", "{opened}");
}
