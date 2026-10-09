//! Terminal panes: kanna-mux's `PaneHost` for the daemon. Each terminal is a
//! program on a pty (from a `Spawner`), its screen (a `Screen`, for `read`)
//! and its recent output (replayed to the app when it attaches). Everything
//! here runs on the daemon's main thread; a reader thread per pty posts its
//! output there as events.

use crate::pty::{self, Spawned, Spawner, Spec};
use crate::screen::{Screen, TextScreen};
use crate::Event;
use kanna_mux::{KmuxError, Pane, PaneHost, PaneState, WeakCore};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::panic::AssertUnwindSafe;
use std::sync::mpsc::{self, Sender};

/// How much recent output each terminal keeps for the app.
const REPLAY: usize = 1 << 20;

/// A terminal's size before anything has asked for one.
/// Told which pane started.
pub type OnStart = Box<dyn Fn(&str)>;

const COLS: u16 = 80;
const ROWS: u16 = 24;

struct Terminal {
    /// Which start of the pane this is: a restart is a new one, and events
    /// from an older one are ignored.
    serial: u64,
    pid: i32,
    master: OwnedFd,
    input: Sender<Vec<u8>>,
    screen: Box<dyn Screen>,
    replay: VecDeque<u8>,
    cols: u16,
    rows: u16,
    exit_code: Option<i32>,
}

pub struct Terminals {
    pub core: RefCell<Option<WeakCore>>,
    spawner: Box<dyn Spawner>,
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
    pub fn new(spawner: Box<dyn Spawner>, events: Sender<Event>, env: Vec<(String, String)>) -> Terminals {
        Terminals { core: RefCell::new(None), spawner, events, env, live: RefCell::default(), serial: Cell::new(0), sizes: RefCell::default(), on_start: RefCell::new(None) }
    }

    fn report(&self, id: &str, state: PaneState, exit_code: Option<i64>, error: Option<String>) {
        let Some(core) = self.core.borrow().as_ref().and_then(WeakCore::upgrade) else { return };
        let (id, reporter) = (id.to_string(), core.clone());
        core.later(move || reporter.update(&id, state, exit_code, error));
    }

    /// What a pane runs: its command through the user's shell, else the shell
    /// itself, as a login shell.
    fn spec(&self, pane: &Pane) -> Spec {
        let shell = std::env::var("SHELL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/bin/sh".into());
        let name = shell.rsplit('/').next().unwrap_or("sh").to_string();
        let (args, arg0) = match pane.command.as_deref().filter(|c| !c.is_empty()) {
            Some(command) => (vec!["-l".into(), "-c".into(), command.into()], None),
            None => (vec![], Some(format!("-{name}"))),
        };
        let mut env = self.env.clone();
        env.extend([("KANNA_PANE".into(), pane.id.clone()), ("TERM".into(), "xterm-256color".into()), ("COLORTERM".into(), "truecolor".into())]);
        let cwd = pane.cwd.clone().or_else(|| std::env::var("HOME").ok());
        let (cols, rows) = self.sizes.borrow().get(&pane.id).copied().unwrap_or((COLS, ROWS));
        Spec { program: shell, args, arg0, cwd, env, unset: vec!["KANNA_INSTANCE".into()], cols, rows }
    }

    // MARK: Events from the reader and wait threads

    /// Output from a pty. Returns whether it's from the pane's current terminal.
    pub fn output(&self, id: &str, serial: u64, bytes: &[u8]) -> bool {
        let mut live = self.live.borrow_mut();
        let Some(terminal) = live.get_mut(id).filter(|t| t.serial == serial) else { return false };
        guarded(id, terminal, |screen| screen.feed(bytes));
        terminal.replay.extend(bytes);
        let over = terminal.replay.len().saturating_sub(REPLAY);
        terminal.replay.drain(..over);
        true
    }

    /// A program ended. Returns whether it's the pane's current one.
    pub fn exited(&self, id: &str, serial: u64, code: i32) -> bool {
        let current = match self.live.borrow_mut().get_mut(id).filter(|t| t.serial == serial) {
            Some(terminal) => {
                terminal.exit_code = Some(code);
                true
            }
            None => false,
        };
        if current {
            self.report(id, PaneState::Exited, Some(code as i64), None);
        }
        current
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
        let (cols, rows) = (cols.max(1), rows.max(1));
        if (cols, rows) != (terminal.cols, terminal.rows) {
            pty::set_size(&terminal.master, cols, rows, width, height);
            (terminal.cols, terminal.rows) = (cols, rows);
            guarded(id, terminal, |screen| screen.resize(cols, rows));
            self.sizes.borrow_mut().insert(id.into(), (cols, rows));
        }
        Some((cols, rows))
    }

    /// What the app needs to show a terminal it attaches to: its size, its
    /// recent output, and its exit code if it has ended.
    pub fn attach(&self, id: &str) -> Option<(u16, u16, Vec<u8>, Option<i32>)> {
        let live = self.live.borrow();
        let t = live.get(id)?;
        Some((t.cols, t.rows, t.replay.iter().copied().collect(), t.exit_code))
    }

    pub fn hang_up_all(&self) {
        for (_, terminal) in self.live.borrow_mut().drain() {
            self.spawner.hang_up(terminal.pid);
        }
    }
}

/// Runs `f` on a terminal's screen. The screen reads untrusted output: if
/// it panics anyway, only that screen is lost (it starts over, empty), not
/// the daemon and its other terminals.
fn guarded<R>(id: &str, terminal: &mut Terminal, f: impl FnOnce(&mut dyn Screen) -> R) -> Option<R> {
    match std::panic::catch_unwind(AssertUnwindSafe(|| f(terminal.screen.as_mut()))) {
        Ok(result) => Some(result),
        Err(_) => {
            eprintln!("kanna-daemon: {id}'s screen failed; it starts over");
            terminal.screen = Box::new(TextScreen::new(terminal.cols, terminal.rows));
            None
        }
    }
}

impl PaneHost for Terminals {
    fn start(&self, pane: &Pane) {
        let serial = self.serial.get() + 1;
        self.serial.set(serial);
        let spec = self.spec(pane);
        let (id, events) = (pane.id.clone(), self.events.clone());
        let exited = Box::new(move |code| {
            let _ = events.send(Event::Exited { pane: id, serial, code });
        });
        let started = self.spawner.spawn(&spec, exited).and_then(|spawned| self.run(&pane.id, serial, spawned, &spec));
        match started {
            Ok(terminal) => {
                self.live.borrow_mut().insert(pane.id.clone(), terminal);
                if let Some(on_start) = self.on_start.borrow().as_ref() {
                    on_start(&pane.id);
                }
                self.report(&pane.id, PaneState::Running, None, None);
            }
            Err(error) => self.report(&pane.id, PaneState::Failed, None, Some(format!("could not start {}: {error}", spec.program))),
        }
    }

    fn stop(&self, pane: &Pane) {
        // Dropping it closes the pty and ends its writer thread.
        if let Some(terminal) = self.live.borrow_mut().remove(&pane.id) {
            self.spawner.hang_up(terminal.pid);
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
        let mut live = self.live.borrow_mut();
        let terminal = live.get_mut(&pane.id).ok_or_else(|| KmuxError::new("bad_request", format!("{} has no terminal", pane.label())))?;
        guarded(&pane.id, terminal, |screen| screen.text(lines)).ok_or_else(|| KmuxError::new("bad_request", format!("{}'s screen failed and was reset", pane.label())))
    }
}

impl Terminals {
    /// Starts the threads that read and write a new terminal's pty.
    fn run(&self, id: &str, serial: u64, spawned: Spawned, spec: &Spec) -> std::io::Result<Terminal> {
        let Spawned { pid, master } = spawned;
        let mut reader = File::from(master.try_clone()?);
        let mut writer = File::from(master.try_clone()?);
        let (pane, events) = (id.to_string(), self.events.clone());
        std::thread::Builder::new().name(format!("kanna.read.{id}")).spawn(move || {
            let mut buffer = vec![0u8; 65536];
            // Ends when the program and everything else holding the pty's
            // other side have gone (EIO), or the pane closed.
            while let Ok(n @ 1..) = reader.read(&mut buffer) {
                if events.send(Event::Output { pane: pane.clone(), serial, bytes: buffer[..n].to_vec() }).is_err() {
                    return;
                }
            }
        })?;
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
            screen: Box::new(TextScreen::new(spec.cols, spec.rows)),
            replay: VecDeque::new(),
            cols: spec.cols,
            rows: spec.rows,
            exit_code: None,
        })
    }
}
