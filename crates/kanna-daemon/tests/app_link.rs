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

#[test]
fn pixel_sizes_reach_the_pty() {
    let instance = Instance::start();
    let pane = instance.open("sh", None);
    let mut app = instance.app();
    app.tell("attach", json!({"pane": pane}));
    app.tell("resize", json!({"pane": pane, "cols": 90, "rows": 30, "width": 900, "height": 600}));
    app.until("the grid", |m| is(m, "grid", &pane) && m["args"]["cols"] == 90);
    app.tell("resize", json!({"pane": pane, "cols": 90, "rows": 30, "width": 1800, "height": 1200}));
    app.until("the grid", |m| is(m, "grid", &pane));
    let probe = "python3 -c 'import fcntl,termios,struct;print(\"size\",*struct.unpack(\"HHHH\",fcntl.ioctl(0,termios.TIOCGWINSZ,bytes(8))))'";
    instance.call("send", json!({"pane": pane, "text": probe}));
    wait_for("the probe", || instance.screen(&pane).contains("size "));
    let screen = instance.screen(&pane);
    assert!(screen.contains("size 30 90 1800 1200"), "{screen}");
}

#[test]
fn an_attach_starts_the_view_over() {
    let instance = Instance::start();
    let pane = instance.open("hello", Some("printf hello; exec cat"));
    wait_for("the output", || instance.screen(&pane).contains("hello"));
    for _ in 0..2 {
        let mut app = instance.app();
        app.tell("attach", json!({"pane": pane}));
        let first = app.until("the replay", |m| is(m, "output", &pane));
        assert_eq!(data(&first), b"\x1bc", "{first}");
        let replay = app.until("the replay", |m| is(m, "output", &pane));
        assert_eq!(data(&replay), b"hello");
    }
}

#[test]
fn an_endless_line_drops_the_app() {
    let instance = Instance::start();
    let mut app = instance.app();
    let mut writer = app.stream.try_clone().unwrap();
    std::thread::spawn(move || {
        let chunk = vec![b'x'; 1 << 20];
        for _ in 0..64 {
            if writer.write_all(&chunk).is_err() {
                return;
            }
        }
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.next().is_some() {
        assert!(Instant::now() < deadline, "the daemon kept reading the line");
    }
    assert!(instance.rss_mb() < 100, "the daemon grew to {} MB", instance.rss_mb());
    wait_for("the daemon to let the app go", || instance.log().contains("the app went away"));
    instance.app();
}
