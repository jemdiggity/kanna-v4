//! Where the keeper keeps its socket, lock, pid and log.

use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::DirBuilderExt;
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
    let path = std::ffi::CString::new(dir.as_os_str().as_bytes())
        .map_err(|_| io::Error::other(format!("{} contains NUL", dir.display())))?;
    let fd = unsafe {
        libc::open(path.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
    };
    if fd < 0 {
        let e = io::Error::last_os_error();
        if e.raw_os_error() == Some(libc::ELOOP) || e.raw_os_error() == Some(libc::ENOTDIR) {
            return Err(io::Error::other(format!("{} is not a directory (a symlink?)", dir.display())));
        }
        return Err(e);
    }
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(fd.as_raw_fd(), &mut st) } != 0 {
        return Err(io::Error::last_os_error());
    }
    if st.st_uid != unsafe { libc::getuid() } {
        return Err(io::Error::other(format!("{} belongs to another user", dir.display())));
    }
    if st.st_mode as u32 & 0o077 != 0 && unsafe { libc::fchmod(fd.as_raw_fd(), 0o700) } != 0 {
        return Err(io::Error::last_os_error());
    }
    if socket(dir).as_os_str().len() >= SUN_PATH {
        return Err(io::Error::other(format!("socket path {} is too long", socket(dir).display())));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kkp{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn ensure_dir_refuses_a_symlink() {
        let dir = scratch("link");
        let target = dir.join("target");
        std::fs::create_dir(&target).unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).unwrap();
        let link = dir.join("state");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(ensure_dir(&link).is_err());
        let mode = std::fs::metadata(&target).unwrap().mode() & 0o777;
        assert_eq!(mode, 0o755);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ensure_dir_creates_and_tightens() {
        let dir = scratch("tighten");
        let state = dir.join("a/state");
        ensure_dir(&state).unwrap();
        assert_eq!(std::fs::metadata(&state).unwrap().mode() & 0o777, 0o700);
        std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o755)).unwrap();
        ensure_dir(&state).unwrap();
        assert_eq!(std::fs::metadata(&state).unwrap().mode() & 0o777, 0o700);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
