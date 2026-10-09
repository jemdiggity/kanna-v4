//! Sending and receiving bytes with file descriptors attached
//! (`SCM_RIGHTS`), and the peer's uid, on Unix sockets.

use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};

/// Most descriptors one message carries. The keeper sends at most one per
/// line; the room is for a peer that sends more by mistake (the extras are
/// closed).
const MAX_FDS: usize = 16;

fn cmsg_space(n: usize) -> usize {
    unsafe { libc::CMSG_SPACE((n * std::mem::size_of::<RawFd>()) as u32) as usize }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
const SEND_FLAGS: libc::c_int = libc::MSG_NOSIGNAL;
#[cfg(not(any(target_os = "linux", target_os = "android")))]
const SEND_FLAGS: libc::c_int = 0;

#[cfg(any(target_os = "linux", target_os = "android"))]
const RECV_FLAGS: libc::c_int = libc::MSG_CMSG_CLOEXEC;
#[cfg(not(any(target_os = "linux", target_os = "android")))]
const RECV_FLAGS: libc::c_int = 0;

/// One `sendmsg`: `bytes`, with `fds` attached to the first byte. Returns
/// how many bytes went (the fds went if it's more than 0).
pub fn send(sock: RawFd, bytes: &[u8], fds: &[RawFd], nonblocking: bool) -> io::Result<usize> {
    let mut iov = libc::iovec { iov_base: bytes.as_ptr() as *mut _, iov_len: bytes.len() };
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    // u64 for the alignment cmsghdr needs.
    let mut control = vec![0u64; cmsg_space(fds.len().max(1)).div_ceil(8)];
    if !fds.is_empty() {
        msg.msg_control = control.as_mut_ptr().cast();
        msg.msg_controllen = cmsg_space(fds.len()) as _;
        unsafe {
            let cmsg = libc::CMSG_FIRSTHDR(&msg);
            (*cmsg).cmsg_level = libc::SOL_SOCKET;
            (*cmsg).cmsg_type = libc::SCM_RIGHTS;
            (*cmsg).cmsg_len = libc::CMSG_LEN(std::mem::size_of_val(fds) as u32) as _;
            std::ptr::copy_nonoverlapping(fds.as_ptr(), libc::CMSG_DATA(cmsg).cast::<RawFd>(), fds.len());
        }
    }
    let flags = SEND_FLAGS | if nonblocking { libc::MSG_DONTWAIT } else { 0 };
    loop {
        let n = unsafe { libc::sendmsg(sock, &msg, flags) };
        if n >= 0 {
            return Ok(n as usize);
        }
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::Interrupted {
            return Err(e);
        }
    }
}

/// One `recvmsg` into `buf`. Returns the byte count (0 at end of file) and
/// the descriptors that came with them, close-on-exec.
pub fn recv(sock: RawFd, buf: &mut [u8], nonblocking: bool) -> io::Result<(usize, Vec<OwnedFd>)> {
    let mut iov = libc::iovec { iov_base: buf.as_mut_ptr().cast(), iov_len: buf.len() };
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    let mut control = vec![0u64; cmsg_space(MAX_FDS).div_ceil(8)];
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = cmsg_space(MAX_FDS) as _;
    let flags = RECV_FLAGS | if nonblocking { libc::MSG_DONTWAIT } else { 0 };
    let n = loop {
        let n = unsafe { libc::recvmsg(sock, &mut msg, flags) };
        if n >= 0 {
            break n as usize;
        }
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::Interrupted {
            return Err(e);
        }
    };
    let mut fds = Vec::new();
    unsafe {
        let mut cmsg = libc::CMSG_FIRSTHDR(&msg);
        while !cmsg.is_null() {
            if (*cmsg).cmsg_level == libc::SOL_SOCKET && (*cmsg).cmsg_type == libc::SCM_RIGHTS {
                let data = libc::CMSG_DATA(cmsg).cast::<RawFd>();
                let len = (*cmsg).cmsg_len as usize - (data as usize - cmsg as usize);
                for i in 0..len / std::mem::size_of::<RawFd>() {
                    let fd = std::ptr::read_unaligned(data.add(i));
                    #[cfg(not(any(target_os = "linux", target_os = "android")))]
                    crate::sig::set_cloexec(fd);
                    fds.push(OwnedFd::from_raw_fd(fd));
                }
            }
            cmsg = libc::CMSG_NXTHDR(&msg, cmsg);
        }
    }
    if msg.msg_flags & libc::MSG_CTRUNC != 0 {
        return Err(io::Error::other("too many file descriptors in one message"));
    }
    Ok((n, fds))
}

/// The uid of the process at the other end of a Unix socket.
pub fn peer_uid(sock: &impl AsRawFd) -> io::Result<libc::uid_t> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        let rc = unsafe {
            libc::getsockopt(
                sock.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut cred as *mut libc::ucred).cast(),
                &mut len,
            )
        };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(cred.uid)
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        let (mut uid, mut gid) = (0, 0);
        if unsafe { libc::getpeereid(sock.as_raw_fd(), &mut uid, &mut gid) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(uid)
    }
}
