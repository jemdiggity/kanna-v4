//! kmux's layout: windows, tabs and split trees. Sizes are fractions of the
//! parent split and always add up to 1. Empty containers close: a tab without
//! panes, then a window without tabs. Mirrors reference/kmux/index.html.

use crate::error::KmuxError;
use crate::fraction;
use crate::json::{num, opt, string, trim, Map, Value};
use crate::layout::{LayoutHost, Placement, SavedLayout, Scope, SplitSide};
use crate::pane::Panes;
use crate::state::{bad, number_in, quoted, whole};
use std::any::Any;
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Row,
    Column,
}

impl Axis {
    pub fn as_str(self) -> &'static str {
        match self {
            Axis::Row => "row",
            Axis::Column => "column",
        }
    }

    pub fn parse(name: &str) -> Option<Axis> {
        match name {
            "row" => Some(Axis::Row),
            "column" => Some(Axis::Column),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Child {
    pub node: Node,
    pub size: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Split {
    pub axis: Axis,
    pub kids: Vec<Child>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Pane(String),
    Split(Split),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tab {
    pub id: String,
    pub title: String,
    pub root: Option<Node>,
    pub last_focus: Option<String>,
}

impl Tab {
    fn new(id: String, title: String) -> Tab {
        Tab { id, title, root: None, last_focus: None }
    }
}

/// Where a window is on screen, in the host's coordinates; saved with the
/// layout so a restored window comes back in place.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub id: String,
    pub tabs: Vec<Tab>,
    pub active: Option<String>,
    pub focused: Option<String>,
    pub zoomed: Option<String>,
    pub frame: Option<Frame>,
    /// Tabs are named "Tab 1", "Tab 2", … in the order the window made them.
    pub tab_count: usize,
}

impl Window {
    fn new(id: String) -> Window {
        Window { id, tabs: vec![], active: None, focused: None, zoomed: None, frame: None, tab_count: 0 }
    }

    fn active_index(&self) -> Option<usize> {
        self.tabs.iter().position(|t| Some(&t.id) == self.active.as_ref()).or(if self.tabs.is_empty() { None } else { Some(0) })
    }

    pub fn active_tab(&self) -> Option<&Tab> {
        self.active_index().map(|i| &self.tabs[i])
    }
}

/// Where a pane is: indices of its window and tab, the path of child indices
/// from the tab's root to its parent split (none if it is the root), and its
/// index in that split.
#[derive(Clone, Debug)]
pub struct Location {
    pub window: usize,
    pub tab: usize,
    pub parent: Option<Vec<usize>>,
    pub index: usize,
}

pub struct Model {
    windows: Vec<Window>,
    pub key: Option<String>,
    counters: (usize, usize),
    /// Whether a pane is wider than it is tall, for `split: auto`.
    pub pane_is_wide: Box<dyn Fn(&str) -> bool>,
}

impl Default for Model {
    fn default() -> Model {
        Model { windows: vec![], key: None, counters: (0, 0), pane_is_wide: Box::new(|_| true) }
    }
}

// MARK: Trees

pub fn pane_ids(node: Option<&Node>) -> Vec<String> {
    fn collect(node: &Node, out: &mut Vec<String>) {
        match node {
            Node::Pane(id) => out.push(id.clone()),
            Node::Split(split) => split.kids.iter().for_each(|kid| collect(&kid.node, out)),
        }
    }
    let mut out = vec![];
    if let Some(node) = node {
        collect(node, &mut out);
    }
    out
}

/// The path to a pane's parent split and its index there, or (None, 0) for the root.
fn locate_in(root: Option<&Node>, id: &str) -> Option<(Option<Vec<usize>>, usize)> {
    fn find(split: &Split, id: &str, here: &mut Vec<usize>) -> Option<(Option<Vec<usize>>, usize)> {
        for (i, kid) in split.kids.iter().enumerate() {
            match &kid.node {
                Node::Pane(pane) if pane == id => return Some((Some(here.clone()), i)),
                Node::Pane(_) => {}
                Node::Split(inner) => {
                    here.push(i);
                    if let Some(found) = find(inner, id, here) {
                        return Some(found);
                    }
                    here.pop();
                }
            }
        }
        None
    }
    match root? {
        Node::Pane(pane) => (pane == id).then_some((None, 0)),
        Node::Split(split) => find(split, id, &mut vec![]),
    }
}

fn split_at<'a>(root: &'a mut Node, path: &[usize]) -> &'a mut Split {
    let mut node = root;
    for &i in path {
        node = match node {
            Node::Split(split) => &mut split.kids[i].node,
            Node::Pane(_) => unreachable!("a path goes through splits"),
        };
    }
    match node {
        Node::Split(split) => split,
        Node::Pane(_) => unreachable!("a path ends at a split"),
    }
}

/// Collapses splits with one child into that child.
pub fn normalize(node: Node) -> Node {
    match node {
        Node::Split(mut split) => {
            split.kids = split.kids.into_iter().map(|kid| Child { node: normalize(kid.node), size: kid.size }).collect();
            if split.kids.len() == 1 { split.kids.pop().unwrap().node } else { Node::Split(split) }
        }
        pane => pane,
    }
}

pub(crate) fn fraction(value: &Value) -> Result<f64, KmuxError> {
    fraction::parse(value).ok_or_else(|| KmuxError::new("bad_request", format!("\"{}\" is not a fraction", describe(value))))
}

/// How Swift prints a JSON value, for messages.
fn describe(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(b) => format!("bool({b})"),
        Value::Number(n) => format!("number({})", swift_double(n.as_f64().unwrap_or(0.0))),
        Value::String(s) => format!("string({})", swift_quoted(s)),
        Value::Array(items) => format!("array([{}])", items.iter().map(|item| format!("KmuxCore.JSON.{}", describe(item))).collect::<Vec<_>>().join(", ")),
        Value::Object(fields) if fields.is_empty() => "object([:])".into(),
        Value::Object(fields) => format!(
            "object([{}])",
            fields.iter().map(|(key, item)| format!("{}: KmuxCore.JSON.{}", swift_quoted(key), describe(item))).collect::<Vec<_>>().join(", ")
        ),
    }
}

fn swift_double(value: f64) -> String {
    let size = value.abs();
    if !value.is_finite() || size == 0.0 || (size >= 1e-4 && size <= 9007199254740992.0) {
        return format!("{value:?}");
    }
    let text = format!("{value:e}");
    let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
    let (sign, digits) = match exponent.strip_prefix('-') {
        Some(digits) => ('-', digits),
        None => ('+', exponent),
    };
    format!("{mantissa}e{sign}{digits:0>2}")
}

fn swift_quoted(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\'' => out.push_str("\\'"),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            c if c.is_ascii_control() => out.push_str(&format!("\\u{{{:X}}}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn not_found(message: String) -> KmuxError {
    KmuxError::new("not_found", message)
}

fn bad_request(message: impl Into<String>) -> KmuxError {
    KmuxError::new("bad_request", message)
}

fn layout_invalid(message: impl Into<String>) -> KmuxError {
    KmuxError::new("layout_invalid", message)
}

impl Model {
    pub fn new() -> Model {
        Model::default()
    }

    pub fn windows(&self) -> &[Window] {
        &self.windows
    }

    pub fn window(&self, id: &str) -> Option<&Window> {
        self.windows.iter().find(|w| w.id == id)
    }

    pub fn window_mut(&mut self, id: &str) -> Option<&mut Window> {
        self.windows.iter_mut().find(|w| w.id == id)
    }

    pub fn key_window(&self) -> Option<&Window> {
        self.windows.iter().find(|w| Some(&w.id) == self.key.as_ref())
    }

    fn window_index(&self, id: &str) -> Option<usize> {
        self.windows.iter().position(|w| w.id == id)
    }

    /// A tab's window and index there.
    fn tab_index(&self, id: &str) -> Option<(usize, usize)> {
        self.windows.iter().enumerate().find_map(|(w, window)| window.tabs.iter().position(|t| t.id == id).map(|t| (w, t)))
    }

    pub fn tab(&self, id: &str) -> Option<&Tab> {
        self.tab_index(id).map(|(w, t)| &self.windows[w].tabs[t])
    }

    /// The window a tab is in.
    pub fn window_of(&self, tab: &str) -> Option<&Window> {
        self.tab_index(tab).map(|(w, _)| &self.windows[w])
    }

    fn make_window(&mut self) -> usize {
        self.counters.1 += 1;
        let window = Window::new(format!("w{}", self.counters.1));
        self.key = Some(window.id.clone());
        self.windows.push(window);
        self.windows.len() - 1
    }

    fn make_tab(&mut self, w: usize, activate: bool) -> usize {
        self.counters.0 += 1;
        let window = &mut self.windows[w];
        window.tab_count += 1;
        let tab = Tab::new(format!("t{}", self.counters.0), format!("Tab {}", window.tab_count));
        if activate || window.active.is_none() {
            window.active = Some(tab.id.clone());
        }
        window.tabs.push(tab);
        window.tabs.len() - 1
    }

    pub fn locate(&self, id: &str) -> Option<Location> {
        for (w, window) in self.windows.iter().enumerate() {
            for (t, tab) in window.tabs.iter().enumerate() {
                if let Some((parent, index)) = locate_in(tab.root.as_ref(), id) {
                    return Some(Location { window: w, tab: t, parent, index });
                }
            }
        }
        None
    }

    /// Removes a pane from its tree; its siblings share the freed space.
    fn detach(&mut self, id: &str) {
        let Some(location) = self.locate(id) else { return };
        let window = &mut self.windows[location.window];
        let tab = &mut window.tabs[location.tab];
        if let Some(path) = &location.parent {
            let parent = split_at(tab.root.as_mut().unwrap(), path);
            let gone = parent.kids.remove(location.index).size;
            let count = parent.kids.len() as f64;
            for kid in &mut parent.kids {
                kid.size = if gone < 1.0 { kid.size / (1.0 - gone) } else { 1.0 / count };
            }
            tab.root = tab.root.take().map(normalize);
        } else {
            tab.root = None;
        }
        if window.zoomed.as_deref() == Some(id) {
            window.zoomed = None;
        }
    }

    /// Puts `node` next to the target pane, taking `size` of the target's space.
    /// A parent split along the same axis takes it as a sibling.
    fn insert(&mut self, node: Node, beside: &str, (w, t): (usize, usize), axis: Axis, after: bool, size: f64) {
        let tab = &mut self.windows[w].tabs[t];
        let Some((parent, index)) = locate_in(tab.root.as_ref(), beside) else { return };
        if let Some(path) = &parent {
            let split = split_at(tab.root.as_mut().unwrap(), path);
            if split.axis == axis {
                let share = split.kids[index].size;
                split.kids[index].size = share * (1.0 - size);
                split.kids.insert(index + after as usize, Child { node, size: share * size });
                return;
            }
        }
        let mut kids = vec![Child { node: Node::Pane(beside.into()), size: 1.0 - size }, Child { node, size }];
        if !after {
            kids.reverse();
        }
        let split = Node::Split(Split { axis, kids });
        match &parent {
            Some(path) => split_at(tab.root.as_mut().unwrap(), path).kids[index].node = split,
            None => tab.root = Some(split),
        }
    }

    /// Adds a pane to a tab: as its root, or as a new column on the right
    /// (all columns then share the width equally).
    fn append(&mut self, id: &str, (w, t): (usize, usize)) {
        let tab = &mut self.windows[w].tabs[t];
        let pane = Node::Pane(id.into());
        tab.root = Some(match tab.root.take() {
            None => pane,
            Some(Node::Split(mut split)) if split.axis == Axis::Row => {
                let count = split.kids.len() as f64;
                for kid in &mut split.kids {
                    kid.size *= count / (count + 1.0);
                }
                split.kids.push(Child { node: pane, size: 1.0 / (count + 1.0) });
                Node::Split(split)
            }
            Some(root) => Node::Split(Split { axis: Axis::Row, kids: vec![Child { node: root, size: 0.5 }, Child { node: pane, size: 0.5 }] }),
        });
    }

    /// Puts pane `b` where pane `a` is, and `a` where `b` is.
    fn swap(&mut self, a: &str, b: &str) {
        let (Some(la), Some(lb)) = (self.locate(a), self.locate(b)) else { return };
        self.set_leaf(&la, b);
        self.set_leaf(&lb, a);
    }

    fn set_leaf(&mut self, location: &Location, id: &str) {
        let tab = &mut self.windows[location.window].tabs[location.tab];
        match &location.parent {
            Some(path) => split_at(tab.root.as_mut().unwrap(), path).kids[location.index].node = Node::Pane(id.into()),
            None => tab.root = Some(Node::Pane(id.into())),
        }
    }

    fn move_tab_to(&mut self, tab: &str, destination: usize, index: Option<usize>) {
        let Some((source, from)) = self.tab_index(tab) else { return };
        let tab = self.windows[source].tabs.remove(from);
        let mut index = index.unwrap_or(self.windows[destination].tabs.len());
        if source == destination && index > from {
            index -= 1;
        }
        let tabs = &mut self.windows[destination].tabs;
        tabs.insert(index.min(tabs.len()), tab);
    }

    /// Closes empty tabs and windows and repairs focus.
    fn drop_empty(&mut self) {
        for window in &mut self.windows {
            let at = window.tabs.iter().position(|t| Some(&t.id) == window.active.as_ref()).unwrap_or(0);
            window.tabs.retain(|t| t.root.is_some());
            if !window.tabs.iter().any(|t| Some(&t.id) == window.active.as_ref()) {
                window.active = if window.tabs.is_empty() { None } else { Some(window.tabs[at.min(window.tabs.len() - 1)].id.clone()) };
            }
            let ids = pane_ids(window.active_tab().and_then(|t| t.root.as_ref()));
            if window.focused.as_ref().is_some_and(|f| ids.contains(f)) {
                continue;
            }
            let last = window.active_tab().and_then(|t| t.last_focus.clone()).filter(|l| ids.contains(l));
            window.focused = last.or_else(|| ids.first().cloned());
            if window.zoomed.as_ref().is_some_and(|z| !ids.contains(z)) {
                window.zoomed = None;
            }
        }
        self.windows.retain(|w| !w.tabs.is_empty());
        if self.key_window().is_none() {
            self.key = self.windows.last().map(|w| w.id.clone());
        }
    }

    fn target_window(&mut self, reference: Option<&str>) -> Result<usize, KmuxError> {
        match reference {
            None => Ok(match self.key.clone().and_then(|k| self.window_index(&k)) {
                Some(w) => w,
                None => self.make_window(),
            }),
            Some("new") => Ok(self.make_window()),
            Some(id) => self.window_index(id).ok_or_else(|| not_found(format!("no window \"{id}\""))),
        }
    }

    fn need_tab(&self, reference: Option<&str>) -> Result<(usize, usize), KmuxError> {
        reference.and_then(|r| self.tab_index(r)).ok_or_else(|| not_found(format!("no tab \"{}\"", reference.unwrap_or(""))))
    }

    fn focus_pane(&mut self, id: &str) {
        let Some(location) = self.locate(id) else { return };
        let window = &mut self.windows[location.window];
        window.active = Some(window.tabs[location.tab].id.clone());
        if window.zoomed.as_ref().is_some_and(|z| z != id) {
            window.zoomed = None;
        }
        window.focused = Some(id.into());
        window.tabs[location.tab].last_focus = Some(id.into());
        self.key = Some(window.id.clone());
    }

    /// The pane `step` places after the focused one in reading order, wrapping.
    pub fn cycle_pane(&self, window: &str, step: isize) -> Option<String> {
        let window = self.window(window)?;
        let ids = pane_ids(window.active_tab().and_then(|t| t.root.as_ref()));
        if ids.is_empty() {
            return None;
        }
        let at = window.focused.as_ref().and_then(|f| ids.iter().position(|id| id == f)).unwrap_or(0) as isize;
        let count = ids.len() as isize;
        Some(ids[((at + step) % count + count) as usize % ids.len()].clone())
    }

    /// The tab `step` places after the active one, wrapping.
    pub fn cycle_tab(&self, window: &str, step: isize) -> Option<String> {
        let window = self.window(window)?;
        if window.tabs.is_empty() {
            return None;
        }
        let at = window.tabs.iter().position(|t| Some(&t.id) == window.active.as_ref()).unwrap_or(0) as isize;
        let count = window.tabs.len() as isize;
        Some(window.tabs[((at + step) % count + count) as usize % window.tabs.len()].id.clone())
    }

    // MARK: Views for the protocol

    pub fn tree(&self, node: Option<&Node>, size: Option<f64>, panes: &Panes) -> Value {
        let Some(node) = node else { return Value::Null };
        let mut out = Map::new();
        match node {
            Node::Pane(id) => {
                out.insert("pane".into(), panes.display(id).into());
            }
            Node::Split(split) => {
                out.insert("split".into(), split.axis.as_str().into());
                out.insert("children".into(), Value::Array(split.kids.iter().map(|kid| self.tree(Some(&kid.node), Some(kid.size), panes)).collect()));
            }
        }
        if let Some(size) = size {
            if size < 1.0 - 1e-6 {
                out.insert("size".into(), fraction::format(size).into());
            }
        }
        Value::Object(out)
    }

    fn tab_tree(&self, tab: &str, panes: &Panes) -> Value {
        self.tab(tab).map_or(Value::Null, |t| self.tree(t.root.as_ref(), None, panes))
    }

    // MARK: Commands (ported from reference/kmux/core.js)

    fn focus(&mut self, args: &Value, panes: &Panes) -> Result<Map, KmuxError> {
        if let Some(reference) = string(args, "pane") {
            let id = panes.need(Some(reference))?.id.clone();
            self.focus_pane(&id);
        } else if let Some(reference) = string(args, "tab") {
            let (w, t) = self.tab_index(reference).ok_or_else(|| not_found(format!("no tab \"{reference}\"")))?;
            let window = &mut self.windows[w];
            let ids = pane_ids(window.tabs[t].root.as_ref());
            window.active = Some(window.tabs[t].id.clone());
            window.zoomed = None;
            window.focused = window.tabs[t].last_focus.clone().filter(|l| ids.contains(l)).or_else(|| ids.first().cloned());
            self.key = Some(window.id.clone());
        } else if args.get("window").is_some() {
            let w = self.target_window(string(args, "window"))?;
            self.key = Some(self.windows[w].id.clone());
        } else {
            return Err(bad_request("focus needs a pane, tab or window"));
        }
        Ok(reply([("window", opt(self.key.as_deref()))]))
    }

    fn zoom(&mut self, args: &Value, panes: &Panes) -> Result<Map, KmuxError> {
        let id = panes.need(string(args, "pane"))?.id.clone();
        let w = self.locate(&id).ok_or_else(|| not_found(format!("no pane \"{id}\"")))?.window;
        let was = self.windows[w].zoomed.as_deref() == Some(&id);
        self.focus_pane(&id);
        let window = &mut self.windows[w];
        window.zoomed = if was { None } else { Some(id) };
        Ok(reply([("zoomed", Value::Bool(window.zoomed.is_some()))]))
    }

    fn rename_tab(&mut self, args: &Value) -> Result<Map, KmuxError> {
        let reference = string(args, "tab").unwrap_or("");
        let (w, t) = self.tab_index(reference).ok_or_else(|| not_found(format!("no tab \"{reference}\"")))?;
        let title = trim(string(args, "title").unwrap_or(""));
        if title.is_empty() {
            return Err(bad_request("a tab title cannot be empty"));
        }
        let tab = &mut self.windows[w].tabs[t];
        tab.title = title.into();
        Ok(reply([("tab", tab.id.clone().into()), ("title", title.into())]))
    }

    fn move_pane(&mut self, args: &Value, panes: &Panes) -> Result<Map, KmuxError> {
        let id = panes.need(string(args, "pane"))?.id.clone();
        let from = self.locate(&id).ok_or_else(|| not_found(format!("no pane \"{id}\"")))?;
        let from_window = self.windows[from.window].id.clone();
        let from_tab = self.windows[from.window].tabs[from.tab].id.clone();
        if let Some(reference) = string(args, "to") {
            let target = panes.need(Some(reference))?.id.clone();
            let side = string(args, "side").unwrap_or("swap");
            if !["left", "right", "top", "bottom", "swap"].contains(&side) {
                return Err(bad_request("side must be left, right, top, bottom or swap"));
            }
            if target == id {
                return Ok(reply([("window", from_window.into())]));
            }
            if side == "swap" {
                self.swap(&id, &target);
                let window = &mut self.windows[from.window];
                if window.focused.as_deref() == Some(&id) {
                    window.focused = Some(target);
                }
            } else {
                self.detach(&id);
                let location = self.locate(&target).ok_or_else(|| not_found(format!("no pane \"{reference}\"")))?;
                let axis = if side == "left" || side == "right" { Axis::Row } else { Axis::Column };
                self.insert(Node::Pane(id.clone()), &target, (location.window, location.tab), axis, side == "right" || side == "bottom", 0.5);
            }
        } else {
            let tab = string(args, "tab");
            let destination = if let Some(reference) = tab.filter(|r| *r != "new") {
                let (w, t) = self.need_tab(Some(reference))?;
                self.windows[w].tabs[t].id.clone()
            } else if tab == Some("new") {
                let w = if args.get("window").is_some() { self.target_window(string(args, "window"))? } else { from.window };
                let t = self.make_tab(w, true);
                self.windows[w].tabs[t].id.clone()
            } else if args.get("window").is_some() {
                let w = self.target_window(string(args, "window"))?;
                let t = match self.windows[w].active_index() {
                    Some(t) => t,
                    None => self.make_tab(w, true),
                };
                self.windows[w].tabs[t].id.clone()
            } else {
                return Err(bad_request("move needs to + side, tab, or window"));
            };
            if destination == from_tab {
                return Ok(reply([("window", from_window.into())]));
            }
            self.detach(&id);
            let at = self.tab_index(&destination).unwrap();
            self.append(&id, at);
        }
        self.focus_pane(&id);
        self.drop_empty();
        let Some(location) = self.locate(&id) else { return Ok(Map::new()) };
        let window = &self.windows[location.window];
        let tab = &window.tabs[location.tab];
        Ok(reply([("window", window.id.clone().into()), ("tab", tab.id.clone().into()), ("layout", self.tree(tab.root.as_ref(), None, panes))]))
    }

    fn move_tab(&mut self, args: &Value) -> Result<Map, KmuxError> {
        let (source, t) = self.need_tab(string(args, "tab"))?;
        let tab = self.windows[source].tabs[t].id.clone();
        let destination = match args.get("window") {
            Some(window) => self.target_window(window.as_str())?,
            None => source,
        };
        let mut index = None;
        if let Some(value) = args.get("index") {
            match value.as_f64() {
                Some(n) if n >= 0.0 && n.round() == n => index = Some(n as usize),
                _ => return Err(bad_request("index must be a whole number ≥ 0")),
            }
        }
        self.move_tab_to(&tab, destination, index);
        let (d, t) = self.tab_index(&tab).unwrap();
        let ids = pane_ids(self.windows[d].tabs[t].root.as_ref());
        let window = &mut self.windows[d];
        window.active = Some(tab.clone());
        window.focused = window.tabs[t].last_focus.clone().filter(|l| ids.contains(l)).or_else(|| ids.first().cloned());
        window.zoomed = None;
        let id = window.id.clone();
        self.key = Some(id.clone());
        self.drop_empty();
        let tabs = self.window(&id).map(|w| w.tabs.iter().map(|t| Value::String(t.id.clone())).collect()).unwrap_or_default();
        Ok(reply([("window", id.into()), ("tabs", Value::Array(tabs))]))
    }

    fn resize(&mut self, args: &Value, panes: &Panes) -> Result<Map, KmuxError> {
        let pane = panes.need(string(args, "pane"))?;
        let size = fraction(args.get("size").unwrap_or(&Value::Null))?;
        if !(size > 0.0 && size < 1.0) {
            return Err(bad_request("size must be a fraction between 0 and 1"));
        }
        let location = self.locate(&pane.id).ok_or_else(|| not_found(format!("no pane \"{}\"", pane.id)))?;
        let Some(path) = &location.parent else {
            return Err(bad_request(format!("{} fills its tab, so there is nothing to resize against", pane.label())));
        };
        let tab = &mut self.windows[location.window].tabs[location.tab];
        let parent = split_at(tab.root.as_mut().unwrap(), path);
        let old = parent.kids[location.index].size;
        let others = (parent.kids.len() - 1) as f64;
        for (i, kid) in parent.kids.iter_mut().enumerate() {
            if i != location.index {
                kid.size = if old < 1.0 - 1e-9 { kid.size * (1.0 - size) / (1.0 - old) } else { (1.0 - size) / others };
            }
        }
        parent.kids[location.index].size = size;
        let tab = &self.windows[location.window].tabs[location.tab];
        Ok(reply([("layout", self.tree(tab.root.as_ref(), None, panes))]))
    }

    fn arrange(&mut self, args: &Value, panes: &Panes) -> Result<Map, KmuxError> {
        fn build(node: &Value, used: &mut Vec<String>, panes: &Panes) -> Result<Node, KmuxError> {
            if let Some(reference) = string(node, "pane") {
                let id = panes.need(Some(reference))?.id.clone();
                if used.contains(&id) {
                    return Err(layout_invalid(format!("\"{reference}\" appears more than once")));
                }
                used.push(id.clone());
                return Ok(Node::Pane(id));
            }
            let axis = string(node, "split").and_then(Axis::parse);
            let children = node.get("children").and_then(Value::as_array).filter(|c| !c.is_empty());
            let (Some(axis), Some(children)) = (axis, children) else { return Err(layout_invalid("malformed layout tree")) };
            let sizes = children
                .iter()
                .map(|child| match child.get("size") {
                    None | Some(Value::Null) => Ok(None),
                    Some(value) => {
                        let size = fraction(value)?;
                        if !(size > 0.0 && size <= 1.0) {
                            return Err(layout_invalid("sizes must be fractions between 0 and 1"));
                        }
                        Ok(Some(size))
                    }
                })
                .collect::<Result<Vec<_>, _>>()?;
            let given: f64 = sizes.iter().flatten().sum();
            let unsized_count = sizes.iter().filter(|s| s.is_none()).count();
            if given > 1.0 + 1e-9 {
                return Err(layout_invalid(format!("sizes in a {} add up to {}, more than 1", axis.as_str(), fraction::format(given))));
            }
            if unsized_count > 0 && given >= 1.0 - 1e-9 {
                return Err(layout_invalid(format!("no space left for the unsized panes in a {}", axis.as_str())));
            }
            let fill = if unsized_count > 0 { (1.0 - given) / unsized_count as f64 } else { 0.0 };
            let scale = if unsized_count > 0 { 1.0 } else { 1.0 / given };
            let kids = children
                .iter()
                .zip(sizes)
                .map(|(child, size)| Ok(Child { node: build(child, used, panes)?, size: size.unwrap_or(fill) * scale }))
                .collect::<Result<Vec<_>, KmuxError>>()?;
            Ok(Node::Split(Split { axis, kids }))
        }
        let mut used = vec![];
        let root = normalize(build(args.get("layout").unwrap_or(&Value::Null), &mut used, panes)?);
        let w = self.target_window(string(args, "window"))?;
        let t = match self.windows[w].active_index() {
            Some(t) => t,
            None => self.make_tab(w, true),
        };
        let tab = self.windows[w].tabs[t].id.clone();
        for id in &used {
            self.detach(id);
        }
        let (w, t) = self.tab_index(&tab).unwrap();
        let leftovers = pane_ids(self.windows[w].tabs[t].root.as_ref());
        self.windows[w].tabs[t].root = Some(root);
        if !leftovers.is_empty() {
            let rest = self.make_tab(w, false);
            let share = 1.0 / leftovers.len() as f64;
            let kids = leftovers.into_iter().map(|id| Child { node: Node::Pane(id), size: share }).collect();
            let rest = &mut self.windows[w].tabs[rest];
            rest.title = "unarranged".into();
            rest.root = Some(normalize(Node::Split(Split { axis: Axis::Row, kids })));
        }
        let window = &mut self.windows[w];
        window.zoomed = None;
        window.active = Some(tab.clone());
        if !window.focused.as_ref().is_some_and(|f| used.contains(f)) {
            window.focused = used.first().cloned();
        }
        let id = window.id.clone();
        self.key = Some(id.clone());
        self.drop_empty();
        Ok(reply([("window", id.into()), ("layout", self.tab_tree(&tab, panes))]))
    }

    // MARK: Persistence

    fn export_node(node: &Node) -> Value {
        match node {
            Node::Pane(id) => Value::Object(reply([("pane", id.clone().into())])),
            Node::Split(split) => {
                let children = split
                    .kids
                    .iter()
                    .map(|kid| {
                        let mut fields = match Model::export_node(&kid.node) {
                            Value::Object(fields) => fields,
                            _ => Map::new(),
                        };
                        fields.insert("size".into(), num(kid.size));
                        Value::Object(fields)
                    })
                    .collect();
                Value::Object(reply([("split", split.axis.as_str().into()), ("children", Value::Array(children))]))
            }
        }
    }
}

fn reply<const N: usize>(fields: [(&str, Value); N]) -> Map {
    fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

struct Restored {
    windows: Vec<Window>,
    key: Option<String>,
    counters: (usize, usize),
}

impl LayoutHost for Model {
    fn commands(&self) -> &[&'static str] {
        &["focus", "zoom", "rename-tab", "move", "move-tab", "resize", "arrange"]
    }

    fn features(&self) -> &[&'static str] {
        &["windows", "tabs", "fractionalSizing", "zoom"]
    }

    fn place(&mut self, pane: &str, at: &Placement) -> Result<Option<String>, KmuxError> {
        let w = self.target_window(at.window.as_deref())?;
        let t = if at.new_tab || self.windows[w].tabs.is_empty() { self.make_tab(w, true) } else { self.windows[w].active_index().unwrap() };
        self.windows[w].zoomed = None;
        let tab = &mut self.windows[w].tabs[t];
        if tab.root.is_none() {
            tab.root = Some(Node::Pane(pane.into()));
        } else {
            let ids = pane_ids(tab.root.as_ref());
            let target = self.windows[w].focused.clone().filter(|f| ids.contains(f)).unwrap_or_else(|| ids.last().unwrap().clone());
            let axis = match at.split {
                SplitSide::Right => Axis::Row,
                SplitSide::Down => Axis::Column,
                SplitSide::Auto if (self.pane_is_wide)(&target) => Axis::Row,
                SplitSide::Auto => Axis::Column,
            };
            self.insert(Node::Pane(pane.into()), &target, (w, t), axis, true, at.size);
        }
        let window = &mut self.windows[w];
        window.focused = Some(pane.into());
        if let Some(active) = window.active_index() {
            window.tabs[active].last_focus = Some(pane.into());
        }
        self.key = Some(window.id.clone());
        Ok(Some(window.id.clone()))
    }

    fn location(&self, pane: &str) -> Option<(String, String)> {
        let location = self.locate(pane)?;
        let window = &self.windows[location.window];
        Some((window.id.clone(), window.tabs[location.tab].id.clone()))
    }

    fn panes_in(&mut self, scope: &Scope) -> Result<Vec<String>, KmuxError> {
        match scope {
            Scope::Tab(reference) => {
                let (w, t) = self.tab_index(reference).ok_or_else(|| not_found(format!("no tab \"{reference}\"")))?;
                Ok(pane_ids(self.windows[w].tabs[t].root.as_ref()))
            }
            Scope::Window(reference) => {
                let w = self.target_window(reference.as_deref())?;
                Ok(self.windows[w].tabs.iter().flat_map(|t| pane_ids(t.root.as_ref())).collect())
            }
        }
    }

    fn remove(&mut self, pane: &str) {
        self.detach(pane);
    }

    fn tidy(&mut self) {
        self.drop_empty();
    }

    fn command(&mut self, cmd: &str, args: &Value, panes: &Panes) -> Result<Map, KmuxError> {
        match cmd {
            "focus" => self.focus(args, panes),
            "zoom" => self.zoom(args, panes),
            "rename-tab" => self.rename_tab(args),
            "move" => self.move_pane(args, panes),
            "move-tab" => self.move_tab(args),
            "resize" => self.resize(args, panes),
            "arrange" => self.arrange(args, panes),
            _ => Err(bad_request(format!("unknown command \"{cmd}\""))),
        }
    }

    fn windows(&self, panes: &Panes) -> Value {
        Value::Array(
            self.windows
                .iter()
                .map(|window| {
                    let tabs = window
                        .tabs
                        .iter()
                        .map(|tab| {
                            Value::Object(reply([
                                ("id", tab.id.clone().into()),
                                ("title", tab.title.clone().into()),
                                ("active", Value::Bool(Some(&tab.id) == window.active.as_ref())),
                                ("layout", self.tree(tab.root.as_ref(), None, panes)),
                            ]))
                        })
                        .collect();
                    Value::Object(reply([
                        ("id", window.id.clone().into()),
                        ("key", Value::Bool(Some(&window.id) == self.key.as_ref())),
                        ("focused", window.focused.as_ref().map_or(Value::Null, |p| panes.display(p).into())),
                        ("zoomed", window.zoomed.as_ref().map_or(Value::Null, |p| panes.display(p).into())),
                        ("tabs", Value::Array(tabs)),
                    ]))
                })
                .collect(),
        )
    }

    fn export_state(&self) -> Map {
        let windows = self
            .windows
            .iter()
            .map(|window| {
                let tabs = window
                    .tabs
                    .iter()
                    .map(|tab| {
                        Value::Object(reply([
                            ("id", tab.id.clone().into()),
                            ("title", tab.title.clone().into()),
                            ("lastFocus", opt(tab.last_focus.as_deref())),
                            ("layout", tab.root.as_ref().map_or(Value::Null, Model::export_node)),
                        ]))
                    })
                    .collect();
                let mut out = reply([
                    ("id", window.id.clone().into()),
                    ("active", opt(window.active.as_deref())),
                    ("focused", opt(window.focused.as_deref())),
                    ("zoomed", opt(window.zoomed.as_deref())),
                    ("tabCount", num(window.tab_count as f64)),
                    ("tabs", Value::Array(tabs)),
                ]);
                if let Some(f) = window.frame {
                    out.insert("frame".into(), Value::Object(reply([("x", num(f.x)), ("y", num(f.y)), ("w", num(f.w)), ("h", num(f.h))])));
                }
                Value::Object(out)
            })
            .collect();
        reply([
            ("key", opt(self.key.as_deref())),
            ("counters", Value::Object(reply([("tab", num(self.counters.0 as f64)), ("window", num(self.counters.1 as f64))]))),
            ("windows", Value::Array(windows)),
        ])
    }

    /// Lenient where it can repair (dangling focus, empty tabs, sizes),
    /// strict about anything that would make the layout ambiguous.
    fn read_state(&self, state: &Value, panes: &HashSet<String>) -> Result<SavedLayout, KmuxError> {
        let saved = state.get("counters").cloned().unwrap_or(Value::Object(Map::new()));
        let mut counters = (whole(saved.get("tab")).unwrap_or(0), whole(saved.get("window")).unwrap_or(0));
        let saved_windows = state.get("windows").and_then(Value::as_array).ok_or_else(|| bad("needs panes and windows"))?;

        let mut placed = HashSet::new();
        let mut tab_ids = HashSet::new();
        let mut window_ids = HashSet::new();
        fn build(node: &Value, panes: &HashSet<String>, placed: &mut HashSet<String>) -> Result<Node, KmuxError> {
            if !node.is_object() {
                return Err(bad("malformed layout"));
            }
            if let Some(reference) = node.get("pane").filter(|r| !r.is_null()) {
                let id = reference.as_str().filter(|id| panes.contains(*id)).ok_or_else(|| bad(&format!("layout names unknown pane {}", quoted(Some(reference)))))?;
                if !placed.insert(id.to_string()) {
                    return Err(bad(&format!("pane {id} is placed twice")));
                }
                return Ok(Node::Pane(id.into()));
            }
            let axis = string(node, "split").and_then(Axis::parse);
            let children = node.get("children").and_then(Value::as_array).filter(|c| !c.is_empty());
            let (Some(axis), Some(children)) = (axis, children) else { return Err(bad("malformed layout")) };
            let sizes: Vec<f64> = children
                .iter()
                .map(|child| child.get("size").and_then(Value::as_f64).filter(|s| *s > 0.0 && *s <= 1.0).unwrap_or(1.0 / children.len() as f64))
                .collect();
            let total: f64 = sizes.iter().sum();
            let kids = children
                .iter()
                .zip(sizes)
                .map(|(child, size)| Ok(Child { node: build(child, panes, placed)?, size: size / total }))
                .collect::<Result<Vec<_>, KmuxError>>()?;
            Ok(Node::Split(Split { axis, kids }))
        }

        let mut windows = vec![];
        for item in saved_windows {
            let id = string(item, "id").filter(|_| item.is_object());
            let n = id.and_then(|id| number_in(id, "w"));
            let saved_tabs = item.get("tabs").and_then(Value::as_array);
            let (Some(id), Some(n), Some(saved_tabs)) = (id, n, saved_tabs) else { return Err(bad(&format!("window {}", quoted(item.get("id"))))) };
            if !window_ids.insert(id.to_string()) {
                return Err(bad(&format!("window {}", quoted(item.get("id")))));
            }
            let mut window = Window::new(id.into());
            if let Some(f) = item.get("frame") {
                let get = |k: &str| f.get(k).and_then(Value::as_f64);
                if let (Some(x), Some(y), Some(w), Some(h)) = (get("x"), get("y"), get("w"), get("h")) {
                    if [x, y, w, h].iter().all(|v| v.is_finite()) && w > 0.0 && h > 0.0 {
                        window.frame = Some(Frame { x, y, w, h });
                    }
                }
            }
            for entry in saved_tabs {
                let tab_id = string(entry, "id").filter(|_| entry.is_object());
                let tn = tab_id.and_then(|id| number_in(id, "t"));
                let (Some(tab_id), Some(tn)) = (tab_id, tn) else { return Err(bad(&format!("tab {}", quoted(entry.get("id"))))) };
                if !tab_ids.insert(tab_id.to_string()) {
                    return Err(bad(&format!("tab {}", quoted(entry.get("id")))));
                }
                let root = match entry.get("layout") {
                    None | Some(Value::Null) => None,
                    Some(layout) => Some(normalize(build(layout, panes, &mut placed)?)),
                };
                let title = trim(string(entry, "title").unwrap_or(""));
                let title = if title.is_empty() { format!("Tab {}", window.tabs.len() + 1) } else { title.into() };
                let mut tab = Tab::new(tab_id.into(), title);
                let ids = pane_ids(root.as_ref());
                tab.root = root;
                tab.last_focus = string(entry, "lastFocus").filter(|l| ids.iter().any(|id| id == l)).map(String::from);
                window.tabs.push(tab);
                counters.0 = counters.0.max(tn);
            }
            window.tabs.retain(|t| t.root.is_some());
            let active = string(item, "active");
            window.active = if window.tabs.iter().any(|t| Some(t.id.as_str()) == active) { active.map(String::from) } else { window.tabs.first().map(|t| t.id.clone()) };
            let shown = pane_ids(window.active_tab().and_then(|t| t.root.as_ref()));
            let focused = string(item, "focused").filter(|f| shown.iter().any(|s| s == f)).map(String::from);
            window.focused = focused.or_else(|| window.active_tab().and_then(|t| t.last_focus.clone())).or_else(|| shown.first().cloned());
            window.zoomed = string(item, "zoomed").filter(|z| shown.iter().any(|s| s == z)).map(String::from);
            window.tab_count = whole(item.get("tabCount")).unwrap_or(0).max(window.tabs.len());
            windows.push(window);
            counters.1 = counters.1.max(n);
        }
        windows.retain(|w: &Window| !w.tabs.is_empty());
        let key = string(state, "key").filter(|k| windows.iter().any(|w| w.id == *k)).map(String::from).or_else(|| windows.last().map(|w| w.id.clone()));

        Ok(SavedLayout { placed, layout: Box::new(Restored { windows, key, counters }) })
    }

    fn install_state(&mut self, saved: SavedLayout) {
        let Ok(restored) = saved.layout.downcast::<Restored>() else { return };
        self.windows = restored.windows;
        self.key = restored.key;
        self.counters = (self.counters.0.max(restored.counters.0), self.counters.1.max(restored.counters.1));
        self.drop_empty();
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn values_are_described_as_swift_prints_them() {
        assert_eq!(describe(&json!(null)), "null");
        assert_eq!(describe(&json!(true)), "bool(true)");
        assert_eq!(describe(&json!(100)), "number(100.0)");
        assert_eq!(describe(&json!(-1e20)), "number(-1e+20)");
        assert_eq!(describe(&json!(-0.00001)), "number(-1e-05)");
        assert_eq!(describe(&json!(9.5e15)), "number(9.5e+15)");
        assert_eq!(describe(&json!(9007199254740992.0)), "number(9007199254740992.0)");
        assert_eq!(describe(&json!(1.25e-7)), "number(1.25e-07)");
        assert_eq!(describe(&json!(0.0001)), "number(0.0001)");
        assert_eq!(describe(&json!("a'b\u{1b}é\u{7f}\t\0\\\"")), "string(\"a\\'b\\u{1B}é\\u{7F}\\t\\0\\\\\\\"\")");
        assert_eq!(describe(&json!([])), "array([])");
        assert_eq!(describe(&json!({})), "object([:])");
        assert_eq!(
            describe(&json!([[null], { "k": "v" }, 1, false])),
            "array([KmuxCore.JSON.array([KmuxCore.JSON.null]), KmuxCore.JSON.object([\"k\": KmuxCore.JSON.string(\"v\")]), KmuxCore.JSON.number(1.0), KmuxCore.JSON.bool(false)])"
        );
    }
}
