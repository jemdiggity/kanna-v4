use crate::error::KmuxError;
use crate::json::{num, opt, Map, Value};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PaneType {
    Term,
    Web,
    Ios,
    Md,
    Code,
}

impl PaneType {
    pub const ALL: [PaneType; 5] = [PaneType::Term, PaneType::Web, PaneType::Ios, PaneType::Md, PaneType::Code];

    pub fn as_str(self) -> &'static str {
        match self {
            PaneType::Term => "term",
            PaneType::Web => "web",
            PaneType::Ios => "ios",
            PaneType::Md => "md",
            PaneType::Code => "code",
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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct History {
    pub back: Vec<String>,
    pub forward: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Comment {
    pub path: String,
    pub first: u64,
    pub last: u64,
    pub text: String,
}

impl Comment {
    pub fn to_json(&self) -> Value {
        let mut out = Map::new();
        out.insert("path".into(), Value::String(self.path.clone()));
        out.insert("first".into(), num(self.first as f64));
        out.insert("last".into(), num(self.last as f64));
        out.insert("text".into(), Value::String(self.text.clone()));
        Value::Object(out)
    }

    pub fn from_json(value: &Value) -> Option<Comment> {
        let line = |key: &str| value.get(key).and_then(Value::as_u64).filter(|l| *l > 0);
        let (first, last) = (line("first")?, line("last")?);
        Some(Comment {
            path: value.get("path")?.as_str()?.to_string(),
            first,
            last: last.max(first),
            text: value.get("text")?.as_str()?.to_string(),
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pane {
    pub id: String,
    pub name: Option<String>,
    pub kind: PaneType,
    pub state: PaneState,
    pub exit_code: Option<i64>,
    pub error: Option<String>,
    pub command: Option<String>,
    pub cwd: Option<String>,
    pub opened_in: Option<String>,
    pub url: Option<String>,
    pub title: Option<String>,
    pub app: Option<String>,
    pub device: Option<String>,
    pub path: Option<String>,
    pub line: Option<u64>,
    pub comment: Option<Comment>,
    pub agent: Option<String>,
    pub markup: bool,
    pub history: Option<History>,
    pub zoom: Option<f64>,
    pub session: Option<String>,
    pub pid: Option<i64>,
    pub status: Option<String>,
    pub hung_up: bool,
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
            opened_in: None,
            url: None,
            title: None,
            app: None,
            device: None,
            path: None,
            line: None,
            comment: None,
            agent: None,
            markup: false,
            history: None,
            zoom: None,
            session: None,
            pid: None,
            status: None,
            hung_up: false,
        }
    }

    pub fn status(&self) -> Option<&str> {
        match (self.kind, self.state) {
            (PaneType::Term, PaneState::Exited) => Some("exited"),
            (PaneType::Term, _) => self.status.as_deref(),
            _ => None,
        }
    }

    pub fn label(&self) -> String {
        match &self.name {
            Some(name) => format!("{name} ({})", self.id),
            None => self.id.clone(),
        }
    }

    pub fn tab_name(&self) -> Option<String> {
        if let Some(name) = &self.name {
            return Some(name.clone());
        }
        match self.kind {
            PaneType::Md | PaneType::Code => self.path.as_deref().and_then(file_name),
            PaneType::Web => self.title.clone().filter(|t| !t.trim().is_empty()).or_else(|| self.url.as_deref().and_then(host)),
            PaneType::Term | PaneType::Ios => None,
        }
    }

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
                if let Some(pid) = self.pid {
                    out.insert("pid".into(), num(pid as f64));
                }
            }
            PaneType::Web => {
                out.insert("url".into(), opt(self.url.as_deref()));
                out.insert("history".into(), Value::Bool(self.history.is_some()));
                if self.markup {
                    out.insert("markup".into(), Value::Bool(true));
                }
                if let Some(agent) = &self.agent {
                    out.insert("agent".into(), Value::String(agent.clone()));
                }
            }
            PaneType::Ios => {
                out.insert("app".into(), opt(self.app.as_deref()));
                out.insert("device".into(), opt(self.device.as_deref()));
            }
            PaneType::Md => {
                out.insert("path".into(), opt(self.path.as_deref()));
                out.insert("agent".into(), opt(self.agent.as_deref()));
                if self.markup {
                    out.insert("markup".into(), Value::Bool(true));
                }
            }
            PaneType::Code => {
                out.insert("path".into(), opt(self.path.as_deref()));
                if let Some(line) = self.line {
                    out.insert("line".into(), num(line as f64));
                }
                if let Some(comment) = &self.comment {
                    out.insert("comment".into(), comment.to_json());
                }
            }
        }
        if let Some(code) = self.exit_code {
            out.insert("exitCode".into(), num(code as f64));
        }
        if let Some(status) = self.status() {
            out.insert("status".into(), Value::String(status.into()));
        }
        if let Some(error) = &self.error {
            out.insert("error".into(), Value::String(error.clone()));
        }
        Value::Object(out)
    }
}

fn file_name(path: &str) -> Option<String> {
    path.trim_end_matches('/').rsplit('/').next().filter(|n| !n.is_empty()).map(String::from)
}

fn host(url: &str) -> Option<String> {
    let Some((_, rest)) = url.split_once("://") else { return file_name(url) };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit('@').next().unwrap_or("");
    let host = match authority.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or(""),
        None => authority.split(':').next().unwrap_or(""),
    };
    if host.is_empty() { file_name(rest.split(['?', '#']).next().unwrap_or("")) } else { Some(host.into()) }
}

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

    pub fn pane(&self, reference: &str) -> Option<&Pane> {
        self.panes.get(reference).or_else(|| self.iter().find(|p| p.name.as_deref() == Some(reference)))
    }

    pub fn need(&self, reference: Option<&str>) -> Result<&Pane, KmuxError> {
        let reference = reference.ok_or_else(|| KmuxError::new("bad_request", "missing pane"))?;
        self.pane(reference).ok_or_else(|| KmuxError::new("not_found", format!("no pane \"{reference}\"")))
    }

    pub fn display(&self, id: &str) -> String {
        self.panes.get(id).and_then(|p| p.name.clone()).unwrap_or_else(|| id.into())
    }

    pub fn iter(&self) -> impl Iterator<Item = &Pane> {
        self.order.iter().filter_map(|id| self.panes.get(id))
    }

    pub fn ids(&self) -> Vec<String> {
        self.order.clone()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

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

    pub fn install(&mut self, panes: Vec<Pane>, counter: usize) {
        self.order = panes.iter().map(|p| p.id.clone()).collect();
        self.panes = panes.into_iter().map(|p| (p.id.clone(), p)).collect();
        self.counter = self.counter.max(counter);
    }
}

#[cfg(test)]
mod status_tests {
    use super::*;

    #[test]
    fn agent_terminals_list_their_status_and_every_exited_terminal_says_so() {
        let mut pane = Pane::new("p1", None, PaneType::Term);
        pane.state = PaneState::Running;
        assert!(pane.summary().get("status").is_none());
        pane.status = Some("working".into());
        assert_eq!(pane.summary()["status"], "working");
        pane.state = PaneState::Exited;
        assert_eq!(pane.summary()["status"], "exited");
        pane.status = None;
        assert_eq!(pane.summary()["status"], "exited");
        let mut web = Pane::new("p2", None, PaneType::Web);
        web.state = PaneState::Exited;
        assert!(web.summary().get("status").is_none());
    }
}

#[cfg(test)]
mod pid_tests {
    use super::*;

    #[test]
    fn a_terminal_lists_its_pid_once_it_has_one() {
        let mut pane = Pane::new("p1", None, PaneType::Term);
        assert!(pane.summary().get("pid").is_none());
        pane.pid = Some(4242);
        assert_eq!(pane.summary().get("pid").and_then(|v| v.as_f64()), Some(4242.0));
    }
}

#[cfg(test)]
mod tab_name_tests {
    use super::*;

    fn pane(kind: PaneType, name: Option<&str>) -> Pane {
        Pane::new("p1", name.map(String::from), kind)
    }

    #[test]
    fn a_pane_names_its_tab_by_its_name_then_its_file_or_page() {
        let mut code = pane(PaneType::Code, None);
        code.path = Some("/repo/crates/kanna-daemon/src/store.rs".into());
        assert_eq!(code.tab_name().as_deref(), Some("store.rs"));
        let mut md = pane(PaneType::Md, None);
        md.path = Some("docs/specs/presenting.md".into());
        assert_eq!(md.tab_name().as_deref(), Some("presenting.md"));
        let mut named = pane(PaneType::Code, Some("diff"));
        named.path = Some("/repo/a.rs".into());
        assert_eq!(named.tab_name().as_deref(), Some("diff"));
        assert_eq!(pane(PaneType::Term, None).tab_name(), None);
        assert_eq!(pane(PaneType::Term, Some("logs")).tab_name().as_deref(), Some("logs"));
        assert_eq!(pane(PaneType::Ios, None).tab_name(), None);
    }

    #[test]
    fn a_web_pane_is_its_page_title_or_its_host_until_the_title_loads() {
        let mut web = pane(PaneType::Web, None);
        web.url = Some("https://user@docs.rs:443/serde/latest?x=1".into());
        assert_eq!(web.tab_name().as_deref(), Some("docs.rs"));
        web.title = Some("serde - Rust".into());
        assert_eq!(web.tab_name().as_deref(), Some("serde - Rust"));
        web.title = Some("  ".into());
        assert_eq!(web.tab_name().as_deref(), Some("docs.rs"));
        web.url = Some("http://[::1]:5173/".into());
        assert_eq!(web.tab_name().as_deref(), Some("::1"));
        web.url = Some("file:///Users/me/mockups/tabs.html".into());
        assert_eq!(web.tab_name().as_deref(), Some("tabs.html"));
    }
}
