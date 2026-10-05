//! Last use of files, from `$XDG_DATA_HOME/recently-used.xbel` (the freedesktop
//! recent-files list). The file is read as untrusted text: size and entry
//! counts are capped, and anything that does not parse is skipped.

use crate::uri::uri_to_path;
use std::io::Read;
use std::path::Path;

const MAX_FILE: u64 = 16 * 1024 * 1024;
const MAX_ENTRIES: usize = 100_000;

/// `(absolute path, last use as seconds since the epoch)`.
pub fn load(path: &Path) -> Vec<(Vec<u8>, i64)> {
    let f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                log::warn!("recent files not read: {e}");
                log::debug!("recent files file: {}", path.display());
            }
            return Vec::new();
        }
    };
    let mut buf = Vec::new();
    if let Err(e) = f.take(MAX_FILE).read_to_end(&mut buf) {
        log::warn!("recent files not read: {e}");
        log::debug!("recent files file: {}", path.display());
        return Vec::new();
    }
    parse(&String::from_utf8_lossy(&buf))
}

pub fn parse(text: &str) -> Vec<(Vec<u8>, i64)> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(p) = rest.find("<bookmark ") {
        rest = &rest[p + 10..];
        let tag_end = rest.find('>').unwrap_or(rest.len());
        let tag = &rest[..tag_end];
        rest = &rest[tag_end..];
        let Some(href) = attr(tag, "href") else {
            continue;
        };
        let Some(path) = uri_to_path(&unescape(href)) else {
            continue;
        };
        let t = ["modified", "visited", "added"]
            .iter()
            .filter_map(|k| attr(tag, k).and_then(parse_time))
            .max();
        if let Some(t) = t {
            out.push((path, t));
            if out.len() >= MAX_ENTRIES {
                break;
            }
        }
    }
    out
}

/// Value of `name="..."` in a tag (the name must start the tag or follow a space).
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let pat = format!("{name}=\"");
    let mut from = 0;
    while let Some(i) = tag.get(from..)?.find(&pat) {
        let i = i + from;
        if i == 0 || tag.as_bytes()[i - 1].is_ascii_whitespace() {
            let start = i + pat.len();
            let end = tag[start..].find('"')? + start;
            return Some(&tag[start..end]);
        }
        from = i + 1;
    }
    None
}

fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// `2024-03-05T14:22:10Z`, optionally with a fraction.
fn parse_time(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let num = |a: usize, z: usize| s.get(a..z)?.parse::<i64>().ok();
    let (y, m, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (hh, mm, ss) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mm > 59 || ss > 60 {
        return None;
    }
    // days from civil (Howard Hinnant)
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hh * 3600 + mm * 60 + ss)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_times() {
        assert_eq!(parse_time("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_time("2000-03-01T00:00:00Z"), Some(951_868_800));
        assert_eq!(
            parse_time("2024-03-05T14:22:10.123456Z"),
            Some(1_709_648_530)
        );
        assert_eq!(parse_time("garbage"), None);
        assert_eq!(parse_time("2024-13-05T14:22:10Z"), None);
    }

    #[test]
    fn parses_bookmarks() {
        let x = r#"<?xml version="1.0"?>
<xbel version="1.0"><bookmark href="file:///home/a/My%20Doc.pdf" added="2024-01-01T00:00:00Z" modified="2024-03-05T14:22:10Z" visited="2024-02-01T00:00:00Z"><info/></bookmark>
<bookmark href="http://x/y" modified="2024-03-05T14:22:10Z"></bookmark>
<bookmark href="file:///b?x" modified="2024-03-05T14:22:10Z"/>
<bookmark nohref="1"/>
<bookmark href="file:///c&amp;d" visited="1999-01-01T00:00:00Z"/></xbel>"#;
        let v = parse(x);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0], (b"/home/a/My Doc.pdf".to_vec(), 1_709_648_530));
        assert_eq!(v[1].0, b"/c&d".to_vec());
    }

    #[test]
    fn survives_junk() {
        assert!(parse("<bookmark ").is_empty());
        assert!(parse("<bookmark href=\"").is_empty());
        assert!(parse("\u{0}<bookmark >").is_empty());
    }
}
