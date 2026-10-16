mod support;

use kanna_mux::json::{json, Value};
use kanna_mux::{Core, KmuxError, Pane, PaneHost, PaneState, Responder, WeakCore};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Default)]
struct Screens {
    core: RefCell<Option<WeakCore>>,
}

impl PaneHost for Screens {
    fn start(&self, pane: &Pane) {
        let Some(core) = self.core.borrow().as_ref().and_then(WeakCore::upgrade) else { return };
        let (id, reporter) = (pane.id.clone(), core.clone());
        core.later(move || reporter.update(&id, PaneState::Running, None, None));
    }
    fn stop(&self, _: &Pane) {}
    fn send(&self, _: &Pane, _: &str, done: Responder) {
        done.reply(Ok(Default::default()));
    }
    fn read(&self, pane: &Pane, lines: Option<usize>) -> Result<String, KmuxError> {
        Ok(format!("screen of {} ({lines:?})", pane.id))
    }
}

fn core() -> Core {
    let core = Core::default();
    let host = Rc::new(Screens::default());
    *host.core.borrow_mut() = Some(core.weak());
    core.set_host(Some(host));
    core
}

fn error(reply: &Value) -> &str {
    reply["error"]["code"].as_str().unwrap_or("")
}

#[test]
fn reads_a_terminal_by_name_or_id() {
    let core = core();
    core.handle_blocking(json!({ "cmd": "open", "args": { "type": "term", "name": "shell" } }));
    let reply = core.handle_blocking(json!({ "cmd": "read", "args": { "pane": "shell" } }));
    assert_eq!(reply["text"], "screen of p1 (None)");
    let reply = core.handle_blocking(json!({ "cmd": "read", "args": { "pane": "p1", "lines": 5 } }));
    assert_eq!(reply["text"], "screen of p1 (Some(5))");
    let commands = core.handle_blocking(json!({ "cmd": "capabilities" }))["commands"].clone();
    assert!(commands.as_array().unwrap().contains(&json!("read")));
}

#[test]
fn read_checks_its_request() {
    let core = core();
    core.handle_blocking(json!({ "cmd": "open", "args": { "type": "md", "path": "/tmp/a.md", "name": "doc" } }));
    core.handle_blocking(json!({ "cmd": "open", "args": { "type": "term", "name": "shell" } }));
    assert_eq!(error(&core.handle_blocking(json!({ "cmd": "read", "args": {} }))), "bad_request");
    assert_eq!(error(&core.handle_blocking(json!({ "cmd": "read", "args": { "pane": "nope" } }))), "not_found");
    assert_eq!(error(&core.handle_blocking(json!({ "cmd": "read", "args": { "pane": "doc" } }))), "wrong_type");
    for lines in [json!(0), json!(1.5), json!("3")] {
        assert_eq!(error(&core.handle_blocking(json!({ "cmd": "read", "args": { "pane": "shell", "lines": lines } }))), "bad_request");
    }
}

#[test]
fn hosts_without_screens_say_so() {
    let (core, _) = support::make_core();
    core.handle_blocking(json!({ "cmd": "open", "args": { "type": "term" } }));
    assert_eq!(error(&core.handle_blocking(json!({ "cmd": "read", "args": { "pane": "p1" } }))), "unsupported");
}
