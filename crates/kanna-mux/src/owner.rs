use crate::error::KmuxError;
use crate::json::{encode, json, num, string, Map, Value};
use crate::layout::{read_pane_table, serve, LayoutHost, Placement, SplitSide};
use crate::model::{Model, Node};
use crate::pane::{Pane, Panes};
use crate::state::{self, number_in, STATE_VERSION};
use crate::state_file::{Contents, StateFile};
use std::collections::HashSet;

pub struct LayoutOwner {
    model: Model,
    panes: Panes,
    saved: Option<Value>,
    file: Option<StateFile>,
    last_saved: Option<String>,
}

impl Default for LayoutOwner {
    fn default() -> LayoutOwner {
        LayoutOwner { model: Model::new(), panes: Panes::default(), saved: None, file: None, last_saved: None }
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
            "export" => Ok(match self.export() {
                Value::Object(state) => state,
                _ => Map::new(),
            }),
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
            self.model.place(&pane.id, &Placement { window: None, split: SplitSide::Auto, size: 0.5, new_tab: false })?;
            added.push(pane.id.clone());
        }
        let mut reply = Map::new();
        reply.insert("dropped".into(), dropped.into());
        reply.insert("placed".into(), added.into());
        reply.insert("problem".into(), problem.map_or(Value::Null, Value::String));
        Ok(reply)
    }

    fn placed(&self) -> Vec<String> {
        self.model.windows().iter().flat_map(|w| w.tabs.iter()).flat_map(|t| crate::model::pane_ids(t.root.as_ref())).collect()
    }

    pub fn view(&self) -> Map {
        let mut view = Map::new();
        let window = self.model.key_window().or_else(|| self.model.windows().first());
        let tab = window.and_then(|w| w.active_tab());
        view.insert("window".into(), window.map_or(Value::Null, |w| w.id.clone().into()));
        view.insert("tab".into(), tab.map_or(Value::Null, |t| t.id.clone().into()));
        view.insert("focused".into(), window.and_then(|w| w.focused.clone()).map_or(Value::Null, Value::String));
        let zoomed = window.and_then(|w| w.zoomed.clone()).filter(|z| tab.is_some_and(|t| crate::model::pane_ids(t.root.as_ref()).contains(z)));
        let root = match (&zoomed, tab) {
            (Some(pane), _) => self.node(&Node::Pane(pane.clone())),
            (None, Some(tab)) => tab.root.as_ref().map_or(Value::Null, |root| self.node(root)),
            (None, None) => Value::Null,
        };
        view.insert("root".into(), root);
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

pub fn prune_state(state: &Value, live: &HashSet<String>) -> Value {
    let mut state = state.clone();
    if let Some(windows) = state.get_mut("windows").and_then(Value::as_array_mut) {
        for window in windows.iter_mut() {
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
            json!({ "window": "w1", "tab": "t1", "focused": "p4", "root": { "split": "row", "children": [
                { "pane": "p1", "type": "term", "size": 0.75 },
                { "pane": "p3", "type": "term", "size": 0.125 },
                { "pane": "p4", "type": "term", "size": 0.125 }
            ] } })
        );
        let _ = std::fs::remove_dir_all(&dir);
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
