//! Persistence (docs/kmux-spec.md §3.6): the layout as a versioned state,
//! ported from the reference model (reference/kmux/core.js: exportState,
//! importState). Panes keep their IDs. Restoring starts every pane again:
//! terminals as new terminals in their cwd with their command, web panes at
//! their URL, md panes on their file, iOS panes relaunching their app.
//!
//! ```text
//! { kmux: "state", version: 1, key, counters: { pane, tab, window },
//!   windows: [{ id, frame?, active, focused, zoomed, tabCount,
//!               tabs: [{ id, title, lastFocus, layout }] }],
//!   panes: [{ id, name, type, …what it shows }] }
//! ```
//!
//! A layout is `{ pane: ID }` or `{ split: row|column, children: [{ …layout, size }] }`.
//! The core reads and writes `panes` and the pane counter; the layout host
//! the rest.

use crate::error::KmuxError;
use crate::json::{encode, is_true, num, opt, string, Map, Value};
use crate::pane::{History, Pane, PaneType, Panes};
use std::collections::HashSet;

pub const STATE_VERSION: u32 = 1;

pub(crate) fn bad(message: &str) -> KmuxError {
    KmuxError::new("bad_request", format!("bad state: {message}"))
}

/// A value as JSON, for messages.
pub(crate) fn quoted(value: Option<&Value>) -> String {
    encode(value.unwrap_or(&Value::Null))
}

/// `n` from an ID such as `p3`, if it is exactly that.
pub(crate) fn number_in(id: &str, prefix: &str) -> Option<usize> {
    let n: usize = id.strip_prefix(prefix)?.parse().ok()?;
    (n > 0 && n < 1_000_000_000 && format!("{prefix}{n}") == id).then_some(n)
}

/// A whole number from 0 up to a billion.
pub(crate) fn whole(value: Option<&Value>) -> Option<usize> {
    let n = value?.as_f64()?;
    (n.round() == n && n >= 0.0 && n < 1e9).then_some(n as usize)
}

pub(crate) fn export_panes(panes: &Panes) -> Value {
    Value::Array(
        panes
            .iter()
            .map(|pane| {
                let mut out = Map::new();
                out.insert("id".into(), pane.id.clone().into());
                out.insert("name".into(), opt(pane.name.as_deref()));
                out.insert("type".into(), pane.kind.as_str().into());
                match pane.kind {
                    PaneType::Term => {
                        out.insert("cmd".into(), opt(pane.command.as_deref()));
                        out.insert("cwd".into(), opt(pane.cwd.as_deref()));
                        out.insert("session".into(), opt(pane.session.as_deref()));
                    }
                    PaneType::Web => {
                        out.insert("url".into(), opt(pane.url.as_deref()));
                        out.insert("history".into(), Value::Bool(pane.history.is_some()));
                    }
                    PaneType::Md => {
                        out.insert("path".into(), opt(pane.path.as_deref()));
                        out.insert("zoom".into(), num(pane.zoom.unwrap_or(1.0)));
                    }
                    PaneType::Ios => {
                        out.insert("app".into(), opt(pane.app.as_deref()));
                        out.insert("device".into(), opt(pane.device.as_deref()));
                    }
                }
                Value::Object(out)
            })
            .collect(),
    )
}

/// Checks a saved state's version and panes and builds the panes. Returns
/// them with the saved pane counter.
pub(crate) fn import_panes(state: &Value) -> Result<(Vec<Pane>, usize), KmuxError> {
    if !state.is_object() {
        return Err(bad("not an object"));
    }
    if state.get("version").and_then(Value::as_f64) != Some(STATE_VERSION as f64) {
        return Err(KmuxError::new(
            "bad_request",
            format!("unsupported state version {} (this kmux reads {STATE_VERSION})", quoted(state.get("version"))),
        ));
    }
    let (Some(saved_panes), Some(_)) = (state.get("panes").and_then(Value::as_array), state.get("windows").and_then(Value::as_array)) else {
        return Err(bad("needs panes and windows"));
    };
    let mut counter = whole(state.get("counters").and_then(|c| c.get("pane"))).unwrap_or(0);

    let mut panes: Vec<Pane> = vec![];
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    for item in saved_panes {
        let id = string(item, "id").filter(|_| item.is_object());
        let n = id.and_then(|id| number_in(id, "p"));
        let (Some(id), Some(n)) = (id, n) else { return Err(bad(&format!("pane {}", quoted(item.get("id"))))) };
        if !ids.insert(id.to_string()) {
            return Err(bad(&format!("pane {}", quoted(item.get("id")))));
        }
        let Some(kind) = string(item, "type").and_then(PaneType::parse) else {
            return Err(bad(&format!("pane {id} has unknown type {}", quoted(item.get("type")))));
        };
        let name = string(item, "name").map(String::from);
        if let Some(name) = &name {
            if name.is_empty() || !names.insert(name.clone()) {
                return Err(bad(&format!("pane name {} is used twice", quoted(Some(&Value::String(name.clone()))))));
            }
        }
        let needed = match kind {
            PaneType::Web => Some("url"),
            PaneType::Md => Some("path"),
            PaneType::Ios => Some("app"),
            PaneType::Term => None,
        };
        if let Some(needed) = needed {
            if string(item, needed).map_or(true, str::is_empty) {
                return Err(bad(&format!("{} pane {id} has no {needed}", kind.as_str())));
            }
        }
        let mut pane = Pane::new(id, name, kind);
        let field = |key: &str| string(item, key).map(String::from);
        pane.command = field("cmd");
        pane.cwd = field("cwd");
        pane.session = field("session");
        pane.url = field("url");
        pane.path = field("path");
        pane.app = field("app");
        pane.device = field("device");
        if kind == PaneType::Md {
            pane.zoom = item.get("zoom").and_then(Value::as_f64).filter(|z| *z > 0.0);
        }
        if (kind == PaneType::Web && is_true(item, "history")) || kind == PaneType::Md {
            pane.history = Some(History::default());
        }
        if kind != PaneType::Term {
            pane.command = None;
            pane.cwd = None;
            pane.session = None;
        }
        panes.push(pane);
        counter = counter.max(n);
    }
    Ok((panes, counter))
}
