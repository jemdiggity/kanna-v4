mod support;

use kanna_mux::json::json;
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
