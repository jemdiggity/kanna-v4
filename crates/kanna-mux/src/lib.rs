//! kmux's core, shared by the kmux app and products built on it: the control
//! protocol (docs/kmux-spec.md §7), panes and their lifecycle, kmux's layout
//! model, the saved state and its file, the control socket, and brand and
//! instance naming. The app plugs in through `PaneHost` (what runs in a
//! pane) and `LayoutHost` (where panes are).

mod brand;
mod core;
mod error;
mod exec;
pub mod fraction;
mod instance;
pub mod json;
mod layout;
pub mod model;
mod pane;
mod socket_server;
mod state;
mod state_file;

pub use crate::core::{normalize_url, resolve, CommandHandler, Core, CoreHandle, PaneHost, Responder, WeakCore, CORE_COMMANDS, DEBUG_COMMANDS};
pub use brand::Brand;
pub use error::KmuxError;
pub use exec::Wakeup;
pub use instance::{Instance, DEFAULT_NAME};
pub use layout::{LayoutHost, Placement, SavedLayout, Scope, SplitSide};
pub use model::Model;
pub use pane::{History, Pane, PaneState, PaneType, Panes};
pub use socket_server::{write_all, Handler, SocketServer, Takeover};
pub use state::STATE_VERSION;
pub use state_file::{Contents, StateFile};
