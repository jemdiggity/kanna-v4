use crate::brand::Brand;
use std::path::PathBuf;

/// One running kmux: a name and the socket it listens on. Several instances
/// can run at once, each with its own windows. The default instance listens
/// on `kmux.sock`; one named `work` listens on `kmux-work.sock` beside it.
/// (Other brands use their own names: `kanna.sock` in `…/kanna`.)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Instance {
    pub name: String,
    pub socket_path: String,
}

pub const DEFAULT_NAME: &str = "default";

impl Instance {
    pub fn new(name: &str) -> Instance {
        Instance { name: name.into(), socket_path: Instance::socket_path_for(name) }
    }

    pub fn with_socket(name: &str, socket_path: Option<String>) -> Instance {
        Instance { name: name.into(), socket_path: socket_path.unwrap_or_else(|| Instance::socket_path_for(name)) }
    }

    /// Where the sockets live: `~/Library/Application Support/kmux` on macOS,
    /// `$XDG_DATA_HOME/kmux` (`~/.local/share/kmux`) elsewhere.
    pub fn directory() -> String {
        let id = &Brand::current().id;
        let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        #[cfg(target_os = "macos")]
        let base = home.join("Library/Application Support");
        #[cfg(not(target_os = "macos"))]
        let base = std::env::var_os("XDG_DATA_HOME").filter(|d| !d.is_empty()).map(PathBuf::from).unwrap_or_else(|| home.join(".local/share"));
        base.join(id).to_string_lossy().into_owned()
    }

    pub fn socket_path_for(name: &str) -> String {
        let brand = &Brand::current().id;
        let file = if name == DEFAULT_NAME { format!("{brand}.sock") } else { format!("{brand}-{name}.sock") };
        PathBuf::from(Instance::directory()).join(file).to_string_lossy().into_owned()
    }
}
