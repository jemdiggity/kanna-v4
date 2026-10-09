//! The app link: Kanna.app's long-lived connection to the daemon
//! (docs/kanna-spec.md §4.5). The app says `app.hello` on the control
//! socket, and from then on the connection carries JSON lines both ways:
//! the daemon sends the layout to mirror, terminal output and sizes, and
//! requests for the app (`debug.*`); the app sends keystrokes, sizes, which
//! terminals it shows, and its answers. One app at a time.

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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;

/// The first request of an app's connection.
pub const HELLO: &str = "app.hello";

const MAX_LINE: u64 = 16 << 20;

const RESET: &[u8] = b"\x1bc";

/// The requests any client may make that the daemon passes to the app.
pub const APP_COMMANDS: [&str; 5] = ["debug.app", "debug.type", "debug.key", "debug.text", "debug.snapshot"];

/// Serves an app's connection on the socket server's thread: posts what it
/// reads to the main thread, and writes what the main thread sends.
pub fn takeover(events: Sender<Event>) -> Takeover {
    let counter = Arc::new(AtomicU64::new(0));
    Arc::new(move |hello: Value, mut reader: BufReader<UnixStream>, stream: UnixStream| {
        let conn = counter.fetch_add(1, Ordering::SeqCst) + 1;
        let (out, lines) = mpsc::channel::<Vec<u8>>();
        let (Ok(writer), Ok(closer)) = (stream.try_clone(), stream.try_clone()) else { return };
        let spawned = std::thread::Builder::new().name("kanna.app.write".into()).spawn(move || {
            for line in lines {
                if !write_all(&writer, &line) {
                    break;
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
    out: Sender<Vec<u8>>,
    stream: UnixStream,
}

/// The app's side of the daemon, on the main thread.
pub struct AppLink {
    core: WeakCore,
    terminals: Rc<Terminals>,
    app: RefCell<Option<Connection>>,
    /// Requests passed to the app, by the ID the daemon gave them.
    pending: RefCell<HashMap<u64, Responder>>,
    next_id: Cell<u64>,
    /// The terminals the app shows: it gets their output.
    attached: RefCell<HashSet<String>>,
    /// The layout last sent.
    shown: RefCell<Option<Value>>,
}

impl AppLink {
    pub fn new(core: WeakCore, terminals: Rc<Terminals>) -> AppLink {
        AppLink {
            core,
            terminals,
            app: RefCell::new(None),
            pending: RefCell::default(),
            next_id: Cell::new(0),
            attached: RefCell::default(),
            shown: RefCell::new(None),
        }
    }

    fn send(&self, message: Value) {
        if let Some(app) = self.app.borrow().as_ref() {
            let mut line = encode(&message).into_bytes();
            line.push(b'\n');
            let _ = app.out.send(line);
        }
    }

    fn notify(&self, cmd: &str, args: Value) {
        self.send(json!({ "cmd": cmd, "args": args }));
    }

    // MARK: The connection

    pub fn hello(&self, conn: u64, hello: Value, out: Sender<Vec<u8>>, stream: UnixStream) {
        let id = hello.get("id").cloned().unwrap_or(Value::Null);
        let answer = |reply: Value| {
            let mut line = encode(&reply).into_bytes();
            line.push(b'\n');
            let _ = out.send(line);
        };
        if self.app.borrow().is_some() {
            answer(json!({ "id": id, "ok": false, "error": { "code": "app_attached", "message": "another Kanna.app is attached to this daemon" } }));
            let _ = stream.shutdown(Shutdown::Read);
            return;
        }
        let pid = hello.get("args").and_then(|a| a.get("pid")).cloned().unwrap_or(Value::Null);
        eprintln!("kanna-daemon: the app attached (pid {pid})");
        answer(json!({ "id": id, "ok": true, "daemon": { "pid": std::process::id() } }));
        *self.app.borrow_mut() = Some(Connection { id: conn, out, stream });
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
        self.shown.borrow_mut().take();
        let pending: Vec<Responder> = self.pending.borrow_mut().drain().map(|(_, r)| r).collect();
        for responder in pending {
            responder.reply(Err(KmuxError::new("app_gone", "Kanna.app went away before answering")));
        }
    }

    /// Closes the app's connection (the daemon is stopping).
    pub fn close(&self) {
        if let Some(app) = self.app.borrow_mut().take() {
            let _ = app.stream.shutdown(Shutdown::Both);
        }
    }

    pub fn message(&self, conn: u64, message: Value) {
        if self.app.borrow().as_ref().map(|a| a.id) != Some(conn) {
            return;
        }
        // An answer to a request the daemon passed on.
        if message.get("ok").is_some() {
            let responder = message.get("id").and_then(Value::as_u64).and_then(|id| self.pending.borrow_mut().remove(&id));
            if let Some(responder) = responder {
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

    /// The app shows a terminal: its size, what it has written lately, and
    /// its exit if it has ended; then its output as it comes.
    fn attach(&self, pane: &str) {
        let Some((cols, rows, replay, exit_code)) = self.terminals.attach(pane) else {
            return self.notify("exited", json!({ "pane": pane, "code": Value::Null }));
        };
        self.attached.borrow_mut().insert(pane.into());
        self.output(pane, RESET);
        self.notify("grid", json!({ "pane": pane, "cols": cols, "rows": rows }));
        for chunk in replay.chunks(65536) {
            self.output(pane, chunk);
        }
        if let Some(code) = exit_code {
            self.exited(pane, code);
        }
    }

    // MARK: From the terminals

    pub fn output(&self, pane: &str, bytes: &[u8]) {
        if self.attached.borrow().contains(pane) {
            self.notify("output", json!({ "pane": pane, "data": BASE64.encode(bytes) }));
        }
    }

    pub fn exited(&self, pane: &str, code: i32) {
        if self.attached.borrow().contains(pane) {
            self.notify("exited", json!({ "pane": pane, "code": code }));
        }
    }

    /// A terminal started. If the app already shows the pane, it's a
    /// `restart`: the app's view starts over, at the new terminal's size.
    pub fn started(&self, pane: &str) {
        if self.attached.borrow().contains(pane) {
            self.output(pane, RESET);
            if let Some((cols, rows, _, _)) = self.terminals.attach(pane) {
                self.notify("grid", json!({ "pane": pane, "cols": cols, "rows": rows }));
            }
        }
    }

    // MARK: The layout

    /// Sends the layout to mirror, if it changed.
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

    // MARK: Requests for the app

    /// Passes a request to the app, with its pane (a name or an ID) as the
    /// pane's ID; the app's answer is the reply.
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
        self.pending.borrow_mut().insert(id, responder);
        self.send(json!({ "id": id, "cmd": cmd, "args": args }));
    }
}

/// An answer from the app as the reply's fields, or its error.
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

/// What the app mirrors for M0: the first window's active tab, with panes
/// by ID. (The app owns the layout from M1.)
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

/// `{pane, type}` for a pane; `{split: row|column, children}` for a split,
/// each child with its `size`, a fraction of the split.
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
