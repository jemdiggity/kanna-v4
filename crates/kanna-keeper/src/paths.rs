//! Where the keeper keeps its socket, lock, pid and log.

use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// `$KANNA_STATE_DIR` if set, else `~/Library/Application Support/kanna` on
/// macOS and `$XDG_DATA_HOME/kanna` (`~/.local/share/kanna`) elsewhere.
pub fn default_state_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("KANNA_STATE_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    if cfg!(target_os = "macos") {
        return home.join("Library/Application Support/kanna");
    }
    std::env::var_os("XDG_DATA_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"))
        .join("kanna")
}

pub fn socket(dir: &Path) -> PathBuf {
    dir.join("keeper.sock")
}
pub fn lock(dir: &Path) -> PathBuf {
    dir.join("keeper.lock")
}
pub fn pid(dir: &Path) -> PathBuf {
    dir.join("keeper.pid")
}
pub fn log(dir: &Path) -> PathBuf {
    dir.join("keeper.log")
}

/// `sizeof(sockaddr_un.sun_path)`.
#[cfg(any(target_os = "linux", target_os = "android"))]
const SUN_PATH: usize = 108;
#[cfg(not(any(target_os = "linux", target_os = "android")))]
const SUN_PATH: usize = 104;

/// Creates the directory 0700, or checks that an existing one belongs to us
/// and tightens it to 0700.
pub fn ensure_dir(dir: &Path) -> io::Result<()> {
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    let meta = std::fs::metadata(dir)?;
    if meta.uid() != unsafe { libc::getuid() } {
        return Err(io::Error::other(format!("{} belongs to another user", dir.display())));
    }
    if meta.mode() & 0o077 != 0 {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    if socket(dir).as_os_str().len() >= SUN_PATH {
        return Err(io::Error::other(format!("socket path {} is too long", socket(dir).display())));
    }
    Ok(())
}
