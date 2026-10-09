use kanna_layout::{kanna_layout_call, kanna_layout_close, kanna_layout_fraction, kanna_layout_free_string, kanna_layout_open};
use serde_json::{json, Value};
use std::ffi::{CStr, CString};
use std::ptr::null_mut;

fn call(layout: *mut kanna_layout::KannaLayout, request: Value) -> Value {
    let request = CString::new(request.to_string()).unwrap();
    let reply = kanna_layout_call(layout, request.as_ptr());
    let text = unsafe { CStr::from_ptr(reply) }.to_str().unwrap().to_string();
    kanna_layout_free_string(reply);
    serde_json::from_str(&text).unwrap()
}

#[test]
fn the_app_places_saves_and_reopens_its_layout_through_the_c_interface() {
    let dir = std::env::temp_dir().join(format!("kl-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::set_permissions(&dir, std::os::unix::fs::PermissionsExt::from_mode(0o700)).unwrap();
    let path = CString::new(dir.join("layout.json").to_string_lossy().as_bytes()).unwrap();
    let mut problem = null_mut();
    let layout = kanna_layout_open(path.as_ptr(), false, &mut problem);
    assert!(problem.is_null());
    let panes = json!([{ "id": "p1", "name": "left", "type": "term" }]);
    assert_eq!(call(layout, json!({ "call": "attach", "panes": panes }))["placed"], json!(["p1"]));
    let placed = call(layout, json!({ "call": "serve", "request": { "op": "place", "pane": { "id": "p2", "name": "right", "type": "term" }, "at": { "split": "right", "size": 0.25 } } }));
    assert_eq!(placed, json!({ "ok": true, "window": "w1", "tab": "t1" }));
    assert_eq!(call(layout, json!({ "call": "save" }))["saved"], true);
    let view = call(layout, json!({ "call": "view" }));
    kanna_layout_close(layout);

    let layout = kanna_layout_open(path.as_ptr(), false, &mut problem);
    assert!(problem.is_null());
    let all = json!([{ "id": "p1", "name": "left", "type": "term" }, { "id": "p2", "name": "right", "type": "term" }]);
    assert_eq!(call(layout, json!({ "call": "attach", "panes": all }))["placed"], json!([]));
    assert_eq!(call(layout, json!({ "call": "view" })), view);
    let windows = call(layout, json!({ "call": "serve", "request": { "op": "windows" } }));
    assert_eq!(windows["windows"][0]["tabs"][0]["layout"], json!({ "split": "row", "children": [{ "pane": "left", "size": "3/4" }, { "pane": "right", "size": "1/4" }] }));
    assert_eq!(call(layout, json!({ "call": "nope" }))["error"]["code"], "bad_request");
    kanna_layout_close(layout);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fractions_are_shown_as_kmux_shows_them() {
    for (value, text) in [(0.25, "1/4"), (2.0 / 3.0, "2/3"), (0.5, "1/2")] {
        let reply = kanna_layout_fraction(value);
        assert_eq!(unsafe { CStr::from_ptr(reply) }.to_str().unwrap(), text);
        kanna_layout_free_string(reply);
    }
}
