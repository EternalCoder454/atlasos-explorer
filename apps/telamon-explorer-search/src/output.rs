//! Result output: names as display names, and JSON written by hand (strings
//! escaped per RFC 8259; control characters and the line separators as \u escapes).

use atlas_explorer_core::display_name;
use atlas_file_index::uri::percent_decode;
use std::io::{self, Write};

type Hit = (String, String, String, String, String, i64, u64, f64);

/// JSON string literal for `s`.
pub fn json_string(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 || c == '\u{7F}' || c == '\u{2028}' || c == '\u{2029}' => {
                o.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// A JSON number for a score (JSON has no NaN or infinity).
fn json_f64(x: f64) -> String {
    if x.is_finite() {
        format!("{x}")
    } else {
        "0".to_string()
    }
}

/// The path of a `file://` URI as display text.
fn display_path(uri: &str) -> String {
    match uri
        .strip_prefix("file://")
        .and_then(|p| percent_decode(p.as_bytes()))
    {
        Some(bytes) => display_name(&bytes[..]),
        None => display_name(uri),
    }
}

pub fn write(out: &mut impl Write, hits: &[Hit], json: bool) -> io::Result<()> {
    if json {
        out.write_all(b"[")?;
        for (i, (uri, name, kind, mime, icon, mtime, size, score)) in hits.iter().enumerate() {
            if i > 0 {
                out.write_all(b",")?;
            }
            write!(
                out,
                "\n  {{\"uri\":{},\"name\":{},\"kind\":{},\"mime\":{},\"icon\":{},\"mtime\":{},\"size\":{},\"score\":{}}}",
                json_string(uri),
                json_string(&display_name(name.as_str())),
                json_string(kind),
                json_string(mime),
                json_string(icon),
                mtime,
                size,
                json_f64(*score)
            )?;
        }
        out.write_all(if hits.is_empty() { b"]\n" } else { b"\n]\n" })?;
    } else {
        for (uri, name, kind, ..) in hits {
            let slash = if kind == "folder" { "/" } else { "" };
            // the name again passes through display_name: the service's is trusted
            // no more than any other reply on the bus
            writeln!(
                out,
                "{}{}\t{}",
                display_name(name.as_str()),
                slash,
                display_path(uri)
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(uri: &str, name: &str, kind: &str) -> Hit {
        (
            uri.into(),
            name.into(),
            kind.into(),
            "text/plain".into(),
            "text-plain".into(),
            5,
            6,
            7.5,
        )
    }

    #[test]
    fn json_escapes() {
        assert_eq!(
            json_string("a\"b\\c\n\u{1}\u{7f}\u{2028}é"),
            "\"a\\\"b\\\\c\\n\\u0001\\u007f\\u2028é\""
        );
        assert_eq!(json_f64(f64::NAN), "0");
        assert_eq!(json_f64(2.5), "2.5");
    }

    #[test]
    fn json_output_parses_as_expected() {
        let mut out = Vec::new();
        write(
            &mut out,
            &[hit("file:///a%0Ab", "x\u{202E}y\"", "file")],
            true,
        )
        .unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.starts_with("[\n  {\"uri\":\"file:///a%0Ab\""), "{s}");
        assert!(s.contains("\"name\":\"x⟨U+202E⟩y\\\"\""), "{s}");
        assert!(s.ends_with("}\n]\n"));
        let mut out = Vec::new();
        write(&mut out, &[], true).unwrap();
        assert_eq!(out, b"[]\n");
    }

    #[test]
    fn text_output_shows_display_names() {
        let mut out = Vec::new();
        write(
            &mut out,
            &[
                hit("file:///h/a%0Ab%FF", "a\nb", "file"),
                hit("file:///h/d", "d", "folder"),
            ],
            false,
        )
        .unwrap();
        let s = String::from_utf8(out).unwrap();
        assert_eq!(s, "a\u{240A}b\t/h/a\u{240A}b\\xFF\nd/\t/h/d\n");
    }
}
