//! Small wrappers over Linux calls: scheduling priority, an event fd to wake a
//! thread, `poll`, and reading one extended attribute. Everything unsafe in the
//! crate that is not the cache folder (`snapshot`) is here.

use std::ffi::{CStr, CString};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

/// Put the calling thread at nice 19 and idle I/O priority, so scanning never
/// competes with the user's work. Failures are ignored: a lower priority is a
/// courtesy, not a requirement.
pub fn set_idle_priority() {
    // SAFETY: plain system calls with integer arguments; the thread id is our own.
    unsafe {
        let tid = libc::syscall(libc::SYS_gettid) as libc::id_t;
        libc::setpriority(libc::PRIO_PROCESS, tid, 19);
        const IOPRIO_WHO_PROCESS: libc::c_long = 1;
        const IOPRIO_CLASS_IDLE: libc::c_long = 3;
        libc::syscall(
            libc::SYS_ioprio_set,
            IOPRIO_WHO_PROCESS,
            tid as libc::c_long,
            IOPRIO_CLASS_IDLE << 13,
        );
    }
}

/// Read the extended attribute `name` of `path` into `buf`, returning how many
/// bytes it holds. Never follows a symlink (`lgetxattr`): a link's own
/// attributes are read, which for `user.*` names means none. An attribute
/// longer than `buf` is an `ERANGE` error, not a cut value.
pub fn lgetxattr(path: &Path, name: &CStr, buf: &mut [u8]) -> io::Result<usize> {
    let c = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    // SAFETY: c and name are NUL-terminated for the call; the kernel writes at
    // most buf.len() bytes into buf.
    let n = unsafe {
        libc::lgetxattr(
            c.as_ptr(),
            name.as_ptr(),
            buf.as_mut_ptr().cast(),
            buf.len(),
        )
    };
    if n < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(n as usize)
    }
}

/// Set the extended attribute `name` of `path` to `value`, never following a
/// symlink. For tests and tools that write tags; the index only reads them.
#[doc(hidden)]
pub fn lsetxattr(path: &Path, name: &CStr, value: &[u8]) -> io::Result<()> {
    let c = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    // SAFETY: c and name are NUL-terminated for the call; the kernel reads at
    // most value.len() bytes from value.
    let r = unsafe {
        libc::lsetxattr(
            c.as_ptr(),
            name.as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
        )
    };
    if r < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// An eventfd: `signal` from any thread wakes a `poll` on `fd`.
pub struct EventFd(OwnedFd);

impl EventFd {
    pub fn new() -> io::Result<EventFd> {
        // SAFETY: eventfd returns a new fd or -1.
        let fd = unsafe { libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fd is new and owned by nobody else.
        Ok(EventFd(unsafe { OwnedFd::from_raw_fd(fd) }))
    }

    pub fn fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }

    pub fn signal(&self) {
        let one: u64 = 1;
        // SAFETY: writes 8 bytes from a live u64. A full counter (EAGAIN) still
        // leaves the fd readable, which is all that matters.
        unsafe {
            libc::write(self.fd(), (&one as *const u64).cast(), 8);
        }
    }

    pub fn drain(&self) {
        let mut v: u64 = 0;
        // SAFETY: reads 8 bytes into a live u64; EAGAIN means already empty.
        unsafe {
            libc::read(self.fd(), (&mut v as *mut u64).cast(), 8);
        }
    }
}

/// Wait for any of `fds` to be readable; `timeout_ms` of -1 waits for ever.
/// Returns which are readable (or hung up). An interrupted wait returns none.
pub fn poll_readable(fds: &[RawFd], timeout_ms: i32) -> io::Result<Vec<bool>> {
    let mut p: Vec<libc::pollfd> = fds
        .iter()
        .map(|&fd| libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        })
        .collect();
    // SAFETY: p is a live slice of pollfd of the length passed.
    let r = unsafe { libc::poll(p.as_mut_ptr(), p.len() as libc::nfds_t, timeout_ms) };
    if r < 0 {
        let e = io::Error::last_os_error();
        if e.kind() == io::ErrorKind::Interrupted {
            return Ok(vec![false; fds.len()]);
        }
        return Err(e);
    }
    Ok(p.iter()
        .map(|x| x.revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eventfd_wakes_poll() {
        let e = EventFd::new().unwrap();
        assert_eq!(poll_readable(&[e.fd()], 0).unwrap(), [false]);
        e.signal();
        assert_eq!(poll_readable(&[e.fd()], 1000).unwrap(), [true]);
        e.drain();
        assert_eq!(poll_readable(&[e.fd()], 0).unwrap(), [false]);
    }

    #[test]
    fn priority_call_does_not_fail() {
        std::thread::spawn(set_idle_priority).join().unwrap();
    }
}
