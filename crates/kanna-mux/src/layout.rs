//! The layout seam. The core owns panes: what they show, their lifecycle,
//! and the commands that act on them alone (`send`, `restart`, `navigate`).
//! Where panes are (windows, tabs, splits, focus, zoom) belongs to a
//! `LayoutHost`. kmux uses its own `Model`; a product that owns its layout
//! (Kanna's app) supplies another.
//!
//! A layout host is called on the core's thread, with the core's state
//! borrowed, and answers at once. It must not call back into the core.

use crate::error::KmuxError;
use crate::json::{Map, Value};
use crate::pane::Panes;
use std::any::Any;
use std::collections::HashSet;

/// Which way `open` splits the focused pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitSide {
    Right,
    Down,
    /// Along the focused pane's longer side.
    Auto,
}

/// Where `open` puts a new pane.
#[derive(Clone, Debug)]
pub struct Placement {
    /// A window ID, `new`, or nil for the key window.
    pub window: Option<String>,
    pub split: SplitSide,
    /// The fraction of the split pane's space the new one takes.
    pub size: f64,
    /// In a new tab.
    pub new_tab: bool,
}

/// What `close` closes besides a single pane.
#[derive(Clone, Debug)]
pub enum Scope {
    Tab(String),
    /// A window ID, `new`, or nil for the key window.
    Window(Option<String>),
}

pub struct SavedLayout {
    pub placed: HashSet<String>,
    pub layout: Box<dyn Any>,
}

pub trait LayoutHost {
    /// The layout commands it answers (`focus`, `move`, …).
    fn commands(&self) -> &[&'static str];

    /// What `capabilities` lists for it under `features` (`windows`, `tabs`, …).
    fn features(&self) -> &[&'static str];

    /// Puts a new pane where `open` asked and focuses it. Fails, changing
    /// nothing, if the target doesn't exist. Returns the pane's window.
    fn place(&mut self, pane: &str, at: &Placement) -> Result<Option<String>, KmuxError>;

    /// The window and tab a pane is in.
    fn location(&self, pane: &str) -> Option<(String, String)>;

    /// The panes in a tab or window, for `close`.
    fn panes_in(&mut self, scope: &Scope) -> Result<Vec<String>, KmuxError>;

    /// Takes a closing pane out of the layout; its siblings share the space.
    /// `tidy` follows.
    fn remove(&mut self, pane: &str);

    /// Closes what is left empty and repairs focus.
    fn tidy(&mut self);

    /// Runs one of `commands`. `panes` resolves pane references and names.
    fn command(&mut self, cmd: &str, args: &Value, panes: &Panes) -> Result<Map, KmuxError>;

    /// `list`'s windows.
    fn windows(&self, panes: &Panes) -> Value;

    /// Its part of the saved state: `key`, `windows`, and `counters` for
    /// `tab` and `window` (docs/kmux-spec.md §3.6).
    fn export_state(&self) -> Map;

    fn read_state(&self, state: &Value, panes: &HashSet<String>) -> Result<SavedLayout, KmuxError>;

    fn install_state(&mut self, saved: SavedLayout);

    /// For `Core::with_layout`, to reach the concrete type.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
