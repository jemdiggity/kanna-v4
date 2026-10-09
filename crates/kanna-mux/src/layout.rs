use crate::error::KmuxError;
use crate::json::{Map, Value};
use crate::pane::Panes;
use std::any::Any;
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitSide {
    Right,
    Down,
    Auto,
}

#[derive(Clone, Debug)]
pub struct Placement {
    pub window: Option<String>,
    pub split: SplitSide,
    pub size: f64,
    pub new_tab: bool,
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
    fn commands(&self) -> &[&'static str];

    fn features(&self) -> &[&'static str];

    fn place(&mut self, pane: &str, at: &Placement) -> Result<Option<String>, KmuxError>;

    fn location(&self, pane: &str) -> Option<(String, String)>;

    fn panes_in(&mut self, scope: &Scope) -> Result<Vec<String>, KmuxError>;

    fn remove(&mut self, pane: &str);

    fn tidy(&mut self);

    fn command(&mut self, cmd: &str, args: &Value, panes: &Panes) -> Result<Map, KmuxError>;

    fn windows(&self, panes: &Panes) -> Value;

    fn export_state(&self) -> Map;

    fn read_state(&self, state: &Value, panes: &HashSet<String>) -> Result<SavedLayout, KmuxError>;

    fn install_state(&mut self, saved: SavedLayout);

    fn as_any_mut(&mut self) -> &mut dyn Any;
}
