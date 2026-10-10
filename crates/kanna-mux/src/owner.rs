use crate::error::KmuxError;
use crate::json::{encode, json, num, string, Map, Value};
use crate::layout::{read_pane_table, serve, LayoutHost, Placement, SplitSide};
use crate::model::{Frame, Model, Node};
use crate::pane::{Pane, PaneType, Panes};
use crate::state::{self, number_in, STATE_VERSION};
use crate::state_file::{Contents, StateFile};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

pub struct LayoutOwner {
    model: Model,
    wide: Arc<Mutex<HashMap<String, bool>>>,
    panes: Panes,
    saved: Option<Value>,
    file: Option<StateFile>,
    last_saved: Option<String>,
}

impl Default for LayoutOwner {
    fn default() -> LayoutOwner {
        let wide: Arc<Mutex<HashMap<String, bool>>> = Arc::default();
        let mut model = Model::new();
        let shapes = wide.clone();
        model.pane_is_wide = Box::new(move |pane| shapes.lock().map_or(true, |w| w.get(pane).copied().unwrap_or(true)));
        LayoutOwner { model, wide, panes: Panes::default(), saved: None, file: None, last_saved: None }
    }
}

impl LayoutOwner {
    pub fn new() -> LayoutOwner {
        LayoutOwner::default()
    }

    pub fn open(file: Option<StateFile>, fresh: bool) -> (LayoutOwner, Option<String>) {
        let mut owner = LayoutOwner { file, ..LayoutOwner::default() };
        let Some(file) = owner.file.clone().filter(|_| !fresh) else { return (owner, None) };
        let problem = match file.load() {
            Contents::None => None,
            Contents::Refused(why) => Some(why),
            Contents::State(state) => match state::import_panes(&state) {
                Ok(_) => {
                    owner.saved = Some(state);
                    None
                }
                Err(error) => Some(error.message),
            },
        };
        let problem = problem.map(|why| format!("not restoring the saved layout: {why}; moved to {}", file.set_aside().unwrap_or_else(|| "nowhere".into())));
        (owner, problem)
    }

    pub fn handle(&mut self, request: &Value) -> Value {
        let result = match string(request, "call").unwrap_or("") {
            "serve" => self.serve(request.get("request").unwrap_or(&Value::Null)),
            "attach" => self.attach(request.get("panes").unwrap_or(&Value::Null)),
            "view" => Ok(self.view()),
            "hello" => Ok([("panes".to_string(), self.hello())].into_iter().collect()),
            "pane" => self.record(request.get("pane").unwrap_or(&Value::Null)),
            "export" => Ok(match self.export() {
                Value::Object(state) => state,
                _ => Map::new(),
            }),
            "focused" => Ok(changed(self.model.set_focused(string(request, "pane").unwrap_or("")))),
            "divider" => self.divider(request),
            "resolve" => self.panes.need(string(request, "pane")).map(|pane| [("id".to_string(), Value::String(pane.id.clone()))].into_iter().collect()),
            "frame" => self.frame(request),
            "shapes" => self.shapes(request),
            "save" => self.save().map(|saved| [("saved".to_string(), Value::Bool(saved))].into_iter().collect()),
            other => Err(KmuxError::new("bad_request", format!("unknown call \"{other}\""))),
        };
        match result {
            Ok(mut reply) => {
                reply.insert("ok".into(), Value::Bool(true));
                Value::Object(reply)
            }
            Err(error) => json!({ "ok": false, "error": { "code": error.code, "message": error.message } }),
        }
    }

    pub fn serve(&mut self, request: &Value) -> Result<Map, KmuxError> {
        if let Some(table) = request.get("panes") {
            let panes = read_pane_table(table)?;
            self.panes.install(panes, 0);
        }
        if string(request, "op") == Some("place") {
            if let [pane] = read_pane_table(&Value::Array(vec![request.get("pane").cloned().unwrap_or(Value::Null)]))?.as_slice() {
                self.learn(pane.clone());
            }
        }
        serve(&mut self.model, request, &self.panes)
    }

    pub fn hello(&self) -> Value {
        let panes = match &self.saved {
            Some(saved) => state::import_panes(saved).map(|(panes, _)| panes).unwrap_or_default(),
            None => {
                let placed: HashSet<String> = self.placed().into_iter().collect();
                self.panes.iter().filter(|p| placed.contains(&p.id)).cloned().collect()
            }
        };
        Value::Array(panes.iter().filter(|p| p.kind != PaneType::Term).map(|p| Value::Object(state::pane_record(p))).collect())
    }

    fn divider(&mut self, request: &Value) -> Result<Map, KmuxError> {
        let tab = string(request, "tab").ok_or_else(|| KmuxError::new("bad_request", "divider needs a tab"))?;
        let path: Vec<usize> = request.get("path").and_then(Value::as_array).map_or(vec![], |p| p.iter().filter_map(Value::as_u64).map(|i| i as usize).collect());
        let index = request.get("index").and_then(Value::as_u64).ok_or_else(|| KmuxError::new("bad_request", "divider needs an index"))? as usize;
        let share = request.get("share").and_then(Value::as_f64).ok_or_else(|| KmuxError::new("bad_request", "divider needs a share"))?;
        self.model.set_divider(tab, &path, index, share)?;
        Ok(Map::new())
    }

    fn frame(&mut self, request: &Value) -> Result<Map, KmuxError> {
        let window = string(request, "window").unwrap_or("");
        let frame = request.get("frame").unwrap_or(&Value::Null);
        let field = |name: &str| frame.get(name).and_then(Value::as_f64).ok_or_else(|| KmuxError::new("bad_request", "a frame has x, y, w and h"));
        let frame = Frame { x: field("x")?, y: field("y")?, w: field("w")?, h: field("h")? };
        Ok(changed(self.model.set_frame(window, frame)))
    }

    fn shapes(&mut self, request: &Value) -> Result<Map, KmuxError> {
        let shapes: HashMap<String, bool> = request.get("panes").and_then(Value::as_object).map_or_else(HashMap::new, |panes| {
            panes.iter().filter_map(|(id, wide)| wide.as_bool().map(|wide| (id.clone(), wide))).collect()
        });
        if let Ok(mut wide) = self.wide.lock() {
            *wide = shapes;
        }
        Ok(Map::new())
    }

    fn record(&mut self, record: &Value) -> Result<Map, KmuxError> {
        let [pane] = read_pane_table(&Value::Array(vec![record.clone()]))?.try_into().map_err(|_| KmuxError::new("bad_request", "one pane"))?;
        if let Some(known) = self.panes.get_mut(&pane.id) {
            *known = pane;
        }
        Ok(Map::new())
    }

    fn learn(&mut self, pane: Pane) {
        let mut panes: Vec<Pane> = self.panes.iter().filter(|p| p.id != pane.id).cloned().collect();
        panes.push(pane);
        self.panes.install(panes, 0);
    }

    pub fn attach(&mut self, live: &Value) -> Result<Map, KmuxError> {
        let live = read_pane_table(live)?;
        let ids: HashSet<String> = live.iter().map(|p| p.id.clone()).collect();
        self.panes.install(live.clone(), 0);
        let mut problem = None;
        if let Some(saved) = self.saved.take() {
            let pruned = prune_state(&saved, &ids);
            match self.model.read_state(&pruned, &ids) {
                Ok(layout) => self.model.install_state(layout),
                Err(error) => {
                    let aside = self.file.as_ref().and_then(StateFile::set_aside).unwrap_or_else(|| "nowhere".into());
                    problem = Some(format!("not restoring the saved layout: {}; moved to {aside}", error.message));
                }
            }
        }
        let mut dropped = vec![];
        for pane in self.placed() {
            if !ids.contains(&pane) {
                self.model.remove(&pane);
                dropped.push(pane);
            }
        }
        self.model.tidy();
        let placed: HashSet<String> = self.placed().into_iter().collect();
        let mut added = vec![];
        for pane in live.iter().filter(|p| !placed.contains(&p.id)) {
            self.model.place(&pane.id, &Placement { window: None, split: SplitSide::Auto, size: 0.5, new_tab: false, dock: None })?;
            added.push(pane.id.clone());
        }
        let mut reply = Map::new();
        reply.insert("dropped".into(), dropped.into());
        reply.insert("placed".into(), added.into());
        reply.insert("problem".into(), problem.map_or(Value::Null, Value::String));
        Ok(reply)
    }

    fn placed(&self) -> Vec<String> {
        self.model
            .windows()
            .iter()
            .flat_map(|w| w.tabs.iter().flat_map(|t| crate::model::pane_ids(t.root.as_ref())).chain(w.dock.iter().map(|d| d.pane.clone())))
            .collect()
    }

    pub fn view(&self) -> Map {
        let windows: Vec<Value> = self
            .model
            .windows()
            .iter()
            .map(|window| {
                let tab = window.active_tab();
                let dock = window.dock.as_ref();
                let docked = |pane: &String| dock.is_some_and(|d| &d.pane == pane);
                let zoomed = window.zoomed.clone().filter(|z| docked(z) || tab.is_some_and(|t| crate::model::pane_ids(t.root.as_ref()).contains(z)));
                let root = match (&zoomed, tab) {
                    (Some(pane), _) if docked(pane) => Value::Null,
                    (Some(pane), _) => self.node(&Node::Pane(pane.clone())),
                    (None, Some(tab)) => tab.root.as_ref().map_or(Value::Null, |root| self.node(root)),
                    (None, None) => Value::Null,
                };
                let dock = dock.map_or(Value::Null, |d| {
                    let mut node = self.node(&Node::Pane(d.pane.clone()));
                    node["edge"] = d.edge.as_str().into();
                    node["width"] = num(d.width);
                    node
                });
                let title = tab.map(|t| t.title.clone()).or_else(|| window.dock.as_ref().map(|d| self.panes.display(&d.pane)));
                let frame = window.frame.map_or(Value::Null, |f| json!({ "x": f.x, "y": f.y, "w": f.w, "h": f.h }));
                let tabs: Vec<Value> = window.tabs.iter().map(|t| json!({ "id": t.id, "title": t.title, "panes": crate::model::pane_ids(t.root.as_ref()) })).collect();
                json!({
                    "id": window.id,
                    "frame": frame,
                    "tabs": tabs,
                    "tab": tab.map_or(Value::Null, |t| t.id.clone().into()),
                    "title": title.map_or(Value::Null, Value::String),
                    "focused": window.focused.clone().map_or(Value::Null, Value::String),
                    "zoomed": zoomed.map_or(Value::Null, Value::String),
                    "root": root,
                    "dock": dock,
                })
            })
            .collect();
        let mut view = Map::new();
        view.insert("key".into(), self.model.key_window().map_or(Value::Null, |w| w.id.clone().into()));
        view.insert("windows".into(), Value::Array(windows));
        view
    }

    fn node(&self, node: &Node) -> Value {
        match node {
            Node::Pane(id) => json!({ "pane": id, "type": self.panes.get(id).map_or("term", |p| p.kind.as_str()) }),
            Node::Split(split) => {
                let children: Vec<Value> = split
                    .kids
                    .iter()
                    .map(|kid| {
                        let mut child = self.node(&kid.node);
                        child["size"] = num(kid.size);
                        child
                    })
                    .collect();
                json!({ "split": split.axis.as_str(), "children": children })
            }
        }
    }

    pub fn export(&self) -> Value {
        let mut out = self.model.export_state();
        let mut counters = match out.remove("counters") {
            Some(Value::Object(counters)) => counters,
            _ => Map::new(),
        };
        let placed: HashSet<String> = self.placed().into_iter().collect();
        let mut panes = Panes::default();
        panes.install(self.panes.iter().filter(|p| placed.contains(&p.id)).cloned().collect(), 0);
        let counter = panes.iter().filter_map(|p| number_in(&p.id, "p")).max().unwrap_or(0);
        counters.insert("pane".into(), num(counter as f64));
        out.insert("kmux".into(), "state".into());
        out.insert("version".into(), num(STATE_VERSION as f64));
        out.insert("counters".into(), Value::Object(counters));
        out.insert("panes".into(), state::export_panes(&panes));
        Value::Object(out)
    }

    pub fn save(&mut self) -> Result<bool, KmuxError> {
        let Some(file) = &self.file else { return Ok(false) };
        if self.saved.is_some() {
            return Ok(false);
        }
        let state = self.export();
        let encoded = encode(&state);
        if self.last_saved.as_ref() == Some(&encoded) {
            return Ok(false);
        }
        file.write(&state)?;
        self.last_saved = Some(encoded);
        Ok(true)
    }
}

fn changed(changed: bool) -> Map {
    [("changed".to_string(), Value::Bool(changed))].into_iter().collect()
}

pub fn prune_state(state: &Value, live: &HashSet<String>) -> Value {
    let mut state = state.clone();
    if let Some(windows) = state.get_mut("windows").and_then(Value::as_array_mut) {
        for window in windows.iter_mut() {
            let gone = window.get("dock").and_then(|d| d.get("pane")).and_then(Value::as_str).is_some_and(|pane| !live.contains(pane));
            if gone {
                window["dock"] = Value::Null;
            }
            if let Some(tabs) = window.get_mut("tabs").and_then(Value::as_array_mut) {
                for tab in tabs.iter_mut() {
                    if let Some(layout) = tab.get_mut("layout") {
                        *layout = prune(layout, live).unwrap_or(Value::Null);
                    }
                }
            }
        }
    }
    state
}

fn prune(node: &Value, live: &HashSet<String>) -> Option<Value> {
    if node.is_null() {
        return None;
    }
    if let Some(pane) = node.get("pane").filter(|p| !p.is_null()) {
        return pane.as_str().filter(|id| live.contains(*id)).map(|_| node.clone());
    }
    let Some(children) = node.get("children").and_then(Value::as_array) else { return Some(node.clone()) };
    let kept: Vec<Value> = children.iter().filter_map(|child| prune(child, live)).collect();
    if kept.is_empty() {
        return None;
    }
    let mut node = node.clone();
    node["children"] = Value::Array(kept);
    Some(node)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::json;

    fn table(ids: &[&str]) -> Value {
        Value::Array(ids.iter().map(|id| json!({ "id": id, "name": null, "type": "term" })).collect())
    }

    fn folder(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("kmo-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::os::unix::fs::PermissionsExt::from_mode(0o700)).unwrap();
        dir
    }

    fn place(owner: &mut LayoutOwner, id: &str, at: Value) {
        let reply = owner.handle(&json!({ "call": "serve", "request": { "op": "place", "pane": { "id": id, "name": null, "type": "term" }, "at": at } }));
        assert_eq!(reply["ok"], true, "{reply}");
    }

    #[test]
    fn a_saved_layout_comes_back_without_the_panes_that_are_gone() {
        let dir = folder("back");
        let file = StateFile::new(&dir.join("layout.json").to_string_lossy());
        let (mut owner, problem) = LayoutOwner::open(Some(file.clone()), false);
        assert_eq!(problem, None);
        owner.handle(&json!({ "call": "attach", "panes": [] }));
        place(&mut owner, "p1", json!({}));
        place(&mut owner, "p2", json!({ "split": "right", "size": 0.25 }));
        place(&mut owner, "p3", json!({ "split": "down" }));
        assert_eq!(owner.handle(&json!({ "call": "save" }))["saved"], true);
        assert_eq!(owner.handle(&json!({ "call": "save" }))["saved"], false);
        let mode = std::fs::metadata(&file.path).map(|m| std::os::unix::fs::PermissionsExt::mode(&m.permissions()) & 0o777).unwrap();
        assert_eq!(mode, 0o600);

        let (mut again, problem) = LayoutOwner::open(Some(file), false);
        assert_eq!(problem, None);
        let reply = again.handle(&json!({ "call": "attach", "panes": table(&["p1", "p3", "p4"]) }));
        assert_eq!(reply["placed"], json!(["p4"]), "{reply}");
        let view = again.view();
        assert_eq!(
            Value::Object(view),
            json!({ "key": "w1", "windows": [{ "id": "w1", "frame": null, "tabs": [{ "id": "t1", "title": "Tab 1", "panes": ["p1", "p3", "p4"] }], "tab": "t1", "title": "Tab 1",
                "focused": "p4", "zoomed": null, "root": { "split": "row", "children": [
                { "pane": "p1", "type": "term", "size": 0.75 },
                { "pane": "p3", "type": "term", "size": 0.125 },
                { "pane": "p4", "type": "term", "size": 0.125 }
            ] }, "dock": null }] })
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_records_of_web_markdown_and_ios_panes_are_saved_and_handed_back_in_the_hello() {
        let dir = folder("records");
        let file = StateFile::new(&dir.join("layout.json").to_string_lossy());
        let (mut owner, _) = LayoutOwner::open(Some(file.clone()), false);
        owner.handle(&json!({ "call": "attach", "panes": [] }));
        let site = json!({ "id": "p1", "name": "site", "type": "web", "url": "http://a.test/", "history": true });
        owner.handle(&json!({ "call": "serve", "request": { "op": "place", "pane": site, "at": {} } }));
        owner.handle(&json!({ "call": "serve", "request": { "op": "place", "pane": { "id": "p2", "name": null, "type": "term" }, "at": { "split": "right" } } }));
        owner.handle(&json!({ "call": "pane", "pane": { "id": "p1", "name": "site", "type": "web", "url": "http://b.test/", "history": true } }));
        owner.handle(&json!({ "call": "pane", "pane": { "id": "p9", "name": "stray", "type": "md", "path": "/x.md" } }));
        assert_eq!(owner.handle(&json!({ "call": "hello" }))["panes"], json!([{ "id": "p1", "name": "site", "type": "web", "url": "http://b.test/", "history": true }]));
        owner.save().unwrap();

        let (again, _) = LayoutOwner::open(Some(file), false);
        assert_eq!(again.hello(), json!([{ "id": "p1", "name": "site", "type": "web", "url": "http://b.test/", "history": true }]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_app_moves_dividers_records_focus_and_frames_and_chooses_auto_splits_by_shape() {
        let mut owner = LayoutOwner::new();
        owner.handle(&json!({ "call": "attach", "panes": [] }));
        place(&mut owner, "p1", json!({}));
        owner.handle(&json!({ "call": "shapes", "panes": { "p1": false } }));
        place(&mut owner, "p2", json!({}));
        owner.handle(&json!({ "call": "shapes", "panes": { "p1": true, "p2": true } }));
        place(&mut owner, "p3", json!({}));
        let root = |owner: &LayoutOwner| owner.view()["windows"][0]["root"].clone();
        assert_eq!(root(&owner)["split"], "column");
        assert_eq!(root(&owner)["children"][1]["split"], "row");
        let reply = owner.handle(&json!({ "call": "divider", "tab": "t1", "path": [], "index": 0, "share": 0.25 }));
        assert_eq!(reply["ok"], true, "{reply}");
        assert_eq!(root(&owner)["children"][0]["size"], 0.25);
        assert_eq!(root(&owner)["children"][1]["size"], 0.75);
        assert_eq!(owner.handle(&json!({ "call": "divider", "tab": "t1", "path": [1], "index": 0, "share": 2.0 }))["ok"], false);
        assert_eq!(owner.handle(&json!({ "call": "divider", "tab": "t1", "path": [0], "index": 0, "share": 0.5 }))["ok"], false);
        assert_eq!(owner.handle(&json!({ "call": "focused", "pane": "p1" }))["changed"], true);
        assert_eq!(owner.handle(&json!({ "call": "focused", "pane": "p1" }))["changed"], false);
        assert_eq!(owner.view()["windows"][0]["focused"], "p1");
        assert_eq!(owner.handle(&json!({ "call": "frame", "window": "w1", "frame": { "x": 10, "y": 20, "w": 800, "h": 600 } }))["changed"], true);
        assert_eq!(owner.view()["windows"][0]["frame"], json!({ "x": 10.0, "y": 20.0, "w": 800.0, "h": 600.0 }));
        assert_eq!(owner.export()["windows"][0]["frame"], json!({ "x": 10, "y": 20, "w": 800, "h": 600 }));
    }

    #[test]
    fn a_docked_pane_is_drawn_beside_the_tabs_saved_and_dropped_when_its_pane_is_gone() {
        let dir = folder("dock");
        let file = StateFile::new(&dir.join("layout.json").to_string_lossy());
        let (mut owner, _) = LayoutOwner::open(Some(file.clone()), false);
        owner.handle(&json!({ "call": "attach", "panes": [] }));
        place(&mut owner, "p1", json!({}));
        place(&mut owner, "p2", json!({ "dock": "right" }));
        let window = |owner: &LayoutOwner| owner.view()["windows"][0].clone();
        assert_eq!(window(&owner)["dock"], json!({ "pane": "p2", "type": "term", "edge": "right", "width": 0.4 }));
        assert_eq!(window(&owner)["root"], json!({ "pane": "p1", "type": "term" }));
        assert_eq!(window(&owner)["focused"], "p2");
        assert_eq!(owner.handle(&json!({ "call": "focused", "pane": "p1" }))["changed"], true);
        assert_eq!(owner.handle(&json!({ "call": "focused", "pane": "p2" }))["changed"], true);
        let zoom = owner.handle(&json!({ "call": "serve", "request": { "op": "command", "cmd": "zoom", "args": { "pane": "p2" } } }));
        assert_eq!(zoom["zoomed"], true, "{zoom}");
        assert_eq!(window(&owner)["root"], Value::Null);
        assert_eq!(window(&owner)["zoomed"], "p2");
        assert_eq!(owner.handle(&json!({ "call": "attach", "panes": table(&["p1", "p2"]) }))["placed"], json!([]));
        owner.save().unwrap();
        assert_eq!(owner.export()["windows"][0]["dock"], json!({ "pane": "p2", "edge": "right", "width": 0.4 }));

        let (mut again, _) = LayoutOwner::open(Some(file.clone()), false);
        again.handle(&json!({ "call": "attach", "panes": table(&["p1", "p2"]) }));
        assert_eq!(window(&again)["dock"]["pane"], "p2");
        assert_eq!(window(&again)["zoomed"], "p2");

        let (mut gone, problem) = LayoutOwner::open(Some(file), false);
        let reply = gone.handle(&json!({ "call": "attach", "panes": table(&["p1"]) }));
        assert_eq!(problem, None);
        assert_eq!(reply["problem"], Value::Null, "{reply}");
        assert_eq!(window(&gone)["dock"], Value::Null);
        assert_eq!(window(&gone)["focused"], "p1");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_window_with_only_a_docked_pane_is_titled_after_it() {
        let mut owner = LayoutOwner::new();
        owner.handle(&json!({ "call": "attach", "panes": [] }));
        let reply = owner.handle(&json!({ "call": "serve", "request": { "op": "place", "pane": { "id": "p1", "name": "agent", "type": "term" }, "at": { "dock": "left" } } }));
        assert_eq!(reply, json!({ "ok": true, "window": "w1", "tab": null, "dock": "left" }));
        let view = owner.view();
        assert_eq!(view["windows"][0]["title"], "agent");
        assert_eq!(view["windows"][0]["tabs"], json!([]));
        assert_eq!(view["windows"][0]["root"], Value::Null);
        assert_eq!(owner.handle(&json!({ "call": "serve", "request": { "op": "place", "pane": { "id": "p2", "name": null, "type": "term" }, "at": { "dock": "top" } } }))["ok"], false);
    }

    #[test]
    fn a_file_it_cannot_trust_is_set_aside() {
        let dir = folder("bad");
        let path = dir.join("layout.json");
        std::fs::write(&path, "{\"kmux\":\"state\",\"version\":99,\"panes\":[],\"windows\":[]}").unwrap();
        std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o600)).unwrap();
        let (_, problem) = LayoutOwner::open(Some(StateFile::new(&path.to_string_lossy())), false);
        assert!(problem.as_deref().is_some_and(|p| p.contains("unsupported state version")), "{problem:?}");
        assert!(dir.join("layout.json.bad").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nothing_is_saved_before_the_saved_layout_is_installed() {
        let dir = folder("early");
        let file = StateFile::new(&dir.join("layout.json").to_string_lossy());
        let (mut owner, _) = LayoutOwner::open(Some(file.clone()), false);
        owner.handle(&json!({ "call": "attach", "panes": table(&["p1"]) }));
        owner.save().unwrap();
        let before = std::fs::read_to_string(&file.path).unwrap();
        let (mut early, _) = LayoutOwner::open(Some(file.clone()), false);
        assert!(!early.save().unwrap());
        assert_eq!(std::fs::read_to_string(&file.path).unwrap(), before);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pruning_drops_empty_splits() {
        let live: HashSet<String> = ["p2".to_string()].into_iter().collect();
        let node = json!({ "split": "row", "children": [{ "pane": "p1" }, { "split": "column", "children": [{ "pane": "p2" }, { "pane": "p3" }] }] });
        assert_eq!(prune(&node, &live), Some(json!({ "split": "row", "children": [{ "split": "column", "children": [{ "pane": "p2" }] }] })));
        assert_eq!(prune(&json!({ "pane": "p9" }), &live), None);
    }
}
