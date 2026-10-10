use crate::error::KmuxError;
use crate::json::{json, num, opt, string, Map, Value};
use crate::pane::{Pane, PaneState, PaneType, Panes};
use crate::state;
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashSet;
use std::future::{ready, Future};
use std::pin::Pin;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitSide {
    Right,
    Down,
    Auto,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
}

impl Edge {
    pub fn as_str(self) -> &'static str {
        match self {
            Edge::Left => "left",
            Edge::Right => "right",
        }
    }

    pub fn parse(name: &str) -> Option<Edge> {
        match name {
            "left" => Some(Edge::Left),
            "right" => Some(Edge::Right),
            _ => None,
        }
    }

    pub fn read(value: Option<&Value>) -> Result<Option<Edge>, KmuxError> {
        match value {
            None | Some(Value::Null) => Ok(None),
            Some(value) => value.as_str().and_then(Edge::parse).map(Some).ok_or_else(|| KmuxError::new("bad_request", "dock must be left or right")),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Placement {
    pub window: Option<String>,
    pub split: SplitSide,
    pub size: f64,
    pub new_tab: bool,
    pub dock: Option<Edge>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Spot {
    pub window: String,
    pub tab: Option<String>,
    pub dock: Option<Edge>,
}

#[derive(Clone, Debug)]
pub enum Scope {
    Tab(String),
    Window(Option<String>),
}

pub struct SavedLayout {
    pub placed: HashSet<String>,
    pub layout: Box<dyn Any>,
}

pub trait LayoutHost {
    fn commands(&self) -> &'static [&'static str];

    fn features(&self) -> &'static [&'static str];

    fn place(&mut self, pane: &str, at: &Placement) -> Result<Option<String>, KmuxError>;

    fn location(&self, pane: &str) -> Option<Spot>;

    fn panes_in(&mut self, scope: &Scope) -> Result<Vec<String>, KmuxError>;

    fn remove(&mut self, pane: &str);

    fn tidy(&mut self);

    fn command(&mut self, cmd: &str, args: &Value, panes: &Panes) -> Result<Map, KmuxError>;

    fn windows(&self, panes: &Panes) -> Value;

    fn export_state(&self) -> Map;

    fn read_state(&self, state: &Value, panes: &HashSet<String>) -> Result<SavedLayout, KmuxError>;

    fn install_state(&mut self, saved: SavedLayout);
}

pub type LayoutCall = Pin<Box<dyn Future<Output = Result<Map, KmuxError>>>>;

pub trait LayoutLink {
    fn commands(&self) -> &'static [&'static str];

    fn features(&self) -> &'static [&'static str];

    fn call(&self, request: Value, panes: &Panes) -> LayoutCall;
}

pub struct LocalLayout(RefCell<Box<dyn LayoutHost>>);

impl LocalLayout {
    pub fn new(host: Box<dyn LayoutHost>) -> LocalLayout {
        LocalLayout(RefCell::new(host))
    }
}

impl LayoutLink for LocalLayout {
    fn commands(&self) -> &'static [&'static str] {
        self.0.borrow().commands()
    }

    fn features(&self) -> &'static [&'static str] {
        self.0.borrow().features()
    }

    fn call(&self, request: Value, panes: &Panes) -> LayoutCall {
        let result = serve(&mut **self.0.borrow_mut(), &request, panes);
        Box::pin(ready(result))
    }
}

pub fn place_request(pane: &Pane, window: Option<&str>, split: Option<SplitSide>, size: Option<f64>, new_tab: bool, dock: Option<Edge>) -> Value {
    let mut at = Map::new();
    if let Some(dock) = dock {
        at.insert("dock".into(), dock.as_str().into());
    }
    if let Some(window) = window {
        at.insert("window".into(), window.into());
    }
    if let Some(split) = split {
        at.insert("split".into(), split_name(split).into());
    }
    if let Some(size) = size {
        at.insert("size".into(), num(size));
    }
    if new_tab {
        at.insert("tab".into(), Value::Bool(true));
    }
    json!({ "op": "place", "pane": pane_entry(pane), "at": at })
}

pub fn scope_request(scope: &Scope) -> Value {
    match scope {
        Scope::Tab(tab) => json!({ "op": "panes-in", "tab": tab }),
        Scope::Window(window) => json!({ "op": "panes-in", "window": opt(window.as_deref()) }),
    }
}

fn split_name(split: SplitSide) -> &'static str {
    match split {
        SplitSide::Right => "right",
        SplitSide::Down => "down",
        SplitSide::Auto => "auto",
    }
}

pub fn pane_entry(pane: &Pane) -> Value {
    Value::Object(state::pane_record(pane))
}

pub fn pane_table(panes: &Panes) -> Value {
    Value::Array(panes.iter().map(pane_entry).collect())
}

pub fn read_pane_table(table: &Value) -> Result<Vec<Pane>, KmuxError> {
    let Some(entries) = table.as_array() else { return Err(bad("panes must be a list")) };
    entries
        .iter()
        .map(|entry| {
            let id = string(entry, "id").filter(|id| !id.is_empty()).ok_or_else(|| bad("a pane has no id"))?;
            let kind = string(entry, "type").and_then(PaneType::parse).ok_or_else(|| bad(&format!("pane {id} has no known type")))?;
            let mut pane = state::read_record(entry, id, kind);
            if let Some(state) = string(entry, "state").and_then(PaneState::parse) {
                pane.state = state;
            }
            pane.error = string(entry, "error").map(String::from);
            Ok(pane)
        })
        .collect()
}

fn bad(message: &str) -> KmuxError {
    KmuxError::new("bad_request", format!("bad layout request: {message}"))
}

fn ids(value: Option<&Value>) -> Result<Vec<String>, KmuxError> {
    let list = value.and_then(Value::as_array).ok_or_else(|| bad("needs a list of panes"))?;
    list.iter().map(|id| id.as_str().map(String::from).ok_or_else(|| bad("pane ids are strings"))).collect()
}

pub fn placement(at: &Value) -> Result<Placement, KmuxError> {
    let split = match string(at, "split") {
        None | Some("auto") => SplitSide::Auto,
        Some("right") => SplitSide::Right,
        Some("down") => SplitSide::Down,
        Some(_) => return Err(bad("split must be right, down or auto")),
    };
    let size = match at.get("size") {
        None => 0.5,
        Some(size) => size.as_f64().filter(|s| *s > 0.0 && *s < 1.0).ok_or_else(|| bad("size must be a fraction between 0 and 1"))?,
    };
    let dock = Edge::read(at.get("dock")).map_err(|_| bad("dock must be left or right"))?;
    Ok(Placement { window: string(at, "window").map(String::from), split, size, new_tab: at.get("tab") == Some(&Value::Bool(true)), dock })
}

fn spot(reply: &mut Map, spot: Option<Spot>, window: Option<String>) {
    reply.insert("window".into(), opt(spot.as_ref().map(|s| s.window.as_str()).or(window.as_deref())));
    reply.insert("tab".into(), opt(spot.as_ref().and_then(|s| s.tab.as_deref())));
    if let Some(edge) = spot.and_then(|s| s.dock) {
        reply.insert("dock".into(), edge.as_str().into());
    }
}

pub fn serve(host: &mut dyn LayoutHost, request: &Value, panes: &Panes) -> Result<Map, KmuxError> {
    let mut reply = Map::new();
    match string(request, "op").unwrap_or("") {
        "place" => {
            let pane = string(request.get("pane").unwrap_or(&Value::Null), "id").ok_or_else(|| bad("place needs a pane"))?;
            let at = placement(request.get("at").unwrap_or(&Value::Null))?;
            let window = host.place(pane, &at)?;
            spot(&mut reply, host.location(pane), window);
        }
        "location" => {
            let location = string(request, "pane").and_then(|pane| host.location(pane));
            spot(&mut reply, location, None);
        }
        "panes-in" => {
            let scope = match string(request, "tab") {
                Some(tab) => Scope::Tab(tab.into()),
                None => Scope::Window(string(request, "window").map(String::from)),
            };
            reply.insert("panes".into(), host.panes_in(&scope)?.into());
        }
        "remove" => {
            for pane in ids(request.get("ids"))? {
                host.remove(&pane);
            }
            if request.get("tidy") == Some(&Value::Bool(true)) {
                host.tidy();
            }
        }
        "command" => {
            let cmd = string(request, "cmd").unwrap_or("");
            if !host.commands().contains(&cmd) {
                return Err(KmuxError::new("bad_request", format!("unknown command \"{cmd}\"")));
            }
            return host.command(cmd, request.get("args").unwrap_or(&Value::Null), panes);
        }
        "windows" => {
            reply.insert("windows".into(), host.windows(panes));
        }
        "export" => return Ok(host.export_state()),
        op @ ("check" | "install") => {
            let known: HashSet<String> = ids(request.get("ids"))?.into_iter().collect();
            let saved = host.read_state(request.get("state").unwrap_or(&Value::Null), &known)?;
            let mut placed: Vec<String> = saved.placed.iter().cloned().collect();
            placed.sort_by_key(|id| (id.len(), id.clone()));
            if op == "install" {
                host.install_state(saved);
            }
            reply.insert("placed".into(), placed.into());
        }
        other => return Err(bad(&format!("unknown op \"{other}\""))),
    }
    Ok(reply)
}
