//! C ABI for the Home page and Connect to Server. The logic is in the core
//! (`home`, `servers`); these functions move bytes. Text goes in as pointer
//! and length and comes out in a caller's buffer; the return value is the
//! length, so a short buffer can be retried with the right size.
//!
//! Functions with several text inputs take them as one buffer, the parts
//! separated by a NUL byte; a part that holds a NUL itself makes the count
//! wrong and the call refuses.

use crate::ffi::{bytes, put};
use atlas_explorer_core::home::{self, Frequent};
use atlas_explorer_core::servers::{self, PROTOCOLS, Protocol, Recents};

fn text<'a>(ptr: *const u8, len: usize) -> std::borrow::Cow<'a, str> {
    // SAFETY: callers pass what their own contract promises.
    String::from_utf8_lossy(unsafe { bytes(ptr, len) })
}

/// Whether the text is the Home page's address (`home:` or `home:/`).
///
/// # Safety
/// `url` covers its length (or is null with length 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_home_is_home(url: *const u8, len: usize) -> bool {
    home::is_home(&text(url, len))
}

/// The key a folder is counted under, or 0 bytes when it is not counted.
/// The input is the folder's URL (percent-encoded), a NUL and the home
/// folder's plain path.
///
/// # Safety
/// `input` covers its length (or is null with length 0); `out` points to
/// `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_home_key(
    input: *const u8,
    len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        let t = text(input, len);
        let mut parts = t.split('\0');
        let (Some(url), Some(home_dir), None) = (parts.next(), parts.next(), parts.next()) else {
            return 0;
        };
        match home::key_of(url, home_dir) {
            // SAFETY: `out` as promised.
            Some(k) => unsafe { put(k.as_bytes(), out, cap) },
            None => 0,
        }
    })
}

/// The saved counts after a visit to `key` at time `now`, as the text to save.
///
/// # Safety
/// Each pointer covers its length (or is null with length 0); `out` points to
/// `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_home_visit(
    saved: *const u8,
    saved_len: usize,
    key: *const u8,
    key_len: usize,
    now: i64,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        let mut f = Frequent::parse(&text(saved, saved_len));
        f.visit(&text(key, key_len), now);
        // SAFETY: `out` as promised.
        unsafe { put(f.to_text().as_bytes(), out, cap) }
    })
}

/// The saved counts without `key`.
///
/// # Safety
/// As for `telamon_home_visit`, without `now`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_home_forget(
    saved: *const u8,
    saved_len: usize,
    key: *const u8,
    key_len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        let mut f = Frequent::parse(&text(saved, saved_len));
        f.forget(&text(key, key_len));
        // SAFETY: `out` as promised.
        unsafe { put(f.to_text().as_bytes(), out, cap) }
    })
}

/// The most visited folders, most visited first, one per line as the count,
/// a tab and the key; at most `n`.
///
/// # Safety
/// `saved` covers its length (or is null with length 0); `out` points to
/// `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_home_top(
    saved: *const u8,
    saved_len: usize,
    n: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        let f = Frequent::parse(&text(saved, saved_len));
        let mut s = String::new();
        for e in f.top(n) {
            s.push_str(&format!("{}\t{}\n", e.count, e.key));
        }
        // SAFETY: `out` as promised.
        unsafe { put(s.as_bytes(), out, cap) }
    })
}

/// A number of the Home page: 0 the most folders counted, 1 the most folders
/// listed, 2 the fewest visits that make a folder frequent, 3 the most
/// recent files listed.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_home_limit(which: u32) -> usize {
    match which {
        0 => home::MAX_FOLDERS,
        1 => home::SHOWN,
        2 => home::MIN_VISITS as usize,
        3 => RECENT_SHOWN,
        _ => 0,
    }
}

/// How many recent files the page lists.
const RECENT_SHOWN: usize = 10;

/// The recent files of the freedesktop list (`recently-used.xbel`) at the
/// path given: the newest first, one per line as the time of last use, a
/// tab and the file's URL (percent-encoded); only files that still exist
/// (a stat each, so call this from a worker), at most `n`. Read as untrusted
/// text with the size and entry limits of the index's reader.
///
/// # Safety
/// `path` covers its length (or is null with length 0); `out` points to
/// `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_home_recent_files(
    path: *const u8,
    path_len: usize,
    n: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        let p = text(path, path_len);
        let s = recent_files(std::path::Path::new(&*p), n);
        // SAFETY: `out` as promised.
        unsafe { put(s.as_bytes(), out, cap) }
    })
}

fn recent_files(xbel: &std::path::Path, n: usize) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut list = atlas_file_index::recent::load(xbel);
    // Newest first; a file listed twice keeps its newest time.
    list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut seen = std::collections::HashSet::new();
    let mut out = String::new();
    let mut kept = 0;
    // A list full of paths that are gone (or on a dead mount) would stat each
    // one: look at a bounded number.
    for (path, time) in list.into_iter().take(n.saturating_mul(8).max(64)) {
        if kept >= n {
            break;
        }
        if !seen.insert(path.clone()) {
            continue;
        }
        let p = std::path::Path::new(std::ffi::OsStr::from_bytes(&path));
        // A file, not a folder, and one that is still there.
        if !std::fs::metadata(p).is_ok_and(|m| m.is_file()) {
            continue;
        }
        let uri = atlas_file_index::uri::path_to_uri(&path);
        out.push_str(&format!("{time}\t{uri}\n"));
        kept += 1;
    }
    out
}

// ---- Connect to Server ----

/// How many protocols Connect to Server lists.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_servers_protocol_count() -> usize {
    PROTOCOLS.len()
}

/// A protocol's scheme (`which` 0) or its name in the list (1); 0 bytes for
/// a protocol that is none.
///
/// # Safety
/// `out` points to `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_servers_protocol(
    code: u32,
    which: u32,
    out: *mut u8,
    cap: usize,
) -> usize {
    let Some(p) = Protocol::from_code(code) else {
        return 0;
    };
    let s = if which == 0 { p.scheme() } else { p.label() };
    // SAFETY: `out` as promised.
    unsafe { put(s.as_bytes(), out, cap) }
}

/// A protocol's usual port, and 0 for a protocol that is none.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_servers_protocol_port(code: u32) -> u32 {
    Protocol::from_code(code).map_or(0, |p| u32::from(p.default_port()))
}

/// Whether the protocol is encrypted; false for a protocol that is none.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_servers_protocol_encrypted(code: u32) -> bool {
    Protocol::from_code(code).is_some_and(|p| p.encrypted())
}

/// The code of the protocol with this scheme, or -1.
///
/// # Safety
/// `scheme` covers its length (or is null with length 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_servers_protocol_code(scheme: *const u8, len: usize) -> i32 {
    Protocol::from_scheme(&text(scheme, len)).map_or(-1, |p| p as i32)
}

/// The address built from Connect to Server's fields. The input is the
/// protocol's code as digits, then the server, the folder and the user, each
/// after a NUL. The output is `U` and the address, or `E` and the reason in
/// plain words.
///
/// # Safety
/// `input` covers its length (or is null with length 0); `out` points to
/// `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_servers_build(
    input: *const u8,
    len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        let t = text(input, len);
        let parts: Vec<&str> = t.split('\0').collect();
        let answer = match parts.as_slice() {
            [code, server, folder, user] => {
                match code
                    .parse::<u32>()
                    .ok()
                    .and_then(Protocol::from_code)
                    .ok_or("Choose a protocol.")
                    .and_then(|p| servers::build(p, server, folder, user))
                {
                    Ok(url) => format!("U{url}"),
                    Err(why) => format!("E{why}"),
                }
            }
            _ => "EThat isn't something Files can connect to.".to_string(),
        };
        // SAFETY: `out` as promised.
        unsafe { put(answer.as_bytes(), out, cap) }
    })
}

/// "Not encrypted" for a location on a server that does not protect what it
/// carries, 0 bytes otherwise.
///
/// # Safety
/// `url` covers its length (or is null with length 0); `out` points to `cap`
/// writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_servers_note(
    url: *const u8,
    len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        match servers::security_note(&text(url, len)) {
            // SAFETY: `out` as promised.
            Some(s) => unsafe { put(s.as_bytes(), out, cap) },
            None => 0,
        }
    })
}

/// An address taken apart for Connect to Server's fields: the protocol's code
/// as digits, then the server, the folder and the user, each after a NUL; 0
/// bytes for an address that is not one of ours (or holds a password).
///
/// # Safety
/// As for `telamon_servers_note`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_servers_parse(
    url: *const u8,
    len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        match servers::parse_url(&text(url, len)) {
            Some(p) => {
                let s = format!(
                    "{}\0{}\0{}\0{}",
                    p.protocol as u32, p.server, p.folder, p.user
                );
                // SAFETY: `out` as promised.
                unsafe { put(s.as_bytes(), out, cap) }
            }
            None => 0,
        }
    })
}

/// An address as the recent list keeps it, 0 bytes when it is not one of ours.
///
/// # Safety
/// As for `telamon_servers_note`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_servers_clean(
    url: *const u8,
    len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        match servers::clean_recent(&text(url, len)) {
            // SAFETY: `out` as promised.
            Some(s) => unsafe { put(s.as_bytes(), out, cap) },
            None => 0,
        }
    })
}

/// How a recent server is listed (decoded; the caller makes it safe to show).
///
/// # Safety
/// As for `telamon_servers_note`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_servers_label(
    url: *const u8,
    len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        let s = servers::recent_label(&text(url, len));
        // SAFETY: `out` as promised.
        unsafe { put(s.as_bytes(), out, cap) }
    })
}

/// The saved list of recent servers (one address per line) cleaned, that is
/// every line that is not one of ours dropped, as the text to save.
///
/// # Safety
/// `saved` covers its length (or is null with length 0); `out` points to
/// `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_servers_recent_clean(
    saved: *const u8,
    saved_len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        let r = Recents::parse(&text(saved, saved_len));
        // SAFETY: `out` as promised.
        unsafe { put(r.to_text().as_bytes(), out, cap) }
    })
}

/// The saved list with `url` as the newest (an address that is not one of
/// ours leaves the list as it was, cleaned).
///
/// # Safety
/// As for `telamon_home_visit`, without `now`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_servers_recent_push(
    saved: *const u8,
    saved_len: usize,
    url: *const u8,
    url_len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        let mut r = Recents::parse(&text(saved, saved_len));
        r.push(&text(url, url_len));
        // SAFETY: `out` as promised.
        unsafe { put(r.to_text().as_bytes(), out, cap) }
    })
}

/// The saved list without `url`.
///
/// # Safety
/// As for `telamon_servers_recent_push`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_servers_recent_remove(
    saved: *const u8,
    saved_len: usize,
    url: *const u8,
    url_len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        let mut r = Recents::parse(&text(saved, saved_len));
        r.remove(&text(url, url_len));
        // SAFETY: `out` as promised.
        unsafe { put(r.to_text().as_bytes(), out, cap) }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn call(
        f: unsafe extern "C" fn(*const u8, usize, *mut u8, usize) -> usize,
        input: &[u8],
    ) -> Vec<u8> {
        let mut buf = vec![0u8; 8];
        // SAFETY: the buffers are as long as promised.
        let need = unsafe { f(input.as_ptr(), input.len(), buf.as_mut_ptr(), buf.len()) };
        if need > buf.len() {
            buf = vec![0u8; need];
            // SAFETY: as above, with the size asked for.
            let again = unsafe { f(input.as_ptr(), input.len(), buf.as_mut_ptr(), buf.len()) };
            assert_eq!(again, need);
        }
        buf.truncate(need);
        buf
    }

    #[test]
    fn the_server_address_goes_through_the_bridge() {
        let ok = call(telamon_servers_build, b"0\0nas.lan\0/srv\0me");
        assert_eq!(ok, b"Usftp://me@nas.lan/srv");
        // a NUL inside a field, a missing field, a bad protocol and a hostile field
        for bad in [
            &b"0\0na\0s\0/x\0me"[..],
            b"0\0nas\0/x",
            b"9\0nas\0\0",
            b"x\0nas\0\0",
            b"0\0user:pw@nas\0\0",
            b"",
        ] {
            let r = call(telamon_servers_build, bad);
            assert_eq!(r.first(), Some(&b'E'), "{:?}", String::from_utf8_lossy(bad));
        }
        assert_eq!(
            call(telamon_servers_note, b"ftp://h/x"),
            b"Not encrypted".to_vec()
        );
        assert!(call(telamon_servers_note, b"sftp://h/x").is_empty());
        let parts = call(telamon_servers_parse, b"smb://nas/share/a%20b");
        assert_eq!(parts, b"1\0nas\0/share/a b\0");
        assert!(call(telamon_servers_parse, b"smb://u:p@nas/share").is_empty());
        assert_eq!(
            call(telamon_servers_clean, b"SFTP://nas/x"),
            b"sftp://nas/x".to_vec()
        );
        assert_eq!(telamon_servers_protocol_count(), 6);
        assert_eq!(telamon_servers_protocol_port(0), 22);
        assert!(telamon_servers_protocol_encrypted(0));
        assert!(!telamon_servers_protocol_encrypted(2));
        assert!(!telamon_servers_protocol_encrypted(99));
        // SAFETY: a literal.
        assert_eq!(
            unsafe { telamon_servers_protocol_code(b"ftp".as_ptr(), 3) },
            2
        );
        assert_eq!(
            unsafe { telamon_servers_protocol_code(b"file".as_ptr(), 4) },
            -1
        );
        let mut buf = [0u8; 32];
        // SAFETY: a buffer of the length given.
        let n = unsafe { telamon_servers_protocol(3, 0, buf.as_mut_ptr(), buf.len()) };
        assert_eq!(&buf[..n], b"webdavs");
    }

    #[test]
    fn recent_servers_hold_no_password() {
        let mut saved = Vec::new();
        for u in ["sftp://a/x", "smb://u:pw@b/s", "ftp://c/"] {
            let mut out = vec![0u8; 512];
            // SAFETY: buffers as long as promised.
            let n = unsafe {
                telamon_servers_recent_push(
                    saved.as_ptr(),
                    saved.len(),
                    u.as_ptr(),
                    u.len(),
                    out.as_mut_ptr(),
                    out.len(),
                )
            };
            out.truncate(n);
            saved = out;
        }
        let text = String::from_utf8(saved.clone()).unwrap();
        assert_eq!(text, "ftp://c/\nsftp://a/x\n");
        assert!(!text.contains("pw"));
    }

    #[test]
    fn frequent_goes_through_the_bridge() {
        let mut saved: Vec<u8> = Vec::new();
        for now in [10, 20, 30] {
            let k = b"file:///d";
            let mut out = vec![0u8; 512];
            // SAFETY: buffers as long as promised.
            let n = unsafe {
                telamon_home_visit(
                    saved.as_ptr(),
                    saved.len(),
                    k.as_ptr(),
                    k.len(),
                    now,
                    out.as_mut_ptr(),
                    out.len(),
                )
            };
            out.truncate(n);
            saved = out;
        }
        let mut top = vec![0u8; 64];
        // SAFETY: buffers as long as promised.
        let n = unsafe {
            telamon_home_top(saved.as_ptr(), saved.len(), 5, top.as_mut_ptr(), top.len())
        };
        assert_eq!(&top[..n], b"3\tfile:///d\n");
        let key = call(telamon_home_key, b"file:///home/me/Docs/\0/home/me");
        assert_eq!(key, b"file:///home/me/Docs");
        assert!(call(telamon_home_key, b"file:///home/me\0/home/me").is_empty());
        assert!(call(telamon_home_key, b"file:///x").is_empty());
        let mut gone = vec![0u8; 64];
        // SAFETY: buffers as long as promised.
        let n = unsafe {
            telamon_home_forget(
                saved.as_ptr(),
                saved.len(),
                b"file:///d".as_ptr(),
                9,
                gone.as_mut_ptr(),
                gone.len(),
            )
        };
        assert_eq!(n, 0);
        // SAFETY: literals.
        assert!(unsafe { telamon_home_is_home(b"home:/".as_ptr(), 6) });
        assert!(!unsafe { telamon_home_is_home(b"file:///".as_ptr(), 8) });
        assert_eq!(telamon_home_limit(0), home::MAX_FOLDERS);
        assert_eq!(telamon_home_limit(9), 0);
    }

    #[test]
    fn recent_files_are_the_newest_that_still_exist() {
        let dir = std::env::temp_dir().join(format!("telamon-home-recent-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sub")).unwrap();
        for f in ["a.txt", "b b.txt", "c.txt"] {
            fs::write(dir.join(f), "x").unwrap();
        }
        let d = dir.display();
        let xbel = format!(
            "<?xml version=\"1.0\"?>\n<xbel version=\"1.0\">\n\
             <bookmark href=\"file://{d}/a.txt\" modified=\"2026-01-01T10:00:00Z\"/>\n\
             <bookmark href=\"file://{d}/b%20b.txt\" modified=\"2026-03-01T10:00:00Z\"/>\n\
             <bookmark href=\"file://{d}/gone.txt\" modified=\"2026-04-01T10:00:00Z\"/>\n\
             <bookmark href=\"file://{d}/sub\" modified=\"2026-05-01T10:00:00Z\"/>\n\
             <bookmark href=\"file://{d}/c.txt\" modified=\"2026-02-01T10:00:00Z\"/>\n\
             <bookmark href=\"file://{d}/c.txt\" modified=\"2025-02-01T10:00:00Z\"/>\n\
             <bookmark href=\"http://example.org/remote.txt\" modified=\"2026-06-01T10:00:00Z\"/>\n\
             </xbel>\n"
        );
        let p = dir.join("recently-used.xbel");
        fs::write(&p, xbel).unwrap();
        let s = recent_files(&p, 10);
        let urls: Vec<&str> = s.lines().map(|l| l.split('\t').nth(1).unwrap()).collect();
        // newest first; the missing file, the folder, the web address and the repeat are left out
        assert_eq!(
            urls,
            [
                format!("file://{d}/b%20b.txt"),
                format!("file://{d}/c.txt"),
                format!("file://{d}/a.txt"),
            ]
        );
        assert_eq!(recent_files(&p, 2).lines().count(), 2);
        assert_eq!(recent_files(&p, 0), "");
        assert_eq!(recent_files(&dir.join("missing.xbel"), 5), "");
        fs::remove_dir_all(&dir).unwrap();
    }
}
