use crate::pty::{self, Adopted, Spawned, Spawner, Spec};
use crate::reader::{Feed, Reader, Taken};
use crate::store::{pane_number, Record, Saved, Store};
use crate::Event;
use kanna_mux::{KmuxError, Pane, PaneHost, PaneState, PaneType, WeakCore};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::os::fd::OwnedFd;
use std::rc::Rc;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

pub type OnStart = Box<dyn Fn(&str)>;

pub struct Attach {
    pub cols: u16,
    pub rows: u16,
    pub snapshot: Result<Vec<u8>, kanna_vt::Error>,
    pub exit_code: Option<i32>,
}

const COLS: u16 = 80;
const ROWS: u16 = 24;

const MAX_SIDE: u16 = 1000;

struct Pty {
    master: OwnedFd,
    input: Sender<Vec<u8>>,
    reader: Reader,
}

struct Terminal {
    serial: u64,
    pid: i32,
    keeper: String,
    pty: Option<Pty>,
    feed: Arc<Mutex<Feed>>,
    pixels: (u16, u16),
    exit_code: Option<i32>,
    exit_reported: bool,
    drained: bool,
}

impl Terminal {
    fn running(&self) -> Option<&Pty> {
        self.pty.as_ref().filter(|_| self.exit_code.is_none())
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct Restored {
    pub adopted: usize,
    pub unknown: usize,
    pub exited: usize,
    pub dropped: usize,
}

pub struct Terminals {
    pub core: RefCell<Option<WeakCore>>,
    spawner: Rc<dyn Spawner>,
    store: Store,
    events: Sender<Event>,
    env: Vec<(String, String)>,
    live: RefCell<HashMap<String, Terminal>>,
    serial: Cell<u64>,
    sizes: RefCell<HashMap<String, (u16, u16)>>,
    changed: Cell<bool>,
    pub on_start: RefCell<Option<OnStart>>,
}

impl Terminals {
    pub fn new(spawner: Rc<dyn Spawner>, store: Store, events: Sender<Event>, env: Vec<(String, String)>) -> Terminals {
        Terminals {
            core: RefCell::new(None),
            spawner,
            store,
            events,
            env,
            live: RefCell::default(),
            serial: Cell::new(0),
            sizes: RefCell::default(),
            changed: Cell::new(false),
            on_start: RefCell::new(None),
        }
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
        let mut env: Vec<(String, Option<String>)> = vec![("KANNA_INSTANCE".into(), None)];
        env.extend(self.env.iter().map(|(k, v)| (k.clone(), Some(v.clone()))));
        env.push(("KANNA_PANE".into(), Some(pane.id.clone())));
        let (cols, rows) = self.sizes.borrow().get(&pane.id).copied().unwrap_or((COLS, ROWS));
        let command = pane.command.clone().filter(|c| !c.is_empty());
        Spec { id: pane.id.clone(), command, cwd: pane.cwd.clone(), env, cols, rows }
    }

    fn next_serial(&self) -> u64 {
        let serial = self.serial.get() + 1;
        self.serial.set(serial);
        serial
    }

    pub fn output(&self, id: &str, serial: u64) -> Option<Taken> {
        let live = self.live.borrow();
        let terminal = live.get(id).filter(|t| t.serial == serial)?;
        let taken = Feed::lock(&terminal.feed).take();
        taken
    }

    pub fn exited(&self, keeper: &str, pid: i32, code: i32) -> Option<(String, i32)> {
        let pane = {
            let mut live = self.live.borrow_mut();
            let found = live.iter_mut().find(|(_, t)| t.keeper == keeper && t.pid == pid && t.exit_code.is_none() && t.pty.is_some());
            found.map(|(id, terminal)| {
                terminal.exit_code = Some(code);
                if !terminal.drained {
                    if let Some(pty) = &terminal.pty {
                        pty.reader.drain();
                    }
                }
                (id.clone(), terminal.drained)
            })
        };
        if pane.is_some() {
            self.changed.set(true);
            self.save_if_changed();
        }
        self.spawner.acknowledge_exit(keeper);
        match pane {
            Some((id, true)) => self.report_exit(&id).map(|code| (id, code)),
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

    pub fn write(&self, id: &str, bytes: Vec<u8>) {
        if let Some(pty) = self.live.borrow().get(id).and_then(Terminal::running) {
            let _ = pty.input.send(bytes);
        }
    }

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
            if let Some(pty) = &terminal.pty {
                pty::set_size(&pty.master, cols, rows, width, height);
            }
            terminal.pixels = (width, height);
            self.sizes.borrow_mut().insert(id.into(), (cols, rows));
            let replies = feed.resize(id, cols, rows, width, height);
            if let (false, Some(pty)) = (replies.is_empty(), terminal.running()) {
                let _ = pty.input.send(replies);
            }
        }
        Some((cols, rows))
    }

    pub fn attach(&self, id: &str) -> Option<Attach> {
        let live = self.live.borrow();
        let t = live.get(id)?;
        let mut feed = Feed::lock(&t.feed);
        let snapshot = feed.snapshot();
        Some(Attach { cols: feed.cols, rows: feed.rows, snapshot, exit_code: t.exit_code.filter(|_| t.exit_reported) })
    }

    pub fn mark_changed(&self) {
        self.changed.set(true);
    }

    pub fn save_if_changed(&self) {
        if !self.changed.replace(false) {
            return;
        }
        let Some(saved) = self.saved() else { return };
        if let Err(error) = self.store.save(&saved) {
            eprintln!("kanna-daemon: couldn't save the terminals to {}: {error}", self.store.dir().display());
        }
    }

    fn saved(&self) -> Option<Saved> {
        let core = self.core.borrow().as_ref().and_then(WeakCore::upgrade)?;
        let live = self.live.borrow();
        Some(core.with_panes(|panes| Saved {
            counter: panes.counter(),
            records: panes
                .iter()
                .filter(|pane| pane.kind == PaneType::Term)
                .filter_map(|pane| {
                    let terminal = live.get(&pane.id)?;
                    let (state, exit_code) = match terminal.exit_code {
                        Some(code) => (PaneState::Exited, Some(code as i64)),
                        None => (PaneState::Running, None),
                    };
                    Some(Record {
                        id: pane.id.clone(),
                        name: pane.name.clone(),
                        cmd: pane.command.clone(),
                        cwd: pane.cwd.clone(),
                        state,
                        exit_code,
                        pid: terminal.pid as i64,
                        keeper: terminal.keeper.clone(),
                    })
                })
                .collect(),
        }))
    }

    pub fn shutdown(&self) {
        let mut live = self.live.borrow_mut();
        for pty in live.values().filter_map(|t| t.pty.as_ref()) {
            pty.reader.stop();
        }
        for pty in live.values_mut().filter_map(|t| t.pty.as_mut()) {
            pty.reader.join();
        }
        drop(live);
        self.changed.set(true);
        self.save_if_changed();
        self.store.flush();
        self.store.stop();
    }

    pub fn restore(&self, adopted: Vec<Adopted>) -> Restored {
        let saved = self.store.load();
        let mut counter = saved.counter;
        for id in saved.records.iter().flat_map(|r| [&r.id, &r.keeper]).chain(adopted.iter().map(|a| &a.id)) {
            counter = counter.max(pane_number(id).unwrap_or(0));
        }
        let mut ptys: HashMap<String, Adopted> = adopted.into_iter().map(|pty| (pty.id.clone(), pty)).collect();
        let mut restored = Restored::default();
        let mut panes = Vec::new();
        let mut exits = Vec::new();
        for record in saved.records {
            let mut pane = Pane::new(&record.id, record.name.clone(), PaneType::Term);
            (pane.command, pane.cwd, pane.pid) = (record.cmd.clone(), record.cwd.clone(), Some(record.pid));
            let terminal = match ptys.remove(&record.keeper) {
                Some(pty) => {
                    let snapshot = (i64::from(pty.pid) == record.pid).then(|| self.store.snapshot(&record.id)).flatten();
                    pane.pid = Some(pty.pid as i64);
                    pane.state = PaneState::Running;
                    exits.extend(pty.exit.map(|code| (record.keeper.clone(), pty.pid, code)));
                    restored.adopted += 1;
                    self.adopt(&record.id, &record.keeper, pty, snapshot)
                }
                None if record.state == PaneState::Exited => {
                    pane.state = PaneState::Exited;
                    pane.exit_code = record.exit_code;
                    restored.exited += 1;
                    self.ended(&record, self.store.snapshot(&record.id))
                }
                None => {
                    eprintln!("kanna-daemon: {} ended while no daemon was running, and the keeper has no exit for it", pane.label());
                    restored.dropped += 1;
                    continue;
                }
            };
            match terminal {
                Ok(terminal) => {
                    self.live.borrow_mut().insert(record.id.clone(), terminal);
                    panes.push(pane);
                }
                Err(error) => eprintln!("kanna-daemon: couldn't take {} back: {error}", pane.label()),
            }
        }
        let mut unknown: Vec<Adopted> = ptys.into_values().collect();
        unknown.sort_by_key(|pty| (pane_number(&pty.id).unwrap_or(usize::MAX), pty.id.clone()));
        for pty in unknown {
            let id = match pane_number(&pty.id) {
                Some(_) if !self.live.borrow().contains_key(&pty.id) => pty.id.clone(),
                _ => {
                    counter += 1;
                    format!("p{counter}")
                }
            };
            let mut pane = Pane::new(&id, None, PaneType::Term);
            pane.command = match pty.cmd.as_slice() {
                [_, l, c, command] if l == "-l" && c == "-c" => Some(command.clone()),
                [sh, c, command] if sh == "/bin/sh" && c == "-c" => Some(command.clone()),
                _ => None,
            };
            pane.cwd = Some(pty.cwd.clone()).filter(|cwd| !cwd.is_empty());
            pane.pid = Some(pty.pid as i64);
            pane.state = PaneState::Running;
            let keeper = pty.id.clone();
            exits.extend(pty.exit.map(|code| (keeper.clone(), pty.pid, code)));
            match self.adopt(&id, &keeper, pty, None) {
                Ok(terminal) => {
                    eprintln!("kanna-daemon: took back {id}, which the keeper had and the disk didn't know");
                    self.live.borrow_mut().insert(id, terminal);
                    panes.push(pane);
                    restored.unknown += 1;
                }
                Err(error) => {
                    eprintln!("kanna-daemon: couldn't take back the keeper's {keeper}: {error}; closing it");
                    self.spawner.close(&keeper);
                }
            }
        }
        if let Some(core) = self.core.borrow().as_ref().and_then(WeakCore::upgrade) {
            core.install_panes(panes, counter);
        }
        let keep: Vec<String> = self.live.borrow().keys().cloned().collect();
        self.store.prune(&keep);
        self.changed.set(true);
        self.save_if_changed();
        for (keeper, pid, code) in exits {
            if let Some((pane, _)) = self.exited(&keeper, pid, code) {
                eprintln!("kanna-daemon: {pane} exited ({code}) while no daemon was running");
            }
        }
        restored
    }

    fn adopt(&self, id: &str, keeper: &str, pty: Adopted, snapshot: Option<Vec<u8>>) -> std::io::Result<Terminal> {
        let restored = snapshot.and_then(|snapshot| match Feed::restore(&snapshot) {
            Ok(feed) => Some(feed),
            Err(error) => {
                eprintln!("kanna-daemon: {id}'s saved screen can't be read ({error}); starting it blank");
                None
            }
        });
        let fresh = restored.is_none();
        let feed = match restored {
            Some(feed) => feed,
            None => Feed::new(COLS, ROWS)?,
        };
        let (cols, rows) = {
            let mut screen = Feed::lock(&feed);
            let (cols, rows) = pty::size(&pty.master).unwrap_or((screen.cols, screen.rows));
            let (cols, rows) = (cols.min(MAX_SIDE), rows.min(MAX_SIDE));
            if (cols, rows) != (screen.cols, screen.rows) {
                screen.resize(id, cols, rows, 0, 0);
            }
            screen.unsaved |= fresh;
            (cols, rows)
        };
        pty::set_size(&pty.master, cols, rows, 0, 0);
        self.sizes.borrow_mut().insert(id.into(), (cols, rows));
        let serial = self.next_serial();
        let terminal = self.run(id, keeper, serial, Spawned { pid: pty.pid, master: pty.master }, feed)?;
        if let (None, Some(running)) = (pty.exit, &terminal.pty) {
            pty::redraw(&running.master);
        }
        Ok(terminal)
    }

    fn ended(&self, record: &Record, snapshot: Option<Vec<u8>>) -> std::io::Result<Terminal> {
        let feed = match snapshot.map(|snapshot| Feed::restore(&snapshot)) {
            Some(Ok(feed)) => feed,
            _ => Feed::new(COLS, ROWS)?,
        };
        let (cols, rows) = {
            let screen = Feed::lock(&feed);
            (screen.cols, screen.rows)
        };
        self.sizes.borrow_mut().insert(record.id.clone(), (cols, rows));
        self.store.track(&record.id, feed.clone());
        Ok(Terminal {
            serial: self.next_serial(),
            pid: record.pid as i32,
            keeper: record.keeper.clone(),
            pty: None,
            feed,
            pixels: (0, 0),
            exit_code: Some(record.exit_code.unwrap_or(-1) as i32),
            exit_reported: true,
            drained: true,
        })
    }
}

impl PaneHost for Terminals {
    fn start(&self, pane: &Pane) {
        let serial = self.next_serial();
        let spec = self.spec(pane);
        let started = self.spawner.spawn(&spec).and_then(|spawned| {
            let feed = Feed::new(spec.cols, spec.rows)?;
            self.run(&pane.id, &pane.id, serial, spawned, feed)
        });
        match started {
            Ok(terminal) => {
                let pid = terminal.pid;
                self.live.borrow_mut().insert(pane.id.clone(), terminal);
                self.changed.set(true);
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
        let removed = self.live.borrow_mut().remove(&pane.id);
        if let Some(terminal) = removed {
            if terminal.running().is_some() {
                self.spawner.close(&terminal.keeper);
            }
            self.store.forget(&pane.id);
            self.changed.set(true);
        }
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
        let text = Feed::lock(&terminal.feed).text(lines);
        Ok(text)
    }
}

impl Terminals {
    fn run(&self, id: &str, keeper: &str, serial: u64, spawned: Spawned, feed: Arc<Mutex<Feed>>) -> std::io::Result<Terminal> {
        let Spawned { pid, master } = spawned;
        let (input, inputs) = mpsc::channel::<Vec<u8>>();
        let reader = Reader::start(id, serial, master.try_clone()?, feed.clone(), input.clone(), self.events.clone())?;
        let mut writer = File::from(master.try_clone()?);
        std::thread::Builder::new().name(format!("kanna.write.{id}")).spawn(move || {
            for bytes in inputs {
                if writer.write_all(&bytes).is_err() {
                    return;
                }
            }
        })?;
        self.store.track(id, feed.clone());
        Ok(Terminal {
            serial,
            pid,
            keeper: keeper.into(),
            pty: Some(Pty { master, input, reader }),
            feed,
            pixels: (0, 0),
            exit_code: None,
            exit_reported: false,
            drained: false,
        })
    }
}
