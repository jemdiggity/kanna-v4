use crate::error::KmuxError;
use crate::exec::{sleep, Executor, Signal, Turns, Wakeup};
use crate::instance::{Instance, DEFAULT_NAME};
use crate::json::{is_true, json, num, string, Map, Value};
use crate::layout::{place_request, scope_request, Edge, LayoutHost, LayoutLink, LocalLayout, Scope, SplitSide};
use crate::model::{fraction, Model};
use crate::pane::{Pane, PaneState, PaneType, Panes};
use crate::state;
use std::cell::{RefCell, RefMut};
use std::collections::{HashMap, HashSet};
use std::future::{poll_fn, Future};
use std::ops::Deref;
use std::pin::pin;
use std::rc::{Rc, Weak};
use std::sync::{Arc, Mutex};
use std::task::{Poll, Waker};
use std::time::{Duration, Instant};

pub trait PaneHost {
    fn start(&self, pane: &Pane);
    fn stop(&self, pane: &Pane);
    fn send(&self, pane: &Pane, text: &str);
    fn read(&self, pane: &Pane, lines: Option<usize>) -> Result<String, KmuxError> {
        let _ = lines;
        Err(KmuxError::new("unsupported", format!("{} can't be read here", pane.label())))
    }
}

pub type CommandHandler = Rc<dyn Fn(Value, Responder)>;

pub type PrefixHandler = Rc<dyn Fn(&str, Value, Responder)>;

pub const TASKS_PER_PROJECT: usize = 3;

pub fn project(cwd: Option<&str>) -> Option<std::path::PathBuf> {
    let given = std::path::Path::new(cwd?);
    let resolved = std::fs::canonicalize(given).ok();
    let start = resolved.as_deref().unwrap_or(given);
    let Some(root) = start.ancestors().find(|dir| dir.join(".git").exists()) else { return Some(start.to_path_buf()) };
    let marker = root.join(".git");
    let main = std::fs::metadata(&marker)
        .ok()
        .filter(|m| m.is_file() && m.len() <= 4096)
        .and_then(|_| std::fs::read_to_string(&marker).ok())
        .and_then(|text| text.lines().next()?.strip_prefix("gitdir: ").map(|dir| root.join(dir.trim())))
        .and_then(|dir| {
            let text = dir.to_string_lossy().to_string();
            text.find("/.git/worktrees/").map(|at| std::path::PathBuf::from(&text[..at]))
        });
    Some(main.unwrap_or_else(|| root.to_path_buf()))
}

pub const CORE_COMMANDS: [&str; 10] = ["capabilities", "open", "list", "close", "restart", "send", "navigate", "read", "markup", "rename-pane"];

pub const DEBUG_COMMANDS: [&str; 2] = ["debug.saveState", "debug.restoreState"];

const FEATURES: [&str; 7] = ["windows", "tabs", "fractionalSizing", "namedPanes", "zoom", "lifecycle", "dock"];
const CORE_FEATURES: [&str; 2] = ["namedPanes", "lifecycle"];

struct State {
    panes: Panes,
    instance: Instance,
    pane_types: Vec<PaneType>,
    start_timeout: Duration,
    ios_start_timeout: Duration,
    saved_state: Option<Value>,
}

struct Inner {
    state: RefCell<State>,
    layout: Rc<dyn LayoutLink>,
    turns: Rc<Turns>,
    exec: Rc<Executor>,
    host: RefCell<Option<Rc<dyn PaneHost>>>,
    on_change: RefCell<Option<Rc<dyn Fn()>>>,
    extra: RefCell<HashMap<String, CommandHandler>>,
    prefixes: RefCell<Vec<(String, PrefixHandler)>>,
    waiters: RefCell<HashMap<String, Vec<Rc<Signal>>>>,
}

pub struct Core(CoreHandle);

#[derive(Clone)]
pub struct CoreHandle(Rc<Inner>);

#[derive(Clone)]
pub struct WeakCore(Weak<Inner>);

impl WeakCore {
    pub fn upgrade(&self) -> Option<CoreHandle> {
        self.0.upgrade().map(CoreHandle)
    }
}

impl Default for Core {
    fn default() -> Core {
        Core::new(Box::new(Model::new()))
    }
}

impl Core {
    pub fn new(layout: Box<dyn LayoutHost>) -> Core {
        Core::with_link(Rc::new(LocalLayout::new(layout)))
    }

    pub fn with_link(layout: Rc<dyn LayoutLink>) -> Core {
        Core(CoreHandle(Rc::new(Inner {
            layout,
            turns: Rc::default(),
            state: RefCell::new(State {
                panes: Panes::default(),
                instance: Instance::new(DEFAULT_NAME),
                pane_types: PaneType::ALL.to_vec(),
                start_timeout: Duration::from_secs(10),
                ios_start_timeout: Duration::from_secs(180),
                saved_state: None,
            }),
            exec: Rc::default(),
            host: RefCell::new(None),
            on_change: RefCell::new(None),
            extra: RefCell::new(HashMap::new()),
            prefixes: RefCell::new(vec![]),
            waiters: RefCell::new(HashMap::new()),
        })))
    }
}

impl Deref for Core {
    type Target = CoreHandle;
    fn deref(&self) -> &CoreHandle {
        &self.0
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        let inner = &self.0 .0;
        inner.exec.clear();
        inner.waiters.borrow_mut().clear();
        inner.extra.borrow_mut().clear();
        inner.prefixes.borrow_mut().clear();
        inner.host.borrow_mut().take();
        inner.on_change.borrow_mut().take();
    }
}

fn bad_request(message: impl Into<String>) -> KmuxError {
    KmuxError::new("bad_request", message)
}

fn reply<const N: usize>(fields: [(&str, Value); N]) -> Map {
    fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

impl CoreHandle {
    fn state(&self) -> RefMut<'_, State> {
        self.0.state.borrow_mut()
    }

    pub fn weak(&self) -> WeakCore {
        WeakCore(Rc::downgrade(&self.0))
    }

    pub fn set_host(&self, host: Option<Rc<dyn PaneHost>>) {
        *self.0.host.borrow_mut() = host;
    }

    pub fn set_on_change(&self, on_change: Option<Rc<dyn Fn()>>) {
        *self.0.on_change.borrow_mut() = on_change;
    }

    pub fn set_wakeup(&self, wakeup: Option<Wakeup>) {
        self.0.exec.set_wakeup(wakeup);
    }

    pub fn set_instance(&self, instance: Instance) {
        self.state().instance = instance;
    }

    pub fn instance(&self) -> Instance {
        self.state().instance.clone()
    }

    pub fn set_pane_types(&self, types: Vec<PaneType>) {
        self.state().pane_types = types;
    }

    pub fn set_start_timeout(&self, timeout: Duration) {
        self.state().start_timeout = timeout;
    }

    pub fn set_ios_start_timeout(&self, timeout: Duration) {
        self.state().ios_start_timeout = timeout;
    }

    pub fn add_command(&self, name: &str, handler: CommandHandler) {
        self.0.extra.borrow_mut().insert(name.into(), handler);
    }

    pub fn add_prefix(&self, prefix: &str, handler: PrefixHandler) {
        self.0.prefixes.borrow_mut().push((prefix.into(), handler));
    }

    pub fn commands(&self) -> Vec<String> {
        let mut commands: Vec<String> = CORE_COMMANDS.iter().chain(self.0.layout.commands()).map(|c| c.to_string()).collect();
        commands.sort();
        commands
    }

    pub fn pane(&self, reference: &str) -> Option<Pane> {
        self.state().panes.pane(reference).cloned()
    }

    pub fn need_pane(&self, reference: Option<&str>) -> Result<Pane, KmuxError> {
        self.state().panes.need(reference).cloned()
    }

    pub fn with_panes<R>(&self, f: impl FnOnce(&Panes) -> R) -> R {
        f(&self.state().panes)
    }

    pub fn with_pane_mut<R>(&self, id: &str, f: impl FnOnce(&mut Pane) -> R) -> Option<R> {
        self.state().panes.get_mut(id).map(f)
    }

    async fn layout(&self, request: Value) -> Result<Map, KmuxError> {
        let call = {
            let s = self.state();
            self.0.layout.call(request, &s.panes)
        };
        call.await
    }

    fn host(&self) -> Option<Rc<dyn PaneHost>> {
        self.0.host.borrow().clone()
    }

    fn changed(&self) {
        let on_change = self.0.on_change.borrow().clone();
        if let Some(on_change) = on_change {
            on_change();
        }
    }

    pub fn handle(&self, request: Value, reply: impl FnOnce(Value) + 'static) {
        let core = self.clone();
        self.0.exec.spawn(Box::pin(async move { reply(core.respond(request).await) }));
        self.0.exec.run();
    }

    pub fn handle_blocking(&self, request: Value) -> Value {
        let answer = Rc::new(RefCell::new(None));
        let slot = answer.clone();
        self.handle(request, move |reply| *slot.borrow_mut() = Some(reply));
        loop {
            if let Some(reply) = answer.borrow_mut().take() {
                return reply;
            }
            if !self.0.exec.has_ready() {
                self.0.exec.wait(self.0.exec.next_deadline());
            }
            self.0.exec.run();
        }
    }

    pub fn later(&self, f: impl FnOnce() + 'static) {
        self.0.exec.spawn(Box::pin(async move { f() }));
    }

    pub fn poll(&self) -> Option<Instant> {
        self.0.exec.run();
        self.0.exec.next_deadline()
    }

    pub fn update(&self, id: &str, state: PaneState, exit_code: Option<i64>, error: Option<String>) {
        {
            let mut s = self.state();
            let Some(pane) = s.panes.get_mut(id) else { return };
            pane.state = state;
            pane.exit_code = exit_code;
            pane.error = error;
        }
        if state != PaneState::Starting {
            self.resume(id);
        }
        self.changed();
        self.0.exec.run();
    }

    pub fn install_panes(&self, panes: Vec<Pane>, counter: usize) {
        {
            let mut s = self.state();
            let mut all: Vec<Pane> = s.panes.iter().cloned().collect();
            all.extend(panes.into_iter().filter(|p| s.panes.get(&p.id).is_none()));
            s.panes.install(all, counter);
        }
        self.changed();
    }

    pub fn drop_panes(&self, ids: &[String], error: &str) {
        for id in ids {
            let starting = self.state().panes.get(id).is_some_and(|p| p.state == PaneState::Starting);
            if starting {
                self.update(id, PaneState::Failed, None, Some(error.into()));
            }
            self.state().panes.forget(id);
            self.resume(id);
        }
        self.changed();
        self.0.exec.run();
    }

    pub fn set_pid(&self, id: &str, pid: Option<i64>) {
        if let Some(pane) = self.state().panes.get_mut(id) {
            pane.pid = pid;
        }
    }

    pub fn page_moved(&self, id: &str, url: &str) {
        if let Some(pane) = self.state().panes.get_mut(id) {
            moved(pane, url.into());
        }
    }

    fn resume(&self, id: &str) {
        let waiters = self.0.waiters.borrow_mut().remove(id);
        for signal in waiters.into_iter().flatten() {
            signal.set();
        }
    }

    async fn settled(&self, id: &str) {
        let timeout = {
            let state = self.state();
            match state.panes.get(id) {
                Some(pane) if pane.state == PaneState::Starting => {
                    if pane.kind == PaneType::Ios { state.ios_start_timeout } else { state.start_timeout }
                }
                _ => return,
            }
        };
        let signal = Rc::new(Signal::default());
        self.0.waiters.borrow_mut().entry(id.into()).or_default().push(signal.clone());
        let mut wait = pin!(signal.wait());
        let mut timer = pin!(sleep(self.0.exec.clone(), timeout));
        let timed_out = poll_fn(|cx| {
            if wait.as_mut().poll(cx).is_ready() {
                Poll::Ready(false)
            } else if timer.as_mut().poll(cx).is_ready() {
                Poll::Ready(true)
            } else {
                Poll::Pending
            }
        })
        .await;
        if timed_out {
            let starting = self.state().panes.get(id).is_some_and(|p| p.state == PaneState::Starting);
            if starting {
                self.update(id, PaneState::Failed, None, Some(format!("did not start within {:?} seconds", timeout.as_secs_f64())));
            }
            wait.await;
        }
    }

    async fn respond(&self, request: Value) -> Value {
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let result = match string(&request, "cmd") {
            None => Err(bad_request("missing cmd")),
            Some(cmd) => {
                let cmd = cmd.to_string();
                let args = request.get("args").cloned().unwrap_or_else(|| json!({}));
                self.run(&cmd, args).await
            }
        };
        match result {
            Ok(mut reply) => {
                reply.insert("id".into(), id);
                reply.insert("ok".into(), Value::Bool(true));
                Value::Object(reply)
            }
            Err(error) => json!({ "id": id, "ok": false, "error": { "code": error.code, "message": error.message } }),
        }
    }

    async fn run(&self, cmd: &str, args: Value) -> Result<Map, KmuxError> {
        match cmd {
            "capabilities" => Ok(self.capabilities()),
            "open" => self.open(&args).await,
            "list" => self.list().await,
            "close" => self.close(&args).await,
            "restart" => self.restart(&args).await,
            "send" => self.send(&args),
            "read" => self.read(&args),
            "navigate" => self.navigate(&args).await,
            "markup" => self.markup(&args).await,
            "rename-pane" => self.rename_pane(&args).await,
            "debug.saveState" => {
                let state = self.export_state().await?;
                self.state().saved_state = Some(state.clone());
                Ok(reply([("state", state)]))
            }
            "debug.restoreState" => {
                let saved = self.state().saved_state.clone();
                let state = args.get("state").cloned().or(saved).ok_or_else(|| bad_request("no saved state to restore"))?;
                self.restore_state(&state).await?;
                let windows = match self.layout(json!({ "op": "windows" })).await?.remove("windows") {
                    Some(Value::Array(windows)) => windows.iter().filter_map(|w| w.get("id").cloned()).collect(),
                    _ => vec![],
                };
                let panes = self.state().panes.ids().into_iter().map(Value::String).collect();
                Ok(reply([("windows", Value::Array(windows)), ("panes", Value::Array(panes))]))
            }
            _ if self.0.layout.commands().contains(&cmd) => {
                let _turn = self.0.turns.take().await;
                let result = self.layout(json!({ "op": "command", "cmd": cmd, "args": args })).await;
                if result.is_ok() {
                    self.changed();
                }
                result
            }
            _ => {
                let (responder, answer) = responder();
                let handler = self.0.extra.borrow().get(cmd).cloned();
                if let Some(handler) = handler {
                    handler(args, responder);
                    return answer.await;
                }
                let prefixed = self.0.prefixes.borrow().iter().find(|(prefix, _)| cmd.starts_with(prefix.as_str())).map(|(_, h)| h.clone());
                let Some(handler) = prefixed else { return Err(bad_request(format!("unknown command \"{cmd}\""))) };
                handler(cmd, args, responder);
                answer.await
            }
        }
    }

    fn capabilities(&self) -> Map {
        let commands = self.commands();
        let layout = self.0.layout.features();
        let s = self.state();
        let mut features: Vec<&str> = FEATURES.iter().copied().filter(|f| CORE_FEATURES.contains(f) || layout.contains(f)).collect();
        features.extend(layout.iter().filter(|f| !FEATURES.contains(f)));
        reply([
            ("mux", "kmux".into()),
            ("instance", s.instance.name.clone().into()),
            ("paneTypes", s.pane_types.iter().map(|t| t.as_str()).collect::<Vec<_>>().into()),
            ("commands", commands.into()),
            ("features", features.into()),
        ])
    }

    async fn list(&self) -> Result<Map, KmuxError> {
        let windows = self.layout(json!({ "op": "windows" })).await?.remove("windows").unwrap_or(Value::Array(vec![]));
        let s = self.state();
        Ok(reply([("windows", windows), ("panes", Value::Array(s.panes.iter().map(Pane::summary).collect()))]))
    }

    fn start_pane(&self, id: &str) {
        let pane = self.state().panes.get(id).cloned();
        if let (Some(host), Some(pane)) = (self.host(), pane) {
            host.start(&pane);
        }
    }

    fn summary(&self, id: &str, fallback: &Pane) -> Value {
        self.state().panes.get(id).unwrap_or(fallback).summary()
    }

    async fn open(&self, args: &Value) -> Result<Map, KmuxError> {
        let type_name = string(args, "type").unwrap_or("");
        let types = self.state().pane_types.clone();
        let Some(kind) = PaneType::parse(type_name).filter(|t| types.contains(t)) else {
            let names: Vec<&str> = types.iter().map(|t| t.as_str()).collect();
            let listed = match names.split_last() {
                Some((last, [])) => last.to_string(),
                Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
                None => String::new(),
            };
            return Err(bad_request(format!("unknown pane type \"{type_name}\" ({listed})")));
        };
        let empty = |key: &str| string(args, key).map_or(true, str::is_empty);
        if kind == PaneType::Ios && empty("app") {
            return Err(bad_request("ios panes need an app"));
        }
        if (args.get("app").is_some() || args.get("device").is_some()) && kind != PaneType::Ios {
            return Err(bad_request("app and device are only for ios panes"));
        }
        if kind == PaneType::Web && empty("url") {
            return Err(bad_request("web panes need a url"));
        }
        if args.get("history").is_some() && kind != PaneType::Web {
            return Err(bad_request("history is only for web panes"));
        }
        if kind == PaneType::Md && empty("path") {
            return Err(bad_request("markdown panes need a path"));
        }
        if args.get("path").is_some() && kind != PaneType::Md {
            return Err(bad_request("path is only for markdown panes"));
        }
        if args.get("agent").is_some() && !matches!(kind, PaneType::Md | PaneType::Web) {
            return Err(bad_request("agent is only for markdown and web panes"));
        }
        if args.get("markup").is_some() && !matches!(kind, PaneType::Md | PaneType::Web) {
            return Err(bad_request("markup is only for markdown and web panes"));
        }
        let agent = match string(args, "agent") {
            Some(agent) => Some(self.need_pane(Some(agent))?.id),
            None => None,
        };
        let name = string(args, "name").map(String::from);
        let taken = |core: &CoreHandle| match &name {
            Some(name) if core.state().panes.pane(name).is_some() => Err(KmuxError::new("name_taken", format!("a pane named \"{name}\" already exists"))),
            _ => Ok(()),
        };
        taken(self)?;
        let split = match args.get("split").map(|split| split.as_str().unwrap_or("")) {
            None => None,
            Some("right") => Some(SplitSide::Right),
            Some("down") => Some(SplitSide::Down),
            Some("auto") => Some(SplitSide::Auto),
            Some(_) => return Err(bad_request("split must be right, down or auto")),
        };
        let size = match args.get("size") {
            Some(value) => Some(fraction(value)?),
            None => None,
        };
        if !size.is_none_or(|size| size > 0.0 && size < 1.0) {
            return Err(bad_request("size must be a fraction between 0 and 1"));
        }
        let new_tab = args.get("tab").and_then(Value::as_bool).unwrap_or(false);
        let dock = Edge::read(args.get("dock"))?;
        if dock.is_some() && (args.get("split").is_some() || args.get("size").is_some() || new_tab) {
            return Err(bad_request("a docked pane takes no split, size or tab"));
        }
        let shelf = is_true(args, "shelf");
        if shelf && (dock.is_none() || args.get("window").is_some()) {
            return Err(bad_request("shelf is for a docked pane, without a window"));
        }

        let turn = self.0.turns.take().await;
        taken(self)?;
        if dock.is_some() {
            self.room_for_task(string(args, "cwd")).await?;
        }
        let id = self.state().panes.next_id();
        let placeless = !new_tab && dock.is_none() && split.is_none() && size.is_none() && args.get("window").is_none();
        let from = string(args, "from").filter(|_| placeless).and_then(|from| self.state().panes.pane(from).map(|p| p.id.clone()));
        let placed = self.layout(place_request(&Pane::new(&id, name.clone(), kind), string(args, "window"), split, size, new_tab, dock, from.as_deref(), shelf)).await?;
        let placed = Value::Object(placed);
        {
            let mut s = self.state();
            let pane = s.panes.make(kind, name);
            let field = |key: &str| string(args, key).map(String::from);
            pane.command = field("cmd");
            pane.cwd = field("cwd");
            pane.url = string(args, "url").map(normalize_url);
            pane.app = field("app");
            pane.device = field("device");
            pane.path = field("path");
            pane.agent = agent.clone();
            pane.markup = is_true(args, "markup");
            if (kind == PaneType::Web && is_true(args, "history")) || kind == PaneType::Md {
                pane.history = Some(Default::default());
            }
        }
        drop(turn);
        self.changed();
        self.start_pane(&id);

        let snapshot = self.state().panes.get(&id).cloned().unwrap();
        if args.get("wait") == Some(&Value::Bool(false)) {
            return Ok(opened(self.summary(&id, &snapshot), &placed, &placed));
        }
        self.settled(&id).await;
        self.check_started(&id, &snapshot)?;
        let summary = self.summary(&id, &snapshot);
        let location = Value::Object(self.layout(json!({ "op": "location", "pane": id })).await.unwrap_or_default());
        Ok(opened(summary, &location, &placed))
    }

    async fn refuse_docked(&self, id: &str) -> Result<(), KmuxError> {
        let location = self.layout(json!({ "op": "location", "pane": id })).await?;
        if location.get("dock").is_some_and(|dock| !dock.is_null()) {
            return Err(KmuxError::new("docked", "docked: it closes with its window"));
        }
        Ok(())
    }

    fn check_started(&self, id: &str, fallback: &Pane) -> Result<(), KmuxError> {
        let s = self.state();
        let pane = s.panes.get(id).unwrap_or(fallback);
        if pane.state == PaneState::Failed {
            return Err(KmuxError::new("start_failed", pane.error.clone().unwrap_or_else(|| format!("{} failed to start", pane.label()))));
        }
        Ok(())
    }

    async fn close(&self, args: &Value) -> Result<Map, KmuxError> {
        let _turn = self.0.turns.take().await;
        let victims: Vec<String> = if let Some(reference) = string(args, "pane") {
            let id = self.need_pane(Some(reference))?.id;
            self.refuse_docked(&id).await?;
            vec![id]
        } else {
            let scope = if let Some(reference) = string(args, "tab") {
                Scope::Tab(reference.into())
            } else if let Some(task) = string(args, "task") {
                Scope::Task(self.need_pane(Some(task))?.id)
            } else if args.get("window").is_some() {
                Scope::Window(string(args, "window").map(String::from))
            } else {
                return Err(bad_request("close needs a pane, tab, window or task"));
            };
            let found = self.layout(scope_request(&scope)).await?;
            let ids = found.get("panes").and_then(Value::as_array).cloned().unwrap_or_default();
            let s = self.state();
            ids.iter().filter_map(Value::as_str).filter(|id| s.panes.get(id).is_some()).map(String::from).collect()
        };
        for id in &victims {
            self.forget_pane(id);
        }
        if let Err(error) = self.layout(json!({ "op": "remove", "ids": victims, "tidy": true })).await {
            eprintln!("kanna-mux: closed {} but the layout kept them: {error}", victims.join(", "));
        }
        self.changed();
        Ok(reply([("closed", victims.into())]))
    }

    async fn room_for_task(&self, cwd: Option<&str>) -> Result<(), KmuxError> {
        let Ok(listed) = self.layout(json!({ "op": "command", "cmd": "task", "args": { "action": "list" } })).await else { return Ok(()) };
        let here = project(cwd);
        let same: Vec<String> = {
            let s = self.state();
            let tasks = listed.get("tasks").and_then(Value::as_array).cloned().unwrap_or_default();
            tasks.iter().filter_map(|t| s.panes.get(t["id"].as_str()?)).filter(|p| project(p.cwd.as_deref()) == here).map(|p| p.label()).collect()
        };
        if same.len() < TASKS_PER_PROJECT {
            return Ok(());
        }
        let place = here.map_or("this project".to_string(), |p| p.display().to_string());
        Err(KmuxError::new("task_limit", format!("{place} has {TASKS_PER_PROJECT} tasks already ({}): end one with `kanna task done TASK` first", same.join(", "))))
    }

    fn forget_pane(&self, id: &str) {
        let pane = self.state().panes.forget(id);
        if let (Some(host), Some(pane)) = (self.host(), &pane) {
            host.stop(pane);
        }
        self.resume(id);
    }

    async fn reload(&self, id: &str, clear_exit: bool) -> Pane {
        let pane = self.state().panes.get(id).cloned().unwrap();
        if let Some(host) = self.host() {
            host.stop(&pane);
        }
        if let Some(pane) = self.state().panes.get_mut(id) {
            pane.state = PaneState::Starting;
            pane.pid = None;
            if clear_exit {
                pane.exit_code = None;
            }
            pane.error = None;
        }
        self.changed();
        self.start_pane(id);
        let snapshot = self.state().panes.get(id).cloned().unwrap_or(pane);
        self.settled(id).await;
        snapshot
    }

    async fn restart(&self, args: &Value) -> Result<Map, KmuxError> {
        let id = self.need_pane(string(args, "pane"))?.id;
        let snapshot = self.reload(&id, true).await;
        self.check_started(&id, &snapshot)?;
        Ok(reply([("pane", self.summary(&id, &snapshot))]))
    }

    fn send(&self, args: &Value) -> Result<Map, KmuxError> {
        let pane = self.need_pane(string(args, "pane"))?;
        if pane.kind != PaneType::Term {
            return Err(KmuxError::new("wrong_type", format!("{} is a {} pane; send only works on term panes", pane.label(), pane.kind.as_str())));
        }
        if pane.state != PaneState::Running {
            return Err(bad_request(format!("{} is {}", pane.label(), pane.state.as_str())));
        }
        if let Some(host) = self.host() {
            host.send(&pane, string(args, "text").unwrap_or(""));
        }
        Ok(Map::new())
    }

    fn read(&self, args: &Value) -> Result<Map, KmuxError> {
        let pane = self.need_pane(string(args, "pane"))?;
        if pane.kind != PaneType::Term {
            return Err(KmuxError::new("wrong_type", format!("{} is a {} pane; read only works on term panes", pane.label(), pane.kind.as_str())));
        }
        let lines = match args.get("lines") {
            None | Some(Value::Null) => None,
            Some(value) => match value.as_f64() {
                Some(n) if n >= 1.0 && n.fract() == 0.0 => Some(n as usize),
                _ => return Err(bad_request("lines must be a whole number above 0")),
            },
        };
        let host = self.host().ok_or_else(|| KmuxError::new("unsupported", "nothing runs terminals here"))?;
        let text = host.read(&pane, lines)?;
        Ok(reply([("text", text.into())]))
    }

    async fn navigate(&self, args: &Value) -> Result<Map, KmuxError> {
        let pane = self.need_pane(string(args, "pane"))?;
        let step = if is_true(args, "back") {
            Some("back")
        } else if is_true(args, "forward") {
            Some("forward")
        } else {
            None
        };
        let mut changed = pane.clone();
        if pane.kind == PaneType::Md {
            let mut history = pane.history.clone().unwrap_or_default();
            let current = pane.path.clone().unwrap_or_default();
            let path = if let Some(step) = step {
                let path = if step == "back" { history.back.pop() } else { history.forward.pop() };
                let path = path.ok_or_else(|| bad_request(format!("nothing to go {step} to")))?;
                if step == "back" { history.forward.push(current) } else { history.back.push(current) }
                path
            } else {
                let path = string(args, "path").filter(|p| !p.is_empty()).ok_or_else(|| bad_request("missing path"))?;
                let target = resolve(path, &current);
                if target != current {
                    history.back.push(current);
                    history.forward.clear();
                }
                target
            };
            changed.history = Some(history);
            changed.path = Some(path);
        } else {
            if pane.kind != PaneType::Web {
                return Err(KmuxError::new(
                    "wrong_type",
                    format!("{} is a {} pane; navigate only works on web and markdown panes", pane.label(), pane.kind.as_str()),
                ));
            }
            if let Some(step) = step {
                let mut history = pane.history.clone().ok_or_else(|| bad_request(format!("{} keeps no history; open it with history: true", pane.label())))?;
                let url = if step == "back" { history.back.pop() } else { history.forward.pop() };
                let url = url.ok_or_else(|| bad_request(format!("nothing to go {step} to")))?;
                if let Some(current) = &pane.url {
                    if step == "back" { history.forward.push(current.clone()) } else { history.back.push(current.clone()) }
                }
                changed.history = Some(history);
                changed.url = Some(url);
            } else {
                let url = string(args, "url").filter(|u| !u.is_empty()).ok_or_else(|| bad_request("missing url"))?;
                moved(&mut changed, normalize_url(url));
            }
        }
        if let Some(pane) = self.state().panes.get_mut(&pane.id) {
            pane.history = changed.history;
            pane.path = changed.path;
            pane.url = changed.url;
        }
        let snapshot = self.reload(&pane.id, false).await;
        Ok(reply([("pane", self.summary(&pane.id, &snapshot))]))
    }

    async fn markup(&self, args: &Value) -> Result<Map, KmuxError> {
        let pane = self.need_pane(string(args, "pane"))?;
        if !matches!(pane.kind, PaneType::Md | PaneType::Web) {
            return Err(KmuxError::new(
                "wrong_type",
                format!("{} is a {} pane; comments only work on web and markdown panes", pane.label(), pane.kind.as_str()),
            ));
        }
        let on = !is_true(args, "off");
        let agent = match string(args, "agent").filter(|_| on) {
            Some(agent) => Some(self.need_pane(Some(agent))?.id),
            None => None,
        };
        if let Some(pane) = self.state().panes.get_mut(&pane.id) {
            pane.markup = on;
            if agent.is_some() {
                pane.agent = agent;
            }
        }
        let snapshot = self.reload(&pane.id, false).await;
        Ok(reply([("pane", self.summary(&pane.id, &snapshot))]))
    }

    async fn rename_pane(&self, args: &Value) -> Result<Map, KmuxError> {
        let pane = self.need_pane(string(args, "pane"))?;
        let name = string(args, "name").map(str::trim).filter(|n| !n.is_empty()).map(String::from);
        if let Some(name) = &name {
            if self.state().panes.pane(name).is_some_and(|other| other.id != pane.id) {
                return Err(KmuxError::new("name_taken", format!("a pane named \"{name}\" already exists")));
            }
        }
        if let Some(found) = self.state().panes.get_mut(&pane.id) {
            found.name = name;
        }
        self.changed();
        let _ = self.layout(json!({ "op": "windows" })).await;
        Ok(reply([("pane", self.summary(&pane.id, &pane))]))
    }

    async fn export_state(&self) -> Result<Value, KmuxError> {
        let mut out = self.layout(json!({ "op": "export" })).await?;
        let s = self.state();
        let mut counters = match out.remove("counters") {
            Some(Value::Object(counters)) => counters,
            _ => Map::new(),
        };
        counters.insert("pane".into(), num(s.panes.counter() as f64));
        out.insert("kmux".into(), "state".into());
        out.insert("version".into(), num(state::STATE_VERSION as f64));
        out.insert("counters".into(), Value::Object(counters));
        out.insert("panes".into(), state::export_panes(&s.panes));
        Ok(Value::Object(out))
    }

    async fn restore_state(&self, saved: &Value) -> Result<(), KmuxError> {
        let (panes, counter) = state::import_panes(saved)?;
        let ids: Vec<String> = panes.iter().map(|p| p.id.clone()).collect();
        let _turn = self.0.turns.take().await;
        self.layout(json!({ "op": "check", "state": saved, "ids": ids })).await?;
        let old = self.state().panes.ids();
        for id in &old {
            self.forget_pane(id);
        }
        self.layout(json!({ "op": "remove", "ids": old })).await?;
        let installed = self.layout(json!({ "op": "install", "state": saved, "ids": ids })).await?;
        let placed: HashSet<&str> = installed.get("placed").and_then(Value::as_array).map(|ids| ids.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
        let restored = {
            let mut s = self.state();
            let restored: Vec<Pane> = panes.into_iter().filter(|p| placed.contains(p.id.as_str())).collect();
            let ids = restored.iter().map(|p| p.id.clone()).collect::<Vec<_>>();
            s.panes.install(restored, counter);
            ids
        };
        self.changed();
        for id in &restored {
            self.start_pane(id);
        }
        Ok(())
    }
}

fn opened(summary: Value, location: &Value, placed: &Value) -> Map {
    let window = if location["window"].is_null() { placed["window"].clone() } else { location["window"].clone() };
    let mut out = reply([("pane", summary), ("window", window), ("tab", location["tab"].clone())]);
    if let Some(edge) = location.get("dock").filter(|d| !d.is_null()) {
        out.insert("dock".into(), edge.clone());
    }
    out
}

fn moved(pane: &mut Pane, url: String) {
    let same = |a: &str, b: &str| a == b || format!("{a}/") == b || a == format!("{b}/");
    if let (Some(history), Some(current)) = (pane.history.as_mut(), pane.url.as_ref()) {
        if !same(current, &url) {
            history.back.push(current.clone());
            history.forward.clear();
        }
    }
    pane.url = Some(url);
}

pub fn normalize_url(url: &str) -> String {
    let mut chars = url.chars();
    let scheme = chars.next().is_some_and(|c| c.is_ascii_alphabetic()) && {
        let rest = &url[1..];
        let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '+' || c == '.' || c == '-')).unwrap_or(rest.len());
        rest[end..].starts_with("://")
    };
    if scheme { url.into() } else { format!("http://{url}") }
}

pub fn resolve(link: &str, file: &str) -> String {
    if link.starts_with('/') {
        return link.into();
    }
    let mut parts: Vec<&str> = file.split('/').collect();
    parts.pop();
    for part in link.split('/').filter(|p| !p.is_empty()) {
        if part == ".." {
            parts.pop();
        } else if part != "." {
            parts.push(part);
        }
    }
    parts.join("/")
}

pub struct Responder(Arc<Mutex<Answer>>);

#[derive(Default)]
struct Answer {
    result: Option<Result<Map, KmuxError>>,
    waker: Option<Waker>,
    answered: bool,
}

impl Responder {
    pub fn reply(mut self, result: Result<Map, KmuxError>) {
        self.answer(result);
    }

    fn answer(&mut self, result: Result<Map, KmuxError>) {
        let waker = {
            let mut answer = self.0.lock().unwrap();
            if answer.answered {
                return;
            }
            answer.answered = true;
            answer.result = Some(result);
            answer.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

impl Drop for Responder {
    fn drop(&mut self) {
        self.answer(Err(bad_request("the command ended without answering")));
    }
}

pub fn responder() -> (Responder, impl Future<Output = Result<Map, KmuxError>>) {
    let answer = Arc::new(Mutex::new(Answer::default()));
    let waiting = answer.clone();
    let future = poll_fn(move |cx| {
        let mut answer = waiting.lock().unwrap();
        match answer.result.take() {
            Some(result) => Poll::Ready(result),
            None => {
                answer.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    });
    (Responder(answer), future)
}
