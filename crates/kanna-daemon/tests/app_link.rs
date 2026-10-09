mod common;

use common::{data, is, wait_for, App, Instance};
use serde_json::json;
use std::io::Write;
use std::time::{Duration, Instant};

#[test]
fn a_resize_beyond_reason_is_refused() {
    let instance = Instance::start();
    let pane = instance.open("cat", Some("cat"));
    let mut app = instance.app();
    app.tell("attach", json!({"pane": pane}));
    app.until("the grid", |m| is(m, "grid", &pane));

    app.tell("resize", json!({"pane": pane, "cols": 5000, "rows": 5000, "width": 0, "height": 0}));
    let grid = app.until("the grid", |m| is(m, "grid", &pane));
    assert_eq!((grid["args"]["cols"].as_u64(), grid["args"]["rows"].as_u64()), (Some(80), Some(24)), "{grid}");

    app.tell("resize", json!({"pane": pane, "cols": 1000, "rows": 1000, "width": 0, "height": 0}));
    let grid = app.until("the grid", |m| is(m, "grid", &pane));
    assert_eq!((grid["args"]["cols"].as_u64(), grid["args"]["rows"].as_u64()), (Some(1000), Some(1000)), "{grid}");
}
