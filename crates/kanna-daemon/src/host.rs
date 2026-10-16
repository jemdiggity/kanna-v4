use crate::app_link::AppLink;
use crate::diffs::Diffs;
use crate::terminals::Terminals;
use kanna_mux::{KmuxError, Pane, PaneHost, PaneType, Responder};
use std::cell::RefCell;
use std::rc::{Rc, Weak};

pub struct Host {
    terminals: Rc<Terminals>,
    diffs: Rc<Diffs>,
    app: RefCell<Weak<AppLink>>,
}

impl Host {
    pub fn new(terminals: Rc<Terminals>, diffs: Rc<Diffs>) -> Host {
        Host { terminals, diffs, app: RefCell::new(Weak::new()) }
    }

    pub fn connect(&self, app: &Rc<AppLink>) {
        *self.app.borrow_mut() = Rc::downgrade(app);
    }

    fn app(&self) -> Option<Rc<AppLink>> {
        self.app.borrow().upgrade()
    }
}

impl PaneHost for Host {
    fn start(&self, pane: &Pane) {
        match pane.kind {
            PaneType::Term => self.terminals.start(pane),
            _ => {
                if pane.kind == PaneType::Diff {
                    self.diffs.watch(pane);
                }
                if let Some(app) = self.app() {
                    app.show(pane);
                }
            }
        }
    }

    fn stop(&self, pane: &Pane) {
        match pane.kind {
            PaneType::Term => self.terminals.stop(pane),
            _ => {
                if pane.kind == PaneType::Diff {
                    self.diffs.stop(&pane.id);
                }
                if let Some(app) = self.app() {
                    app.hide(&pane.id);
                }
            }
        }
    }

    fn end(&self, pane: &Pane) {
        if pane.kind == PaneType::Diff {
            self.diffs.stop(&pane.id);
        }
        if pane.kind == PaneType::Term {
            self.terminals.end(pane);
        }
    }

    fn send(&self, pane: &Pane, text: &str) {
        self.terminals.send(pane, text);
    }

    fn send_keys(&self, pane: &Pane, keys: Vec<String>, done: Responder) {
        self.terminals.send_keys(pane, keys, done);
    }

    fn read(&self, pane: &Pane, lines: Option<usize>) -> Result<String, KmuxError> {
        self.terminals.read(pane, lines)
    }
}
