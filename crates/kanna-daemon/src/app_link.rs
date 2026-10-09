use crate::terminals::Terminals;
use crate::Event;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use kanna_mux::json::{encode, json, string, Map, Value};
use kanna_mux::model::{Model, Node};
use kanna_mux::{write_all, CoreHandle, KmuxError, Panes, Responder, Takeover, WeakCore};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const HELLO: &str = "app.hello";

const MAX_LINE: u64 = 16 << 20;

const QUEUE_BEHIND: usize = 16 << 20;

const QUEUE_CAUGHT_UP: usize = 1 << 20;

const QUEUE_LIMIT: usize = 64 << 20;

pub struct Outbox {
    lines: Sender<Vec<u8>>,
    queue: Arc<Queue>,
}

#[derive(Default)]
struct Queue {
    bytes: AtomicUsize,
    behind: AtomicBool,
}

impl Outbox {
    fn push(&self, message: &Value) -> usize {
        let mut line = encode(message).into_bytes();
        line.push(b'\n');
        let queued = self.queue.bytes.fetch_add(line.len(), Ordering::SeqCst) + line.len();
        let _ = self.lines.send(line);
        queued
    }
}

pub const APP_COMMANDS: [&str; 5] = ["debug.app", "debug.type", "debug.key", "debug.text", "debug.snapshot"];

pub fn takeover(events: Sender<Event>) -> Takeover {
    let counter = Arc::new(AtomicU64::new(0));
    Arc::new(move |hello: Value, mut reader: BufReader<UnixStream>, stream: UnixStream| {
        let conn = counter.fetch_add(1, Ordering::SeqCst) + 1;
        let (sender, lines) = mpsc::channel::<Vec<u8>>();
        let out = Outbox { lines: sender, queue: Arc::default() };
        let (queue, caught_up) = (out.queue.clone(), events.clone());
        let (Ok(writer), Ok(closer)) = (stream.try_clone(), stream.try_clone()) else { return };
        let spawned = std::thread::Builder::new().name("kanna.app.write".into()).spawn(move || {
            for line in lines {
                if !write_all(&writer, &line) {
                    break;
                }
                let left = queue.bytes.fetch_sub(line.len(), Ordering::SeqCst) - line.len();
                if left <= QUEUE_CAUGHT_UP && queue.behind.swap(false, Ordering::SeqCst) {
                    let _ = caught_up.send(Event::AppCaughtUp { conn });
                }
            }
            let _ = writer.shutdown(Shutdown::Both);
        });
        if spawned.is_err() || events.send(Event::AppHello { conn, hello, out, stream: closer }).is_err() {
            return;
        }
        let mut line = Vec::new();
        loop {
            line.clear();
            match reader.by_ref().take(MAX_LINE + 1).read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            if line.len() as u64 > MAX_LINE {
                eprintln!("kanna-daemon: the app sent a line over {} MiB; dropping it", MAX_LINE >> 20);
                break;
            }
            match serde_json::from_slice::<Value>(&line) {
                Ok(message) => {
                    if events.send(Event::AppMessage { conn, message }).is_err() {
                        break;
                    }
                }
                Err(_) => eprintln!("kanna-daemon: the app sent a line that isn't JSON"),
            }
        }
        let _ = events.send(Event::AppGone { conn });
    })
}

struct Connection {
    id: u64,
    out: Outbox,
    stream: UnixStream,
    behind: Cell<bool>,
    cut: Cell<bool>,
}

pub struct AppLink {
    core: WeakCore,
    terminals: Rc<Terminals>,
    app: RefCell<Option<Connection>>,
    pending: RefCell<HashMap<u64, (Instant, Responder)>>,
    timeout: Duration,
    next_id: Cell<u64>,
    attached: RefCell<HashSet<String>>,
    unsent: RefCell<HashSet<String>>,
    shown: RefCell<Option<Value>>,
}

impl AppLink {
    pub fn new(core: WeakCore, terminals: Rc<Terminals>, timeout: Duration) -> AppLink {
        AppLink {
            core,
            terminals,
            app: RefCell::new(None),
            pending: RefCell::default(),
            timeout,
            next_id: Cell::new(0),
            attached: RefCell::default(),
            unsent: RefCell::default(),
            shown: RefCell::new(None),
        }
    }

    fn send(&self, message: Value) {
        let app = self.app.borrow();
        let Some(app) = app.as_ref().filter(|app| !app.cut.get()) else { return };
        let queued = app.out.push(&message);
        if queued > QUEUE_LIMIT {
            eprintln!("kanna-daemon: the app is {} MiB behind; dropping it", queued >> 20);
            app.cut.set(true);
            let _ = app.stream.shutdown(Shutdown::Both);
        } else if queued > QUEUE_BEHIND && !app.behind.replace(true) {
            eprintln!("kanna-daemon: the app is {} MiB behind; its terminals start over when it catches up", queued >> 20);
            app.out.queue.behind.store(true, Ordering::SeqCst);
        }
    }

    fn behind(&self) -> bool {
        self.app.borrow().as_ref().is_some_and(|app| app.behind.get())
    }

    fn shows(&self, pane: &str) -> bool {
        !self.behind() && self.attached.borrow().contains(pane)
    }

    pub fn caught_up(&self, conn: u64) {
        match self.app.borrow().as_ref() {
            Some(app) if app.id == conn => app.behind.set(false),
            _ => return,
        }
        let mut panes: Vec<String> = self.attached.borrow().iter().cloned().collect();
        panes.sort();
        for pane in panes {
            self.attach(&pane);
        }
    }

    pub fn resync(&self, pane: &str) {
        if self.shows(pane) {
            self.attach(pane);
        }
    }

    fn notify(&self, cmd: &str, args: Value) {
        self.send(json!({ "cmd": cmd, "args": args }));
    }

    pub fn hello(&self, conn: u64, hello: Value, out: Outbox, stream: UnixStream) {
        let id = hello.get("id").cloned().unwrap_or(Value::Null);
        let answer = |reply: Value| {
            out.push(&reply);
        };
        if self.app.borrow().is_some() {
            answer(json!({ "id": id, "ok": false, "error": { "code": "app_attached", "message": "another Kanna.app is attached to this daemon" } }));
            let _ = stream.shutdown(Shutdown::Read);
            return;
        }
        let pid = hello.get("args").and_then(|a| a.get("pid")).cloned().unwrap_or(Value::Null);
        eprintln!("kanna-daemon: the app attached (pid {pid})");
        answer(json!({ "id": id, "ok": true, "daemon": { "pid": std::process::id() } }));
        *self.app.borrow_mut() = Some(Connection { id: conn, out, stream, behind: Cell::new(false), cut: Cell::new(false) });
        self.shown.borrow_mut().take();
        self.layout_changed();
    }

    pub fn gone(&self, conn: u64) {
        if self.app.borrow().as_ref().map(|a| a.id) != Some(conn) {
            return;
        }
        eprintln!("kanna-daemon: the app went away");
        self.app.borrow_mut().take();
        self.attached.borrow_mut().clear();
        self.unsent.borrow_mut().clear();
        self.shown.borrow_mut().take();
        let pending: Vec<Responder> = self.pending.borrow_mut().drain().map(|(_, (_, r))| r).collect();
        for responder in pending {
            responder.reply(Err(KmuxError::new("app_gone", "Kanna.app went away before answering")));
        }
    }

    pub fn close(&self) {
        if let Some(app) = self.app.borrow_mut().take() {
            let _ = app.stream.shutdown(Shutdown::Both);
        }
    }

    pub fn message(&self, conn: u64, message: Value) {
        if self.app.borrow().as_ref().map(|a| a.id) != Some(conn) {
            return;
        }
        if message.get("ok").is_some() {
            let responder = message.get("id").and_then(Value::as_u64).and_then(|id| self.pending.borrow_mut().remove(&id));
            if let Some((_, responder)) = responder {
                responder.reply(answer(message));
            }
            return;
        }
        let args = message.get("args").cloned().unwrap_or_else(|| json!({}));
        let pane = string(&args, "pane").unwrap_or_default().to_string();
        match string(&message, "cmd").unwrap_or_default() {
            "attach" => self.attach(&pane),
            "detach" => {
                self.attached.borrow_mut().remove(&pane);
                self.unsent.borrow_mut().remove(&pane);
            }
            "input" => {
                if let Some(bytes) = string(&args, "data").and_then(|d| BASE64.decode(d).ok()) {
                    self.terminals.write(&pane, bytes);
                }
            }
            "resize" => {
                let field = |key: &str| args.get(key).and_then(Value::as_u64).unwrap_or(0).min(u16::MAX as u64) as u16;
                if let Some((cols, rows)) = self.terminals.resize(&pane, field("cols"), field("rows"), field("width"), field("height")) {
                    self.notify("grid", json!({ "pane": pane, "cols": cols, "rows": rows }));
                }
            }
            other => eprintln!("kanna-daemon: the app sent an unknown message \"{other}\""),
        }
    }

    fn attach(&self, pane: &str) {
        let Some(attach) = self.terminals.attach(pane) else {
            return self.notify("exited", json!({ "pane": pane, "code": Value::Null }));
        };
        self.attached.borrow_mut().insert(pane.into());
        let snapshot = match attach.snapshot {
            Ok(snapshot) => snapshot,
            Err(error) => {
                if self.unsent.borrow_mut().insert(pane.into()) {
                    eprintln!("kanna-daemon: can't snapshot {pane} yet ({error}); trying again after its next output");
                }
                return;
            }
        };
        self.unsent.borrow_mut().remove(pane);
        if self.shows(pane) {
            self.notify("snapshot", json!({ "pane": pane, "data": BASE64.encode(&snapshot) }));
            self.notify("grid", json!({ "pane": pane, "cols": attach.cols, "rows": attach.rows }));
        }
        if let Some(code) = attach.exit_code {
            self.exited(pane, code);
        }
    }

    pub fn output(&self, pane: &str, bytes: &[u8]) {
        if !self.shows(pane) {
            return;
        }
        if self.unsent.borrow().contains(pane) {
            return self.attach(pane);
        }
        self.notify("output", json!({ "pane": pane, "data": BASE64.encode(bytes) }));
    }

    pub fn exited(&self, pane: &str, code: i32) {
        if !self.shows(pane) {
            return;
        }
        if self.unsent.borrow().contains(pane) {
            self.attach(pane);
            if !self.unsent.borrow().contains(pane) {
                return;
            }
        }
        self.notify("exited", json!({ "pane": pane, "code": code }));
    }

    pub fn started(&self, pane: &str) {
        if self.shows(pane) {
            self.attach(pane);
        }
    }

    pub fn layout_changed(&self) {
        if self.app.borrow().is_none() {
            return;
        }
        let Some(core) = self.core.upgrade() else { return };
        let layout = mirror(&core);
        if self.shown.borrow().as_ref() == Some(&layout) {
            return;
        }
        *self.shown.borrow_mut() = Some(layout.clone());
        self.notify("layout", layout);
    }

    pub fn forward(&self, cmd: &str, mut args: Value, responder: Responder) {
        if self.app.borrow().is_none() {
            return responder.reply(Err(KmuxError::new("no_app", format!("{cmd} needs Kanna.app, and none is attached to this daemon"))));
        }
        if let (Some(reference), Some(core)) = (string(&args, "pane").map(String::from), self.core.upgrade()) {
            match core.pane(&reference) {
                Some(pane) => args["pane"] = Value::String(pane.id),
                None => return responder.reply(Err(KmuxError::new("not_found", format!("no pane \"{reference}\"")))),
            }
        }
        let id = self.next_id.get() + 1;
        self.next_id.set(id);
        self.pending.borrow_mut().insert(id, (Instant::now() + self.timeout, responder));
        self.send(json!({ "id": id, "cmd": cmd, "args": args }));
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        self.pending.borrow().values().map(|(deadline, _)| *deadline).min()
    }

    pub fn expire(&self, now: Instant) {
        let late: Vec<u64> = self.pending.borrow().iter().filter(|(_, (deadline, _))| *deadline <= now).map(|(id, _)| *id).collect();
        for id in late {
            let Some((_, responder)) = self.pending.borrow_mut().remove(&id) else { continue };
            let seconds = self.timeout.as_secs_f64();
            responder.reply(Err(KmuxError::new("timeout", format!("Kanna.app didn't answer within {seconds}s"))));
        }
    }
}

fn answer(message: Value) -> Result<Map, KmuxError> {
    let Value::Object(mut fields) = message else { return Err(KmuxError::new("bad_request", "the app's answer isn't an object")) };
    if fields.get("ok") != Some(&Value::Bool(true)) {
        let error = fields.get("error").cloned().unwrap_or(Value::Null);
        let text = |key: &str, default: &str| string(&error, key).unwrap_or(default).to_string();
        return Err(KmuxError::new(text("code", "bad_request"), text("message", "the app failed")));
    }
    fields.remove("id");
    fields.remove("ok");
    Ok(fields)
}

fn mirror(core: &CoreHandle) -> Value {
    core.with_layout::<Model, _>(|model, panes| {
        let Some(window) = model.windows().first() else { return json!({ "window": null }) };
        let Some(tab) = window.active_tab() else { return json!({ "window": window.id, "tab": null }) };
        let root = match &window.zoomed {
            Some(pane) if tab.root.as_ref().is_some_and(|r| contains(r, pane)) => tree(&Node::Pane(pane.clone()), panes),
            _ => tab.root.as_ref().map_or(Value::Null, |root| tree(root, panes)),
        };
        json!({ "window": window.id, "tab": tab.id, "focused": window.focused, "root": root })
    })
    .unwrap_or(Value::Null)
}

fn contains(node: &Node, pane: &str) -> bool {
    match node {
        Node::Pane(id) => id == pane,
        Node::Split(split) => split.kids.iter().any(|kid| contains(&kid.node, pane)),
    }
}

fn tree(node: &Node, panes: &Panes) -> Value {
    match node {
        Node::Pane(id) => json!({ "pane": id, "type": panes.get(id).map_or("term", |p| p.kind.as_str()) }),
        Node::Split(split) => {
            let children = split
                .kids
                .iter()
                .map(|kid| {
                    let mut child = tree(&kid.node, panes);
                    child["size"] = json!(kid.size);
                    child
                })
                .collect::<Vec<_>>();
            json!({ "split": split.axis.as_str(), "children": children })
        }
    }
}
