use crate::json::{json, Value};
use crate::model::Dock;
use crate::tabs::Tabs;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Where {
    Desk(String),
    Shelf,
    Backlog,
}

impl Where {
    pub fn to_json(&self) -> Value {
        match self {
            Where::Desk(window) => json!({ "desk": window }),
            Where::Shelf => "shelf".into(),
            Where::Backlog => "backlog".into(),
        }
    }

    pub fn read(value: Option<&Value>) -> Option<Where> {
        match value? {
            Value::String(place) if place == "shelf" => Some(Where::Shelf),
            Value::String(place) if place == "backlog" => Some(Where::Backlog),
            Value::Object(place) => place.get("desk").and_then(Value::as_str).map(|w| Where::Desk(w.into())),
            _ => None,
        }
    }

    pub fn desk(&self) -> Option<&str> {
        match self {
            Where::Desk(window) => Some(window),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Workspace {
    pub id: String,
    pub task: Option<String>,
    pub dock: Dock,
    pub tabs: Tabs,
    pub place: Where,
    pub parked_at: Option<f64>,
}

impl Workspace {
    pub(crate) fn new(dock: Dock, place: Where) -> Workspace {
        Workspace { id: dock.pane.clone(), task: None, dock, tabs: Tabs::default(), place, parked_at: None }
    }

    pub fn shown(&self) -> bool {
        matches!(self.place, Where::Desk(_))
    }

    pub fn summary(&self) -> Value {
        json!({ "id": self.id, "task": self.task, "where": self.place.to_json(), "hidden": !matches!(self.place, Where::Desk(_)) })
    }
}
