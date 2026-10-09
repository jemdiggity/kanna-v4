mod common;

use common::{data, is, wait_for, App, Instance};
use serde_json::json;
use std::io::Write;
use std::time::{Duration, Instant};

#[test]
fn a_flood_is_held_back_at_the_pty() {
    let mut instance = Instance::start();
    let pane = instance.open("flood", Some("yes"));
    std::thread::sleep(Duration::from_secs(3));
    let alone = instance.rss_mb();
    assert!(alone < 150, "with no app the daemon grew to {alone} MB");

    let mut app = instance.app();
    app.tell("attach", json!({"pane": pane}));
    app.until("the view to start", |m| is(m, "output", &pane) && data(m) == b"\x1bc");
    let deadline = Instant::now() + Duration::from_secs(60);
    while !instance.log().contains("the app is") {
        assert!(Instant::now() < deadline, "the app never fell behind");
        assert!(instance.rss_mb() < 150, "with a stalled app the daemon grew to {} MB", instance.rss_mb());
        std::thread::sleep(Duration::from_millis(200));
    }
    std::thread::sleep(Duration::from_secs(2));
    let stalled = instance.rss_mb();
    assert!(stalled < 150, "with a stalled app the daemon grew to {stalled} MB");

    app.until("the view to start over", |m| is(m, "output", &pane) && data(m).starts_with(b"\x1bc"));
    app.until("more output", |m| is(m, "output", &pane));

    let asked = Instant::now();
    unsafe { libc::kill(instance.pid(), libc::SIGTERM) };
    wait_for("the daemon to stop", || instance.daemon.try_wait().unwrap().is_some());
    assert!(asked.elapsed() < Duration::from_secs(2), "the daemon took {:?} to stop", asked.elapsed());
}

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
fn exited_comes_after_the_last_output() {
    let instance = Instance::start();
    for round in 0..5 {
        let name = format!("burst{round}");
        let pane = instance.open(&name, Some("sleep 0.3; head -c 2000000 /dev/zero | tr '\\0' x; printf END; exit 7"));
        let mut app = instance.app();
        app.tell("attach", json!({"pane": pane}));
        let mut seen = Vec::new();
        let exited = app.until("the exit", |m| {
            if is(m, "output", &pane) {
                seen.extend(data(m));
            }
            is(m, "exited", &pane)
        });
        assert_eq!(exited["args"]["code"], 7, "{exited}");
        assert!(seen.ends_with(b"END"), "round {round}: exited came before the last output ({} bytes, ending {:?})", seen.len(), String::from_utf8_lossy(&seen[seen.len().saturating_sub(20)..]));
        drop(app);
        instance.call("close", json!({"pane": pane}));
    }
}

#[test]
fn a_request_the_app_never_answers_times_out() {
    let instance = Instance::start_with(&[("KANNA_APP_TIMEOUT_MS", "500")]);
    let mut app = instance.app();
    let asked = Instant::now();
    let reply = instance.call("debug.app", json!({}));
    assert_eq!(reply["ok"], false, "{reply}");
    assert_eq!(reply["error"]["code"], "timeout", "{reply}");
    assert!(asked.elapsed() < Duration::from_secs(3));

    let request = app.until("the request", |m| m["cmd"] == "debug.app");
    app.send(json!({"id": request["id"], "ok": true, "late": true}));
    let mut second = App::raw(&instance.socket());
    second.send(json!({"id": 0, "cmd": "app.hello", "args": {}}));
    assert_eq!(second.next().unwrap()["error"]["code"], "app_attached");
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
fn a_closed_pane_lets_go_of_its_pty() {
    let instance = Instance::start();
    let pidfile = instance.dir.join("orphan.pid");
    let cmd = format!("perl -MPOSIX -e 'if (!fork) {{ setsid; open my $f, \">\", \"{}\"; print $f $$; close $f; sleep 60; exit }}'; sleep 60", pidfile.display());
    let pane = instance.open("orphan", Some(&cmd));
    wait_for("the orphan", || std::fs::read_to_string(&pidfile).is_ok_and(|p| !p.is_empty()));
    let orphan: i32 = std::fs::read_to_string(&pidfile).unwrap().trim().parse().unwrap();
    assert!(instance.ptys_open() > 0);
    instance.call("close", json!({"pane": pane}));
    let deadline = Instant::now() + Duration::from_secs(5);
    while instance.ptys_open() > 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    let left = instance.ptys_open();
    unsafe { libc::kill(orphan, libc::SIGKILL) };
    assert_eq!(left, 0, "the daemon still holds the closed pane's pty");
}
