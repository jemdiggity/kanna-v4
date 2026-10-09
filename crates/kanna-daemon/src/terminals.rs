//! Terminal panes: kanna-mux's `PaneHost` for the daemon. Each terminal is a
//! program on a pty (from a `Spawner`), its screen (a `Screen`, for `read`)
//! and its recent output (replayed to the app when it attaches). Everything
//! here runs on the daemon's main thread; a reader thread per pty posts its
//! output there as events.

use crate::pty::{self, Spawned, Spawner, Spec};
use crate::reader::{Feed, Reader, Taken};
use crate::Event;
use kanna_mux::{KmuxError, Pane, PaneHost, PaneState, WeakCore};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::os::fd::OwnedFd;
use std::rc::Rc;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

/// A terminal's size before anything has asked for one.
/// Told which pane started.
pub type OnStart = Box<dyn Fn(&str)>;

const COLS: u16 = 80;
const ROWS: u16 = 24;

const MAX_SIDE: u16 = 1000;

struct Terminal {
    /// Which start of the pane this is: a restart is a new one, and events
    /// from an older one are ignored.
    serial: u64,
    pid: i32,
    master: OwnedFd,
    input: Sender<Vec<u8>>,
    feed: Arc<Mutex<Feed>>,
    reader: Reader,
    pixels: (u16, u16),
    exit_code: Option<i32>,
    exit_reported: bool,
    drained: bool,
}

pub struct Terminals {
    pub core: RefCell<Option<WeakCore>>,
    spawner: Rc<dyn Spawner>,
    events: Sender<Event>,
    /// Every terminal's environment gets these (`KANNA_SOCKET`).
    env: Vec<(String, String)>,
    live: RefCell<HashMap<String, Terminal>>,
    serial: Cell<u64>,
    /// Each pane's last size, kept for a restart.
    sizes: RefCell<HashMap<String, (u16, u16)>>,
    /// Called when a pane's terminal starts (again, on `restart`).
    pub on_start: RefCell<Option<OnStart>>,
}

impl Terminals {
    pub fn new(spawner: Rc<dyn Spawner>, events: Sender<Event>, env: Vec<(String, String)>) -> Terminals {
        Terminals { core: RefCell::new(None), spawner, events, env, live: RefCell::default(), serial: Cell::new(0), sizes: RefCell::default(), on_start: RefCell::new(None) }
    }

    fn report(&self, id: &str, state: PaneState, exit_code: Option<i64>, error: Option<String>) {
        let Some(core) = self.core.borrow().as_ref().and_then(WeakCore::upgrade) else { return };
        let (id, reporter) = (id.to_string(), core.clone());
        core.later(move || reporter.update(&id, state, exit_code, error));
    }

    fn report_running(&self, id: &str, pid: i32) {
        let Some(core) = self.core.borrow().as_ref().and_then(WeakCore::upgrade) else { return };
        let (id, reporter) = (id.to_string(), core.clone());
        core.later(move || {
            reporter.set_pid(&id, Some(pid as i64));
            reporter.update(&id, PaneState::Running, None, None);
        });
    }

    fn spec(&self, pane: &Pane) -> Spec {
        let mut env: Vec<(String, Option<String>)> = self.env.iter().map(|(k, v)| (k.clone(), Some(v.clone()))).collect();
        env.push(("KANNA_PANE".into(), Some(pane.id.clone())));
        env.push(("KANNA_INSTANCE".into(), None));
        let (cols, rows) = self.sizes.borrow().get(&pane.id).copied().unwrap_or((COLS, ROWS));
        let command = pane.command.clone().filter(|c| !c.is_empty());
        Spec { id: pane.id.clone(), command, cwd: pane.cwd.clone(), env, cols, rows }
    }

    // MARK: Events from the reader and wait threads

    pub fn output(&self, id: &str, serial: u64) -> Option<Taken> {
        let live = self.live.borrow();
        let terminal = live.get(id).filter(|t| t.serial == serial)?;
        let taken = Feed::lock(&terminal.feed).take();
        taken
    }

    pub fn exited(&self, id: &str, pid: i32, code: i32) -> Option<i32> {
        let drained = match self.live.borrow_mut().get_mut(id).filter(|t| t.pid == pid && t.exit_code.is_none()) {
            Some(terminal) => {
                terminal.exit_code = Some(code);
                if !terminal.drained {
                    terminal.reader.drain();
                }
                Some(terminal.drained)
            }
            None => None,
        };
        self.spawner.acknowledge_exit(id);
        match drained {
            Some(true) => self.report_exit(id),
            _ => None,
        }
    }

    pub fn drained(&self, id: &str, serial: u64) -> Option<i32> {
        self.live.borrow_mut().get_mut(id).filter(|t| t.serial == serial)?.drained = true;
        self.report_exit(id)
    }

    fn report_exit(&self, id: &str) -> Option<i32> {
        let code = {
            let mut live = self.live.borrow_mut();
            let terminal = live.get_mut(id).filter(|t| t.drained && !t.exit_reported)?;
            let code = terminal.exit_code?;
            terminal.exit_reported = true;
            code
        };
        self.report(id, PaneState::Exited, Some(code as i64), None);
        Some(code)
    }

    // MARK: For the app

    /// Keystrokes and other input, as the app's terminal view wrote them.
    pub fn write(&self, id: &str, bytes: Vec<u8>) {
        if let Some(terminal) = self.live.borrow().get(id).filter(|t| t.exit_code.is_none()) {
            let _ = terminal.input.send(bytes);
        }
    }

    /// Sets a terminal's size; returns the size it now has.
    pub fn resize(&self, id: &str, cols: u16, rows: u16, width: u16, height: u16) -> Option<(u16, u16)> {
        let mut live = self.live.borrow_mut();
        let terminal = live.get_mut(id)?;
        let mut feed = Feed::lock(&terminal.feed);
        if cols > MAX_SIDE || rows > MAX_SIDE {
            eprintln!("kanna-daemon: refused to make {id} {cols}×{rows}: at most {MAX_SIDE} cells a side");
            return Some((feed.cols, feed.rows));
        }
        let (cols, rows) = (cols.max(1), rows.max(1));
        if (cols, rows, (width, height)) != (feed.cols, feed.rows, terminal.pixels) {
            pty::set_size(&terminal.master, cols, rows, width, height);
            terminal.pixels = (width, height);
        }
        if (cols, rows) != (feed.cols, feed.rows) {
            feed.resize(id, cols, rows);
            self.sizes.borrow_mut().insert(id.into(), (cols, rows));
        }
        Some((cols, rows))
    }

    /// What the app needs to show a terminal it attaches to: its size, its
    /// recent output, and its exit code if it has ended.
    pub fn attach(&self, id: &str) -> Option<(u16, u16, Vec<u8>, Option<i32>)> {
        let live = self.live.borrow();
        let t = live.get(id)?;
        let mut feed = Feed::lock(&t.feed);
        let replay = feed.snapshot();
        Some((feed.cols, feed.rows, replay, t.exit_code.filter(|_| t.exit_reported)))
    }

    pub fn size(&self, id: &str) -> Option<(u16, u16)> {
        let live = self.live.borrow();
        let feed = Feed::lock(&live.get(id)?.feed);
        Some((feed.cols, feed.rows))
    }

    pub fn close_all(&self) {
        let ids: Vec<String> = self.live.borrow_mut().drain().filter(|(_, t)| t.exit_code.is_none()).map(|(id, _)| id).collect();
        for id in ids {
            self.spawner.close(&id);
        }
    }
}

impl PaneHost for Terminals {
    fn start(&self, pane: &Pane) {
        let serial = self.serial.get() + 1;
        self.serial.set(serial);
        let spec = self.spec(pane);
        let started = self.spawner.spawn(&spec).and_then(|spawned| self.run(&pane.id, serial, spawned, &spec));
        match started {
            Ok(terminal) => {
                let pid = terminal.pid;
                self.live.borrow_mut().insert(pane.id.clone(), terminal);
                if let Some(on_start) = self.on_start.borrow().as_ref() {
                    on_start(&pane.id);
                }
                self.report_running(&pane.id, pid);
            }
            Err(error) => {
                let what = spec.command.as_deref().unwrap_or("the shell");
                self.report(&pane.id, PaneState::Failed, None, Some(format!("could not start {what}: {error}")))
            }
        }
    }

    fn stop(&self, pane: &Pane) {
        // Dropping it closes the pty and ends its writer thread.
        let removed = self.live.borrow_mut().remove(&pane.id);
        if removed.is_some_and(|terminal| terminal.exit_code.is_none()) {
            self.spawner.close(&pane.id);
        }
        // A closed pane never comes back; a restarted one starts at once.
        let restarting = self.core.borrow().as_ref().and_then(WeakCore::upgrade).is_some_and(|core| core.pane(&pane.id).is_some());
        if !restarting {
            self.sizes.borrow_mut().remove(&pane.id);
        }
    }

    fn send(&self, pane: &Pane, text: &str) {
        self.write(&pane.id, format!("{text}\r").into_bytes());
    }

    fn read(&self, pane: &Pane, lines: Option<usize>) -> Result<String, KmuxError> {
        let live = self.live.borrow();
        let terminal = live.get(&pane.id).ok_or_else(|| KmuxError::new("bad_request", format!("{} has no terminal", pane.label())))?;
        let text = Feed::lock(&terminal.feed).guarded(&pane.id, |screen| screen.text(lines));
        text.ok_or_else(|| KmuxError::new("bad_request", format!("{}'s screen failed and was reset", pane.label())))
    }
}

impl Terminals {
    /// Starts the threads that read and write a new terminal's pty.
    fn run(&self, id: &str, serial: u64, spawned: Spawned, spec: &Spec) -> std::io::Result<Terminal> {
        let Spawned { pid, master } = spawned;
        let feed = Feed::new(spec.cols, spec.rows);
        let reader = Reader::start(id, serial, master.try_clone()?, feed.clone(), self.events.clone())?;
        let mut writer = File::from(master.try_clone()?);
        let (input, inputs) = mpsc::channel::<Vec<u8>>();
        std::thread::Builder::new().name(format!("kanna.write.{id}")).spawn(move || {
            for bytes in inputs {
                if writer.write_all(&bytes).is_err() {
                    return;
                }
            }
        })?;
        Ok(Terminal {
            serial,
            pid,
            master,
            input,
            feed,
            reader,
            pixels: (0, 0),
            exit_code: None,
            exit_reported: false,
            drained: false,
        })
    }
}
