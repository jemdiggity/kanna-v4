use crate::error::KmuxError;
use crate::json::{num, Map, Value};
use crate::model::{bad_request, normalize, not_found, pane_ids, Child, Model, Node, Split, Window};
use crate::pane::Panes;
use std::time::{SystemTime, UNIX_EPOCH};

fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64().floor())
}

fn reply<const N: usize>(fields: [(&str, Value); N]) -> Map {
    fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

impl Model {
    pub fn shelf(&self) -> &[Window] {
        &self.shelf
    }

    pub fn parked_panes(&self) -> Vec<String> {
        self.shelf.iter().flat_map(|p| p.tabs.iter().flat_map(|t| pane_ids(t.root.as_ref())).chain(p.dock.iter().map(|d| d.pane.clone()))).collect()
    }

    pub(crate) fn detach_parked(&mut self, id: &str) -> bool {
        for parked in &mut self.shelf {
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
        for parked in &mut self.shelf {
            parked.tabs.retain(|t| t.root.is_some());
            if !parked.tabs.iter().any(|t| Some(&t.id) == parked.active.as_ref()) {
                parked.active = parked.tabs.first().map(|t| t.id.clone());
            }
        }
        let orphans: Vec<Window> = self.shelf.iter().filter(|p| p.dock.is_none() && !p.tabs.is_empty()).cloned().collect();
        self.shelf.retain(|p| p.dock.is_some());
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
        self.shelf.insert(0, parked);
        Some(id)
    }

    fn task_window(&self, reference: &str, panes: &Panes) -> Result<(String, Option<usize>), KmuxError> {
        let id = panes.need(Some(reference))?.id.clone();
        if self.shelf.iter().any(|p| p.id == id) {
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
                let i = self.shelf.iter().position(|p| p.id == task).ok_or_else(|| not_found(format!("no task \"{task}\"")))?;
                let parked = self.shelf.remove(i);
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
            other => Err(bad_request(format!("unknown task action \"{other}\" (list, park or pull)"))),
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
        tasks.extend(self.shelf.iter().map(|p| entry(&p.id, None, p.parked_at, p.tabs.len())));
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
