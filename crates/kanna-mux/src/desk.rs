use crate::error::KmuxError;
use crate::json::{num, Map, Value};
use crate::layout::Edge;
use crate::model::{bad_request, normalize, not_found, pane_ids, Child, Dock, Model, Node, Split, Tab, Window, DOCK_WIDTH};
use crate::pane::Panes;
use std::time::{SystemTime, UNIX_EPOCH};

fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64().floor())
}

fn reply<const N: usize>(fields: [(&str, Value); N]) -> Map {
    fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

impl Model {
    pub fn shelf(&self) -> impl Iterator<Item = &Window> {
        self.parked.iter().filter(|p| !p.backlog)
    }

    pub fn parked_panes(&self) -> Vec<String> {
        self.parked.iter().flat_map(|p| p.tabs.iter().flat_map(|t| pane_ids(t.root.as_ref())).chain(p.dock.iter().map(|d| d.pane.clone()))).collect()
    }

    pub(crate) fn detach_parked(&mut self, id: &str) -> bool {
        for parked in &mut self.parked {
            if parked.dock.as_ref().is_some_and(|d| d.pane == id) {
                parked.dock = None;
                return true;
            }
            for tab in &mut parked.tabs {
                if pane_ids(tab.root.as_ref()).iter().any(|p| p == id) {
                    tab.root = tab.root.take().and_then(|root| without(root, id));
                    return true;
                }
            }
        }
        false
    }

    pub(crate) fn settle_shelf(&mut self) {
        for parked in &mut self.parked {
            parked.tabs.retain(|t| t.root.is_some());
            if !parked.tabs.iter().any(|t| Some(&t.id) == parked.active.as_ref()) {
                parked.active = parked.tabs.first().map(|t| t.id.clone());
            }
        }
        let orphans: Vec<Window> = self.parked.iter().filter(|p| p.dock.is_none() && !p.tabs.is_empty()).cloned().collect();
        self.parked.retain(|p| p.dock.is_some());
        for orphan in orphans {
            let w = self.make_window();
            let window = &mut self.windows[w];
            window.tabs = orphan.tabs;
            window.active = orphan.active;
            window.tab_count = window.tabs.len();
            window.focused = orphan.focused;
            window.settle_focus();
        }
    }

    pub(crate) fn shelve(&mut self, pane: &str, edge: Edge, backlog: bool) {
        let mut parked = Window::new(pane.into());
        parked.dock = Some(Dock { pane: pane.into(), edge, width: DOCK_WIDTH });
        parked.desk = true;
        parked.backlog = backlog;
        parked.parked_at = Some(now());
        self.parked.insert(0, parked);
    }

    pub(crate) fn task_panes(&mut self, task: &str) -> Result<Vec<String>, KmuxError> {
        let window = match self.dock_window(task) {
            Some(w) => {
                self.windows[w].desk = true;
                &self.windows[w]
            }
            None => self.parked.iter().find(|p| p.dock.as_ref().is_some_and(|d| d.pane == task)).ok_or_else(|| not_found(format!("no task \"{task}\"")))?,
        };
        Ok(window.dock.iter().map(|d| d.pane.clone()).chain(window.tabs.iter().flat_map(|t| pane_ids(t.root.as_ref()))).collect())
    }

    pub(crate) fn place_on_task(&mut self, pane: &str, from: &str) -> Option<Option<String>> {
        if let Some(w) = self.dock_window(from) {
            let t = self.make_tab(w, true);
            let window = &mut self.windows[w];
            window.tabs[t].root = Some(Node::Pane(pane.into()));
            window.tabs[t].last_focus = Some(pane.into());
            window.focused = Some(pane.into());
            self.key = Some(window.id.clone());
            return Some(Some(window.id.clone()));
        }
        let at = self.parked.iter().position(|p| p.dock.as_ref().is_some_and(|d| d.pane == from))?;
        self.counters.0 += 1;
        let parked = &mut self.parked[at];
        parked.tab_count += 1;
        let mut tab = Tab::new(format!("t{}", self.counters.0), format!("Tab {}", parked.tab_count));
        tab.root = Some(Node::Pane(pane.into()));
        tab.last_focus = Some(pane.into());
        parked.active = Some(tab.id.clone());
        parked.focused = Some(pane.into());
        parked.tabs.push(tab);
        Some(None)
    }

    pub(crate) fn plan(&mut self, pane: &str, window: Option<&str>) -> Result<String, KmuxError> {
        let reference = window.ok_or_else(|| bad_request("a planning agent needs its window"))?;
        let w = self.window_index(reference).ok_or_else(|| not_found(format!("no window \"{reference}\"")))?;
        let window = &mut self.windows[w];
        if let Some(planner) = &window.planner {
            return Err(KmuxError::new("planned", format!("{} has a planning agent already: {}", window.id, planner.pane)));
        }
        window.planner = Some(Dock { pane: pane.into(), edge: Edge::Left, width: DOCK_WIDTH });
        window.desk = true;
        window.settle_focus();
        Ok(window.id.clone())
    }

    /// A Desk with no task shows the Shelf, which holds no tabs.
    pub(crate) fn is_shelf(&self, w: usize) -> bool {
        let window = &self.windows[w];
        window.desk && window.dock.is_none()
    }

    /// A Desk left with tabs but no task (its agent closed or undocked) becomes a plain window,
    /// or, when it has a planning agent to show beside the Shelf, hands its tabs to a window of their own.
    pub(crate) fn clear_shelves(&mut self) {
        for w in 0..self.windows.len() {
            if !self.is_shelf(w) || self.windows[w].tabs.is_empty() {
                continue;
            }
            if self.windows[w].planner.is_none() {
                self.windows[w].desk = false;
                continue;
            }
            let tabs = std::mem::take(&mut self.windows[w].tabs);
            let (active, key) = (self.windows[w].active.take(), self.key.clone());
            let n = self.make_window();
            let window = &mut self.windows[n];
            window.tab_count = tabs.len();
            window.tabs = tabs;
            window.active = active;
            window.settle_focus();
            self.windows[w].settle_focus();
            self.key = key;
        }
    }

    fn is_desk(&self, w: usize) -> bool {
        let window = &self.windows[w];
        window.desk || window.dock.is_some()
    }

    fn park_window(&mut self, w: usize) -> Option<String> {
        let window = &mut self.windows[w];
        let dock = window.dock.take()?;
        window.desk = true;
        let mut parked = Window::new(dock.pane.clone());
        parked.dock = Some(dock);
        parked.tabs = std::mem::take(&mut window.tabs);
        parked.active = window.active.take();
        parked.focused = window.focused.take();
        parked.tab_count = parked.tabs.len();
        parked.desk = true;
        parked.parked_at = Some(now());
        let id = parked.id.clone();
        self.parked.insert(0, parked);
        self.windows[w].settle_focus();
        Some(id)
    }

    fn task_window(&self, reference: &str, panes: &Panes) -> Result<(String, Option<usize>), KmuxError> {
        let id = panes.need(Some(reference))?.id.clone();
        if self.shelf().any(|p| p.id == id) {
            return Ok((id, None));
        }
        let w = self.window_containing(&id).ok_or_else(|| not_found(format!("{reference} is not on a Desk or the Shelf")))?;
        let task = self.windows[w].dock.as_ref().map(|d| d.pane.clone()).ok_or_else(|| not_found(format!("{reference} is not in a task: its window has no docked pane")))?;
        Ok((task, Some(w)))
    }

    fn desk_for(&mut self, window: Option<&str>) -> Result<usize, KmuxError> {
        if let Some(reference) = window.filter(|r| *r != "new") {
            let w = self.window_index(reference).ok_or_else(|| not_found(format!("no window \"{reference}\"")))?;
            return if self.is_desk(w) { Ok(w) } else { Err(bad_request(format!("{reference} is not a Desk: it has no docked pane"))) };
        }
        if window.is_none() {
            if let Some(w) = self.key.clone().and_then(|k| self.window_index(&k)).filter(|w| self.is_desk(*w)) {
                return Ok(w);
            }
        }
        let w = self.make_window();
        self.windows[w].desk = true;
        Ok(w)
    }

    pub(crate) fn task(&mut self, args: &Value, panes: &Panes) -> Result<Map, KmuxError> {
        let string = |key: &str| args.get(key).and_then(Value::as_str);
        match string("action").unwrap_or("") {
            "list" => Ok(self.task_list(panes)),
            "park" => {
                let w = match string("task") {
                    Some(reference) => match self.task_window(reference, panes)? {
                        (task, None) => return Ok(reply([("task", task.into()), ("window", Value::Null), ("parked", Value::Bool(false))])),
                        (_, Some(w)) => w,
                    },
                    None => match string("window") {
                        Some(reference) => self.window_index(reference).ok_or_else(|| not_found(format!("no window \"{reference}\"")))?,
                        None => self.key.clone().and_then(|k| self.window_index(&k)).ok_or_else(|| not_found("no window to park".into()))?,
                    },
                };
                let window = self.windows[w].id.clone();
                let task = self.park_window(w);
                if args.get("close").and_then(Value::as_bool) == Some(true) {
                    self.windows.remove(w);
                    if self.key.as_deref() == Some(window.as_str()) {
                        self.key = self.windows.last().map(|w| w.id.clone());
                    }
                } else {
                    self.key = Some(window.clone());
                }
                match task {
                    Some(task) => Ok(reply([("task", task.into()), ("window", window.into()), ("parked", Value::Bool(true))])),
                    None if args.get("close").and_then(Value::as_bool) == Some(true) => Ok(reply([("task", Value::Null), ("window", window.into()), ("parked", Value::Bool(false))])),
                    None => Err(not_found(format!("{window} has no task to park"))),
                }
            }
            "pull" => {
                let reference = string("task").ok_or_else(|| bad_request("pull needs a task"))?;
                let (task, at) = self.task_window(reference, panes)?;
                if let Some(w) = at {
                    let window = self.windows[w].id.clone();
                    self.key = Some(window.clone());
                    return Ok(reply([("task", task.into()), ("window", window.into()), ("moved", Value::Bool(false))]));
                }
                let w = self.desk_for(string("window"))?;
                let window = self.windows[w].id.clone();
                self.park_window(w);
                let i = self.parked.iter().position(|p| p.id == task).ok_or_else(|| not_found(format!("no task \"{task}\"")))?;
                let parked = self.parked.remove(i);
                let target = &mut self.windows[w];
                target.dock = parked.dock;
                target.tabs = parked.tabs;
                target.active = parked.active;
                target.focused = parked.focused;
                target.tab_count = target.tab_count.max(target.tabs.len());
                target.desk = true;
                target.settle_focus();
                self.key = Some(window.clone());
                Ok(reply([("task", task.into()), ("window", window.into()), ("moved", Value::Bool(true))]))
            }
            "hold" => {
                let reference = string("task").ok_or_else(|| bad_request("hold needs a task"))?;
                let (task, at) = self.task_window(reference, panes)?;
                if let Some(w) = at {
                    self.park_window(w);
                }
                let held = self.parked.iter_mut().find(|p| p.id == task).ok_or_else(|| not_found(format!("no task \"{task}\"")))?;
                held.backlog = true;
                Ok(reply([("task", task.into())]))
            }
            "release" => {
                let reference = string("task").ok_or_else(|| bad_request("release needs a task"))?;
                let i = self.parked.iter().position(|p| p.id == reference && p.backlog).ok_or_else(|| not_found(format!("no task \"{reference}\" held in the backlog")))?;
                let mut held = self.parked.remove(i);
                held.backlog = false;
                held.parked_at = Some(now());
                self.parked.insert(0, held);
                Ok(reply([("task", reference.into())]))
            }
            "desk" => {
                let project = match string("project") {
                    Some(p) if p.starts_with('/') => Some(p.to_string()),
                    None => None,
                    Some(_) => return Err(bad_request("desk needs a project: an absolute path to a folder")),
                };
                let w = self.make_window();
                let window = &mut self.windows[w];
                window.desk = true;
                window.project = project.clone();
                let id = window.id.clone();
                self.key = Some(id.clone());
                Ok(reply([("window", id.into()), ("project", project.map_or(Value::Null, Value::String))]))
            }
            other => Err(bad_request(format!("unknown task action \"{other}\" (list, park, pull, hold, release or desk)"))),
        }
    }

    pub fn task_list(&self, panes: &Panes) -> Map {
        let entry = |task: &str, window: Option<&str>, parked_at: Option<f64>, tabs: usize| {
            Value::Object(reply([
                ("id", task.into()),
                ("name", panes.get(task).and_then(|p| p.name.clone()).map_or(Value::Null, Value::String)),
                ("window", window.map_or(Value::Null, |w| w.into())),
                ("parkedAt", parked_at.map_or(Value::Null, num)),
                ("tabs", num(tabs as f64)),
            ]))
        };
        let mut tasks: Vec<Value> = self.windows.iter().filter_map(|w| w.dock.as_ref().map(|d| entry(&d.pane, Some(&w.id), None, w.tabs.len()))).collect();
        tasks.extend(self.shelf().map(|p| entry(&p.id, None, p.parked_at, p.tabs.len())));
        reply([("tasks", Value::Array(tasks))])
    }
}

fn without(node: Node, id: &str) -> Option<Node> {
    match node {
        Node::Pane(pane) if pane == id => None,
        Node::Pane(pane) => Some(Node::Pane(pane)),
        Node::Split(split) => {
            let kids: Vec<Child> = split.kids.into_iter().filter_map(|kid| without(kid.node, id).map(|node| Child { node, size: kid.size })).collect();
            let total: f64 = kids.iter().map(|k| k.size).sum();
            if kids.is_empty() {
                return None;
            }
            let kids = kids.into_iter().map(|k| Child { node: k.node, size: if total > 0.0 { k.size / total } else { 0.0 } }).collect();
            Some(normalize(Node::Split(Split { axis: split.axis, kids })))
        }
    }
}
