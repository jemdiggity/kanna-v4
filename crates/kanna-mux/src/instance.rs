use crate::brand::Brand;
use crate::error::KmuxError;
use crate::socket_server::SocketServer;
use std::collections::HashMap;
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

    pub fn is_default(&self) -> bool {
        self.name == DEFAULT_NAME
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

    /// Names are short and safe in a file name: letters, digits, `-` and `_`.
    pub fn validate(name: &str) -> Result<(), KmuxError> {
        let ok = !name.is_empty() && name.chars().count() <= 32 && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if ok {
            Ok(())
        } else {
            Err(KmuxError::new("bad_request", format!("bad instance name \"{name}\": use letters, digits, - and _ (up to 32)")))
        }
    }

    /// The instance this process should be: `--instance NAME` on the command
    /// line, else `$KMUX_INSTANCE`, else the default. `$KMUX_SOCKET` overrides
    /// the socket path.
    pub fn current(arguments: &[String], environment: &HashMap<String, String>) -> Result<Instance, KmuxError> {
        let brand = Brand::current();
        let mut name = environment.get(&brand.variable("INSTANCE")).filter(|v| !v.is_empty()).cloned().unwrap_or_else(|| DEFAULT_NAME.into());
        if let Some(flag) = arguments.iter().position(|a| a == "--instance") {
            name = arguments.get(flag + 1).cloned().ok_or_else(|| KmuxError::new("bad_request", "--instance needs a NAME"))?;
        }
        Instance::validate(&name)?;
        let socket = environment.get(&brand.variable("SOCKET")).filter(|v| !v.is_empty()).cloned();
        Ok(Instance::with_socket(&name, socket))
    }

    /// `current` for this process's arguments and environment.
    pub fn from_process() -> Result<Instance, KmuxError> {
        Instance::current(&std::env::args().collect::<Vec<_>>(), &std::env::vars().collect())
    }

    /// The names of instances whose sockets exist in `directory`, default first.
    /// (Some may be stale: check with `SocketServer::is_listening`.)
    pub fn socket_names() -> Vec<String> {
        let brand = &Brand::current().id;
        let files = std::fs::read_dir(Instance::directory()).map(|d| d.filter_map(|e| e.ok()?.file_name().into_string().ok()).collect::<Vec<_>>()).unwrap_or_default();
        let mut names: Vec<String> = files
            .iter()
            .filter_map(|file| {
                if *file == format!("{brand}.sock") {
                    return Some(DEFAULT_NAME.to_string());
                }
                file.strip_prefix(&format!("{brand}-"))?.strip_suffix(".sock").map(String::from)
            })
            .collect();
        names.sort_by(|a, b| {
            let key = |n: &String| if n == DEFAULT_NAME { String::new() } else { n.clone() };
            key(a).cmp(&key(b))
        });
        names
    }

    /// The first unused name of the form 2, 3, …, for "New Instance".
    pub fn unused_name() -> String {
        let mut n = 2;
        while SocketServer::is_listening(&Instance::socket_path_for(&n.to_string())) {
            n += 1;
        }
        n.to_string()
    }
}
