use std::io;
use std::os::fd::{AsRawFd, OwnedFd};

pub struct Spec {
    pub id: String,
    pub command: Option<String>,
    pub cwd: Option<String>,
    pub env: Vec<(String, Option<String>)>,
    pub cols: u16,
    pub rows: u16,
}

pub struct Spawned {
    pub pid: i32,
    pub master: OwnedFd,
}

pub trait Spawner {
    fn spawn(&self, spec: &Spec) -> io::Result<Spawned>;
    fn close(&self, id: &str);
    fn acknowledge_exit(&self, id: &str);
}

pub fn set_size(master: &OwnedFd, cols: u16, rows: u16, width: u16, height: u16) {
    let size = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: width, ws_ypixel: height };
    unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ as _, &size) };
}
