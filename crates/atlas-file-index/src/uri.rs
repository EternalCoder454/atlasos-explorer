//! `file://` URIs for byte paths. Every byte of a name survives the round trip
//! (percent-encoding is of bytes, not characters).

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;

fn unreserved(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/')
}

/// `file://` URI for an absolute path.
pub fn path_to_uri(path: &[u8]) -> String {
    let mut s = String::with_capacity(7 + path.len() + path.len() / 4);
    s.push_str("file://");
    for &b in path {
        if unreserved(b) {
            s.push(b as char);
        } else {
            s.push('%');
            s.push(char::from(b"0123456789ABCDEF"[usize::from(b >> 4)]));
            s.push(char::from(b"0123456789ABCDEF"[usize::from(b & 15)]));
        }
    }
    s
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Percent-decode; `None` on a malformed escape.
pub fn percent_decode(s: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'%' {
            let h = hex(*s.get(i + 1)?)?;
            let l = hex(*s.get(i + 2)?)?;
            out.push(h << 4 | l);
            i += 3;
        } else {
            out.push(s[i]);
            i += 1;
        }
    }
    Some(out)
}

/// The absolute path of a `file://` URI, normalised (no empty or `.` parts, no
/// trailing slash). Refused: other schemes, a host other than `localhost`, a
/// query or fragment, `..`, NUL, malformed escapes, over 4096 bytes.
pub fn uri_to_path(uri: &str) -> Option<Vec<u8>> {
    if uri.len() > 12_300 {
        return None;
    }
    let rest = uri.strip_prefix("file://")?;
    let path = if let Some(p) = rest.strip_prefix("localhost/") {
        &rest[rest.len() - p.len() - 1..]
    } else if rest.starts_with('/') {
        rest
    } else {
        return None;
    };
    if path.contains(['?', '#']) {
        return None;
    }
    let decoded = percent_decode(path.as_bytes())?;
    if decoded.contains(&0) || decoded.len() > 4096 {
        return None;
    }
    let mut out = Vec::with_capacity(decoded.len());
    for comp in decoded.split(|&b| b == b'/') {
        match comp {
            b"" | b"." => {}
            b".." => return None,
            c => {
                out.push(b'/');
                out.extend_from_slice(c);
            }
        }
    }
    if out.is_empty() {
        out.push(b'/');
    }
    Some(out)
}

pub fn uri_to_pathbuf(uri: &str) -> Option<PathBuf> {
    uri_to_path(uri).map(|b| PathBuf::from(OsString::from_vec(b)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_byte() {
        let all: Vec<u8> = (1u8..=255).collect();
        let mut p = b"/x/".to_vec();
        p.extend(all.iter().filter(|&&b| b != b'/'));
        let uri = path_to_uri(&p);
        assert!(uri.is_ascii());
        assert_eq!(uri_to_path(&uri), Some(p));
    }

    #[test]
    fn parses_and_refuses() {
        assert_eq!(
            uri_to_path("file:///home/a%20b/"),
            Some(b"/home/a b".to_vec())
        );
        assert_eq!(
            uri_to_path("file://localhost/x//y/./z"),
            Some(b"/x/y/z".to_vec())
        );
        assert_eq!(uri_to_path("file:///"), Some(b"/".to_vec()));
        assert_eq!(uri_to_path("http://x/y"), None);
        assert_eq!(uri_to_path("file://host/x"), None);
        assert_eq!(uri_to_path("file:///x/../y"), None);
        assert_eq!(uri_to_path("file:///x%00y"), None);
        assert_eq!(uri_to_path("file:///x%zz"), None);
        assert_eq!(uri_to_path("file:///x%4"), None);
        assert_eq!(uri_to_path("file:///x?y"), None);
        assert_eq!(uri_to_path("file:///x#y"), None);
        assert_eq!(uri_to_path("file:relative"), None);
        assert_eq!(uri_to_path(&format!("file:///{}", "a".repeat(5000))), None);
    }
}
