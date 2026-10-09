//! Signals delivered through a self-pipe so poll loops can handle them.

use anyhow::Result;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

static mut WRITE_FD: libc::c_int = -1;

extern "C" fn handler(signo: libc::c_int) {
    unsafe {
        let fd = WRITE_FD;
        if fd >= 0 {
            let byte = [signo as u8];
            libc::write(fd, byte.as_ptr().cast(), 1);
        }
    }
}

pub fn set_nonblocking(fd: libc::c_int) {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
    }
}

pub fn set_cloexec(fd: libc::c_int) {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFD);
        libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC);
    }
}

/// Routes `signals` into the returned pipe; each byte read is a signal number.
/// One pipe per process.
pub fn pipe(signals: &[libc::c_int]) -> Result<OwnedFd> {
    let mut fds = [-1; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    for fd in fds {
        set_nonblocking(fd);
        set_cloexec(fd);
    }
    let read = unsafe { OwnedFd::from_raw_fd(fds[0]) };
    let write = unsafe { OwnedFd::from_raw_fd(fds[1]) };
    unsafe {
        WRITE_FD = write.as_raw_fd();
        std::mem::forget(write);
        for &signo in signals {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = handler as *const () as usize;
            action.sa_flags = libc::SA_RESTART;
            libc::sigemptyset(&mut action.sa_mask);
            if libc::sigaction(signo, &action, std::ptr::null_mut()) < 0 {
                return Err(std::io::Error::last_os_error().into());
            }
        }
    }
    Ok(read)
}

/// Drains the pipe and returns the signals received.
pub fn drain(fd: &OwnedFd) -> Vec<libc::c_int> {
    let mut out = Vec::new();
    let mut buf = [0u8; 64];
    loop {
        let n = unsafe { libc::read(fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
        if n <= 0 {
            break;
        }
        out.extend(buf[..n as usize].iter().map(|&b| b as libc::c_int));
    }
    out
}
