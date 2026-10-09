#![allow(dead_code)]

use kanna_mux::json::Value;
use kanna_mux::{Core, Pane, PaneHost, PaneState, WeakCore};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

#[derive(Default)]
pub struct FakeHost {
    pub core: RefCell<Option<WeakCore>>,
    pub fail: RefCell<HashSet<String>>,
    pub stopped: RefCell<Vec<String>>,
    pub sent: RefCell<Vec<String>>,
}

impl PaneHost for FakeHost {
    fn start(&self, pane: &Pane) {
        let failing = self.fail.borrow().contains(pane.command.as_deref().unwrap_or(""));
        let Some(core) = self.core.borrow().as_ref().and_then(WeakCore::upgrade) else { return };
        let (id, reporter) = (pane.id.clone(), core.clone());
        core.later(move || {
            if failing {
                reporter.update(&id, PaneState::Failed, None, Some("command not found".into()));
            } else {
                reporter.update(&id, PaneState::Running, None, None);
            }
        });
    }

    fn stop(&self, pane: &Pane) {
        self.stopped.borrow_mut().push(pane.id.clone());
    }

    fn send(&self, pane: &Pane, text: &str) {
        self.sent.borrow_mut().push(format!("{}: {text}", pane.id));
    }
}

pub fn make_core() -> (Core, Rc<FakeHost>) {
    let core = Core::default();
    let host = Rc::new(FakeHost::default());
    *host.core.borrow_mut() = Some(core.weak());
    core.set_host(Some(host.clone()));
    (core, host)
}

pub fn send(core: &Core, request: Value) -> Value {
    core.handle_blocking(request)
}
