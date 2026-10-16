use crate::Event;
use kanna_mux::json::{string, Map, Value};
use kanna_mux::{CoreHandle, KmuxError, Pane};
use notify::{RecursiveMode, Watcher};
use serde_json::json;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

const QUIET: Duration = Duration::from_millis(200);

enum Control {
    Show(HashSet<String>),
    Changed(Vec<PathBuf>),
    Stop,
}

struct Watch {
    control: Sender<Control>,
    show: HashSet<String>,
    diff: Option<Value>,
    serial: u64,
    updating: bool,
}

pub struct Diffs {
    events: Sender<Event>,
    watches: RefCell<HashMap<String, Watch>>,
    pub on_diff: RefCell<Option<Box<dyn Fn(&str, &Value)>>>,
}

impl Diffs {
    pub fn new(events: Sender<Event>) -> Diffs {
        Diffs { events, watches: RefCell::new(HashMap::new()), on_diff: RefCell::new(None) }
    }

    pub fn install(self: &Rc<Self>, core: &CoreHandle) {
        let diffs = Rc::downgrade(self);
        let weak = core.weak();
        core.add_command(
            "diff",
            Rc::new(move |args, responder| {
                let Some(diffs) = diffs.upgrade() else { return responder.reply(Err(KmuxError::new("app_gone", "the daemon is stopping"))) };
                let pane = string(&args, "pane").and_then(|p| weak.upgrade()?.pane(p)).map(|p| p.id);
                responder.reply(diffs.command(pane.as_deref(), &args))
            }),
        );
    }

    fn command(&self, pane: Option<&str>, args: &Value) -> Result<Map, KmuxError> {
        let named = string(args, "pane").ok_or_else(|| KmuxError::new("bad_request", "missing pane"))?;
        let pane = pane.ok_or_else(|| KmuxError::new("not_found", format!("no pane \"{named}\"")))?;
        let mut watches = self.watches.borrow_mut();
        let watch = watches.get_mut(pane).ok_or_else(|| KmuxError::new("not_found", format!("{pane} is not a diff pane that's running")))?;
        if let Some(show) = args.get("show").and_then(Value::as_array) {
            let show: HashSet<String> = show.iter().filter_map(Value::as_str).map(String::from).collect();
            if show != watch.show {
                watch.show = show.clone();
                watch.updating = true;
                let _ = watch.control.send(Control::Show(show));
            }
        }
        let mut out = Map::new();
        out.insert("pane".into(), json!(pane));
        out.insert("serial".into(), json!(watch.serial));
        out.insert("updating".into(), json!(watch.updating));
        out.insert("diff".into(), watch.diff.clone().unwrap_or(Value::Null));
        Ok(out)
    }

    pub fn watch(&self, pane: &Pane) {
        self.stop(&pane.id);
        let (control, inbox) = mpsc::channel();
        let folder = pane.path.clone().unwrap_or_default();
        let args = pane.diff.clone().unwrap_or_default();
        let id = pane.id.clone();
        let events = self.events.clone();
        let changes = control.clone();
        let _ = std::thread::Builder::new().name(format!("kanna.diff.{id}")).spawn(move || work(id, folder, args, inbox, changes, events));
        self.watches.borrow_mut().insert(pane.id.clone(), Watch { control, show: HashSet::new(), diff: None, serial: 0, updating: true });
    }

    pub fn stop(&self, pane: &str) {
        if let Some(watch) = self.watches.borrow_mut().remove(pane) {
            let _ = watch.control.send(Control::Stop);
        }
    }

    pub fn updating(&self, pane: &str) {
        if let Some(watch) = self.watches.borrow_mut().get_mut(pane) {
            watch.updating = true;
        }
        if let Some(on_diff) = self.on_diff.borrow().as_ref() {
            on_diff(pane, &json!({ "pane": pane, "updating": true }));
        }
    }

    pub fn loaded(&self, pane: &str, diff: Value) {
        let serial = {
            let mut watches = self.watches.borrow_mut();
            let Some(watch) = watches.get_mut(pane) else { return };
            watch.updating = false;
            if watch.diff.as_ref() == Some(&diff) {
                drop(watches);
                if let Some(on_diff) = self.on_diff.borrow().as_ref() {
                    on_diff(pane, &json!({ "pane": pane, "updating": false }));
                }
                return;
            }
            watch.serial += 1;
            watch.diff = Some(diff.clone());
            watch.serial
        };
        if let Some(on_diff) = self.on_diff.borrow().as_ref() {
            on_diff(pane, &json!({ "pane": pane, "serial": serial, "diff": diff }));
        }
    }
}

fn work(pane: String, folder: String, args: Vec<String>, inbox: Receiver<Control>, changes: Sender<Control>, events: Sender<Event>) {
    let mut show = HashSet::new();
    let diff = kanna_diff::load(&folder, &args, &show);
    let top = PathBuf::from(&diff.top);
    let dirs: Vec<PathBuf> = [&diff.git_dir, &diff.common_dir].into_iter().filter(|d| !d.is_empty()).map(PathBuf::from).collect();
    let _ = events.send(Event::Diff { pane: pane.clone(), diff: diff.to_json() });
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Ok(event) = event {
            let _ = changes.send(Control::Changed(event.paths));
        }
    })
    .ok();
    if let Some(watcher) = watcher.as_mut() {
        let root = if diff.top.is_empty() { PathBuf::from(&folder) } else { top.clone() };
        let _ = watcher.watch(&root, RecursiveMode::Recursive);
        for dir in &dirs {
            if !dir.starts_with(&root) {
                let _ = watcher.watch(dir, RecursiveMode::Recursive);
            }
        }
    }
    loop {
        let mut changed: Vec<PathBuf> = vec![];
        let mut asked = false;
        match inbox.recv() {
            Ok(Control::Stop) | Err(_) => return,
            Ok(Control::Show(next)) => {
                show = next;
                asked = true;
            }
            Ok(Control::Changed(paths)) => changed.extend(paths),
        }
        loop {
            match inbox.recv_timeout(QUIET) {
                Ok(Control::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                Ok(Control::Show(next)) => {
                    show = next;
                    asked = true;
                }
                Ok(Control::Changed(paths)) => changed.extend(paths),
                Err(RecvTimeoutError::Timeout) => break,
            }
        }
        if !asked && !matters(&changed, &top, &dirs) {
            continue;
        }
        let _ = events.send(Event::DiffUpdating { pane: pane.clone() });
        let diff = kanna_diff::load(&folder, &args, &show);
        let _ = events.send(Event::Diff { pane: pane.clone(), diff: diff.to_json() });
    }
}

fn matters(paths: &[PathBuf], top: &Path, dirs: &[PathBuf]) -> bool {
    let mut files = vec![];
    for path in paths {
        if let Some(dir) = dirs.iter().find(|d| path.starts_with(d)) {
            let inside = path.strip_prefix(dir).unwrap_or(path);
            let first = inside.components().next().map(|c| c.as_os_str().to_string_lossy().into_owned()).unwrap_or_default();
            if !matches!(first.as_str(), "objects" | "logs" | "worktrees" | "hooks" | "lfs") && !inside.to_string_lossy().ends_with(".lock") {
                return true;
            }
            continue;
        }
        if let Ok(inside) = path.strip_prefix(top) {
            if inside.components().next().is_some_and(|c| c.as_os_str() == ".git") {
                continue;
            }
            files.push(inside.to_string_lossy().into_owned());
        }
    }
    if files.is_empty() {
        return false;
    }
    let ignored = kanna_diff::ignored(&files, &top.to_string_lossy());
    files.iter().any(|f| !ignored.contains(f))
}
