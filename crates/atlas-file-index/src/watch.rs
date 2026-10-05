//! inotify, for freshness with no polling: the engine thread blocks in `poll`
//! until the kernel says a watched folder changed.

use std::ffi::CString;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

/// Events that change a listing or what it shows about a file.
const MASK: u32 = libc::IN_CREATE
    | libc::IN_DELETE
    | libc::IN_MOVED_FROM
    | libc::IN_MOVED_TO
    | libc::IN_CLOSE_WRITE
    | libc::IN_ATTRIB
    | libc::IN_DELETE_SELF
    | libc::IN_MOVE_SELF
    | libc::IN_ONLYDIR
    | libc::IN_DONT_FOLLOW;

pub struct Inotify {
    fd: OwnedFd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub wd: i32,
    pub mask: u32,
}

impl Event {
    pub fn overflow(&self) -> bool {
        self.mask & libc::IN_Q_OVERFLOW != 0
    }
    /// The watch is gone (the folder was removed or unmounted).
    pub fn ignored(&self) -> bool {
        self.mask & libc::IN_IGNORED != 0
    }
}

impl Inotify {
    pub fn new() -> io::Result<Inotify> {
        // SAFETY: inotify_init1 returns a new fd or -1.
        let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fd is new and owned by nobody else.
        Ok(Inotify {
            fd: unsafe { OwnedFd::from_raw_fd(fd) },
        })
    }

    pub fn fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }

    pub fn add_watch(&self, path: &Path) -> io::Result<i32> {
        let c = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        // SAFETY: c is a valid NUL-terminated string for the call.
        let wd = unsafe { libc::inotify_add_watch(self.fd(), c.as_ptr(), MASK) };
        if wd < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(wd)
        }
    }

    pub fn rm_watch(&self, wd: i32) {
        // SAFETY: plain call; an already-removed watch just fails.
        unsafe {
            libc::inotify_rm_watch(self.fd(), wd);
        }
    }

    /// Read what is queued (never blocks). Bounded per call so a flood cannot
    /// starve the caller.
    pub fn read_events(&self, out: &mut Vec<Event>) -> io::Result<()> {
        let mut buf = vec![0u8; 64 * 1024];
        for _ in 0..64 {
            // SAFETY: reads at most buf.len() bytes into buf.
            let n = unsafe { libc::read(self.fd(), buf.as_mut_ptr().cast(), buf.len()) };
            if n < 0 {
                let e = io::Error::last_os_error();
                return match e.kind() {
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted => Ok(()),
                    _ => Err(e),
                };
            }
            if n == 0 {
                return Ok(());
            }
            parse_events(&buf[..n as usize], out);
        }
        Ok(())
    }
}

/// Parse `struct inotify_event` records (wd i32, mask u32, cookie u32, len u32,
/// then `len` bytes of name), stopping at a record that does not fit.
pub fn parse_events(buf: &[u8], out: &mut Vec<Event>) {
    let mut i = 0;
    while i + 16 <= buf.len() {
        let rd = |o: usize| [buf[i + o], buf[i + o + 1], buf[i + o + 2], buf[i + o + 3]];
        let wd = i32::from_ne_bytes(rd(0));
        let mask = u32::from_ne_bytes(rd(4));
        let len = u32::from_ne_bytes(rd(12)) as usize;
        let Some(next) = (i + 16).checked_add(len) else {
            return;
        };
        if next > buf.len() {
            return;
        }
        out.push(Event { wd, mask });
        i = next;
    }
}

/// Watches the kernel allows this user, from `/proc`.
pub fn max_user_watches() -> Option<usize> {
    std::fs::read_to_string("/proc/sys/fs/inotify/max_user_watches")
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// The watch budget: 64k or half of the user's limit, whichever is less.
pub fn budget_from(limit: Option<usize>) -> usize {
    (limit.unwrap_or(8192) / 2).clamp(16, 65_536)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdir::Scratch;

    #[test]
    fn budget_rule() {
        assert_eq!(budget_from(Some(1_000_000)), 65_536);
        assert_eq!(budget_from(Some(8192)), 4096);
        assert_eq!(budget_from(None), 4096);
        assert_eq!(budget_from(Some(0)), 16);
    }

    #[test]
    fn parses_and_bounds_events() {
        let mut b = Vec::new();
        for (wd, mask, name) in [
            (1i32, libc::IN_CREATE, &b"abc\0"[..]),
            (2, libc::IN_Q_OVERFLOW, &b""[..]),
        ] {
            b.extend_from_slice(&wd.to_ne_bytes());
            b.extend_from_slice(&mask.to_ne_bytes());
            b.extend_from_slice(&0u32.to_ne_bytes());
            b.extend_from_slice(&(name.len() as u32).to_ne_bytes());
            b.extend_from_slice(name);
        }
        let mut out = Vec::new();
        parse_events(&b, &mut out);
        assert_eq!(out.len(), 2);
        assert!(out[1].overflow());
        // truncated and hostile lengths stop the parse
        let mut out = Vec::new();
        parse_events(&b[..19], &mut out);
        assert!(out.is_empty());
        let mut hostile = b[..16].to_vec();
        hostile[12..16].copy_from_slice(&u32::MAX.to_ne_bytes());
        parse_events(&hostile, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn sees_a_new_file() {
        let t = Scratch::new("inotify");
        let ino = Inotify::new().unwrap();
        let wd = ino.add_watch(&t.0).unwrap();
        std::fs::write(t.0.join("f"), b"x").unwrap();
        let mut ev = Vec::new();
        let ready = crate::sys::poll_readable(&[ino.fd()], 2000).unwrap();
        assert!(ready[0]);
        ino.read_events(&mut ev).unwrap();
        assert!(ev.iter().any(|e| e.wd == wd));
        assert!(
            ino.add_watch(&t.0.join("f")).is_err(),
            "only folders are watched"
        );
    }
}
