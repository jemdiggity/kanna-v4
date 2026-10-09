use crate::error::KmuxError;
use crate::json::{num, opt, Map, Value};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PaneType {
    Term,
    Web,
    Ios,
    Md,
}

impl PaneType {
    pub const ALL: [PaneType; 4] = [PaneType::Term, PaneType::Web, PaneType::Ios, PaneType::Md];

    pub fn as_str(self) -> &'static str {
        match self {
            PaneType::Term => "term",
            PaneType::Web => "web",
            PaneType::Ios => "ios",
            PaneType::Md => "md",
        }
    }

    pub fn parse(name: &str) -> Option<PaneType> {
        PaneType::ALL.into_iter().find(|t| t.as_str() == name)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneState {
    Starting,
    Running,
    Exited,
    Failed,
}

impl PaneState {
    pub fn as_str(self) -> &'static str {
        match self {
            PaneState::Starting => "starting",
            PaneState::Running => "running",
            PaneState::Exited => "exited",
            PaneState::Failed => "failed",
        }
    }

    pub fn parse(name: &str) -> Option<PaneState> {
        [PaneState::Starting, PaneState::Running, PaneState::Exited, PaneState::Failed].into_iter().find(|s| s.as_str() == name)
    }
}

/// Back/forward history: of URLs for web panes opened with `history: true`;
/// of files, always, for markdown panes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct History {
    pub back: Vec<String>,
    pub forward: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pane {
    pub id: String,
    pub name: Option<String>,
    /// The pane type (`type` in the protocol).
    pub kind: PaneType,
    pub state: PaneState,
    pub exit_code: Option<i64>,
    pub error: Option<String>,
    pub command: Option<String>,
    pub cwd: Option<String>,
    pub url: Option<String>,
    /// iOS panes: the app (a .app path or a bundle ID) and the simulator
    /// device it runs on (a name or UDID; the host fills in the one it chose).
    pub app: Option<String>,
    pub device: Option<String>,
    /// The markdown file a `md` pane shows.
    pub path: Option<String>,
    pub history: Option<History>,
    /// Markdown panes: the magnification, kept across restarts (the host
    /// fills it in before the layout is saved).
    pub zoom: Option<f64>,
    /// Terminals: reserved for re-attaching a hosted terminal session. Saved
    /// and restored, unused for now.
    pub session: Option<String>,
}

impl Pane {
    pub fn new(id: &str, name: Option<String>, kind: PaneType) -> Pane {
        Pane {
            id: id.into(),
            name,
            kind,
            state: PaneState::Starting,
            exit_code: None,
            error: None,
            command: None,
            cwd: None,
            url: None,
            app: None,
            device: None,
            path: None,
            history: None,
            zoom: None,
            session: None,
        }
    }

    pub fn label(&self) -> String {
        match &self.name {
            Some(name) => format!("{name} ({})", self.id),
            None => self.id.clone(),
        }
    }

    /// The pane as `list` and replies show it.
    pub fn summary(&self) -> Value {
        let mut out = Map::new();
        out.insert("id".into(), Value::String(self.id.clone()));
        out.insert("name".into(), opt(self.name.as_deref()));
        out.insert("type".into(), self.kind.as_str().into());
        out.insert("state".into(), self.state.as_str().into());
        match self.kind {
            PaneType::Term => {
                out.insert("cmd".into(), opt(self.command.as_deref()));
                out.insert("cwd".into(), opt(self.cwd.as_deref()));
            }
            PaneType::Web => {
                out.insert("url".into(), opt(self.url.as_deref()));
                out.insert("history".into(), Value::Bool(self.history.is_some()));
            }
            PaneType::Ios => {
                out.insert("app".into(), opt(self.app.as_deref()));
                out.insert("device".into(), opt(self.device.as_deref()));
            }
            PaneType::Md => {
                out.insert("path".into(), opt(self.path.as_deref()));
            }
        }
        if let Some(code) = self.exit_code {
            out.insert("exitCode".into(), num(code as f64));
        }
        if let Some(error) = &self.error {
            out.insert("error".into(), Value::String(error.clone()));
        }
        Value::Object(out)
    }
}

/// Every pane, in the order they were opened. IDs (`p1`, `p2`, …) are never
/// reused while kmux runs.
#[derive(Debug, Default)]
pub struct Panes {
    panes: HashMap<String, Pane>,
    order: Vec<String>,
    counter: usize,
}

impl Panes {
    pub fn get(&self, id: &str) -> Option<&Pane> {
        self.panes.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Pane> {
        self.panes.get_mut(id)
    }

    /// A pane by ID or name.
    pub fn pane(&self, reference: &str) -> Option<&Pane> {
        self.panes.get(reference).or_else(|| self.iter().find(|p| p.name.as_deref() == Some(reference)))
    }

    pub fn need(&self, reference: Option<&str>) -> Result<&Pane, KmuxError> {
        let reference = reference.ok_or_else(|| KmuxError::new("bad_request", "missing pane"))?;
        self.pane(reference).ok_or_else(|| KmuxError::new("not_found", format!("no pane \"{reference}\"")))
    }

    /// The name a layout shows for a pane: its name, else its ID.
    pub fn display(&self, id: &str) -> String {
        self.panes.get(id).and_then(|p| p.name.clone()).unwrap_or_else(|| id.into())
    }

    /// In the order they were opened.
    pub fn iter(&self) -> impl Iterator<Item = &Pane> {
        self.order.iter().filter_map(|id| self.panes.get(id))
    }

    pub fn ids(&self) -> Vec<String> {
        self.order.clone()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// The ID the next pane will get.
    pub fn next_id(&self) -> String {
        format!("p{}", self.counter + 1)
    }

    pub fn make(&mut self, kind: PaneType, name: Option<String>) -> &mut Pane {
        self.counter += 1;
        let pane = Pane::new(&format!("p{}", self.counter), name, kind);
        let id = pane.id.clone();
        self.order.push(id.clone());
        self.panes.entry(id).or_insert(pane)
    }

    pub fn forget(&mut self, id: &str) -> Option<Pane> {
        self.order.retain(|p| p != id);
        self.panes.remove(id)
    }

    pub fn counter(&self) -> usize {
        self.counter
    }

    /// Replaces every pane with restored ones. The counter only moves forward.
    pub fn install(&mut self, panes: Vec<Pane>, counter: usize) {
        self.order = panes.iter().map(|p| p.id.clone()).collect();
        self.panes = panes.into_iter().map(|p| (p.id.clone(), p)).collect();
        self.counter = self.counter.max(counter);
    }
}
