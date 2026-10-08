//! Display names. File names are untrusted: they can hold control characters
//! (a newline hides the rest of a line), bidi controls (which reorder the text
//! around them, so `report\u{202E}fdp.exe` reads as `reportexe.pdf`) and bytes
//! that are not UTF-8. [`display_name`] returns text that is safe to show: every
//! such character is made visible, and the length is capped. See
//! docs/DESIGN.md, "Trust".

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// Characters shown of a name (a marker or `\xNN` counts as the characters it
/// is made of); the rest is replaced by one `…`.
pub const MAX_DISPLAY_CHARS: usize = 255;

/// Anything that is a file name as bytes: `OsStr`, `Path`, `[u8]`, `str` and
/// their owned forms.
pub trait NameBytes {
    fn name_bytes(&self) -> &[u8];
}

impl NameBytes for [u8] {
    fn name_bytes(&self) -> &[u8] {
        self
    }
}
impl NameBytes for Vec<u8> {
    fn name_bytes(&self) -> &[u8] {
        self
    }
}
impl NameBytes for str {
    fn name_bytes(&self) -> &[u8] {
        self.as_bytes()
    }
}
impl NameBytes for String {
    fn name_bytes(&self) -> &[u8] {
        self.as_bytes()
    }
}
#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::ffi::OsStrExt;

    impl NameBytes for OsStr {
        fn name_bytes(&self) -> &[u8] {
            self.as_bytes()
        }
    }
    impl NameBytes for OsString {
        fn name_bytes(&self) -> &[u8] {
            self.as_bytes()
        }
    }
    impl NameBytes for Path {
        fn name_bytes(&self) -> &[u8] {
            self.as_os_str().as_bytes()
        }
    }
    impl NameBytes for PathBuf {
        fn name_bytes(&self) -> &[u8] {
            self.as_os_str().as_bytes()
        }
    }
}

/// True for the characters that reorder or isolate text (U+202A to U+202E,
/// U+2066 to U+2069, U+200E, U+200F, U+061C), the line and paragraph
/// separators (U+2028, U+2029), and the ones that show nothing or look like
/// something else: format characters (zero-width U+200B to U+200D, U+2060 to
/// U+2064, U+FEFF, soft hyphen U+00AD, U+034F, U+180E, U+206A to U+206F,
/// U+FFF9 to U+FFFB), tag characters (U+E0000 to U+E007F), variation
/// selectors (U+FE00 to U+FE0F, U+E0100 to U+E01EF) and blank fillers (U+3164,
/// U+115F, U+1160, U+FFA0, U+2800).
fn is_marked(c: char) -> bool {
    matches!(c,
        '\u{202A}'..='\u{202E}'
        | '\u{2066}'..='\u{2069}'
        | '\u{200E}' | '\u{200F}' | '\u{061C}'
        | '\u{2028}' | '\u{2029}'
        | '\u{200B}'..='\u{200D}'
        | '\u{2060}'..='\u{2064}'
        | '\u{206A}'..='\u{206F}'
        | '\u{FEFF}' | '\u{00AD}' | '\u{034F}' | '\u{180E}'
        | '\u{FFF9}'..='\u{FFFB}'
        | '\u{E0000}'..='\u{E007F}'
        | '\u{FE00}'..='\u{FE0F}'
        | '\u{E0100}'..='\u{E01EF}'
        | '\u{3164}' | '\u{115F}' | '\u{1160}' | '\u{FFA0}' | '\u{2800}')
}

/// No-break and width spaces (U+00A0, U+2000 to U+200A): shown as they are,
/// but only one in a row, so a run of them cannot push the extension out of sight.
fn is_wide_space(c: char) -> bool {
    matches!(c, '\u{00A0}' | '\u{2000}'..='\u{200A}')
}

fn push_marker(out: &mut String, c: char) {
    use std::fmt::Write;
    // `⟨U+202E⟩`: visible, and not text the bidi algorithm reorders
    let _ = write!(out, "\u{27E8}U+{:04X}\u{27E9}", c as u32);
}

/// Characters `push_char` adds for `c`.
fn shown_len(c: char, in_space_run: bool) -> usize {
    match c {
        '\u{80}'..='\u{9F}' => 8,
        c if is_marked(c) || (in_space_run && is_wide_space(c)) => 8,
        _ => 1,
    }
}

fn push_char(out: &mut String, c: char, in_space_run: bool) {
    match c {
        // C0 controls become their control pictures: U+2400 + code (newline is U+240A)
        '\u{0}'..='\u{1F}' => out.push(char::from_u32(0x2400 + c as u32).unwrap_or('\u{FFFD}')),
        '\u{7F}' => out.push('\u{2421}'),
        // C1 controls have no control pictures
        '\u{80}'..='\u{9F}' => push_marker(out, c),
        c if is_marked(c) || (in_space_run && is_wide_space(c)) => push_marker(out, c),
        c => out.push(c),
    }
}

/// Adds `c` to `out` as it may be shown: a control, bidi or invisible
/// character comes out as a visible marker (for text that is not a name, such
/// as a file's content in Quick Look; a line break is the caller's to keep).
pub(crate) fn push_visible(out: &mut String, c: char) {
    push_char(out, c, false);
}

/// The text to show for a file name. Never longer than
/// [`MAX_DISPLAY_CHARS`] characters plus a `…`, and holds no control or bidi
/// character. Invalid UTF-8 bytes appear as `\xNN`.
pub fn display_name<N: NameBytes + ?Sized>(name: &N) -> String {
    let mut out = String::new();
    let mut shown = 0usize;
    let mut rest = name.name_bytes();
    // Bytes beyond what can ever be shown are not looked at.
    let limit_bytes = MAX_DISPLAY_CHARS * 4 * 4 + 64;
    let mut truncated = false;
    let mut prev_space = false;
    if rest.len() > limit_bytes {
        rest = &rest[..limit_bytes];
        truncated = true;
    }
    'outer: while !rest.is_empty() {
        let (valid, bad) = match std::str::from_utf8(rest) {
            Ok(s) => (s, 0),
            Err(e) => {
                let n = e.valid_up_to();
                // SAFETY-free: the prefix is valid by from_utf8's contract
                let s = std::str::from_utf8(&rest[..n]).unwrap_or("");
                let bad = e.error_len().unwrap_or(rest.len() - n);
                (s, bad)
            }
        };
        for c in valid.chars() {
            let n = shown_len(c, prev_space);
            if shown + n > MAX_DISPLAY_CHARS {
                truncated = true;
                break 'outer;
            }
            push_char(&mut out, c, prev_space);
            prev_space = is_wide_space(c);
            shown += n;
        }
        rest = &rest[valid.len()..];
        for b in &rest[..bad] {
            if shown + 4 > MAX_DISPLAY_CHARS {
                truncated = true;
                break 'outer;
            }
            out.push_str(&format!("\\x{b:02X}"));
            shown += 4;
        }
        rest = &rest[bad..];
    }
    if truncated {
        out.push('\u{2026}');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_names_are_unchanged() {
        assert_eq!(display_name("Résumé 2024.pdf"), "Résumé 2024.pdf");
        assert_eq!(display_name(""), "");
    }

    #[test]
    fn c0_controls_and_del_become_control_pictures() {
        for b in 0u8..0x20 {
            let s = display_name(&[b][..]);
            assert_eq!(s.chars().count(), 1);
            assert_eq!(
                s.chars().next().map(|c| c as u32),
                Some(0x2400 + u32::from(b))
            );
        }
        assert_eq!(display_name("a\nb"), "a\u{240A}b");
        assert_eq!(display_name("\u{7F}"), "\u{2421}");
    }

    #[test]
    fn c1_controls_are_marked() {
        for c in '\u{80}'..='\u{9F}' {
            let s = display_name(&c.to_string());
            assert!(s.contains("U+00"), "{s}");
            assert!(!s.chars().any(|x| x.is_control()), "{s}");
        }
    }

    #[test]
    fn invisible_and_blank_characters_are_marked() {
        let all = [
            '\u{200B}',
            '\u{200C}',
            '\u{200D}',
            '\u{2060}',
            '\u{2064}',
            '\u{FEFF}',
            '\u{00AD}',
            '\u{034F}',
            '\u{180E}',
            '\u{206A}',
            '\u{206F}',
            '\u{FFF9}',
            '\u{FFFB}',
            '\u{E0001}',
            '\u{E007F}',
            '\u{FE00}',
            '\u{FE0F}',
            '\u{E0100}',
            '\u{E01EF}',
            '\u{3164}',
            '\u{115F}',
            '\u{1160}',
            '\u{FFA0}',
            '\u{2800}',
        ];
        for c in all {
            let s = display_name(&format!("a{c}b"));
            assert!(!s.contains(c), "{:04X} survived", c as u32);
            assert!(s.contains(&format!("U+{:04X}", c as u32)), "{s}");
        }
        // ordinary spaces and one no-break space are left alone
        assert_eq!(display_name("a b\u{A0}c"), "a b\u{A0}c");
        // a run of them is marked after the first
        let s = display_name("a\u{A0}\u{2003}\u{2003}.exe");
        assert!(s.starts_with("a\u{A0}\u{27E8}U+2003"), "{s}");
        assert!(s.ends_with(".exe"));
    }

    #[test]
    fn every_bidi_char_is_marked() {
        let all = [
            '\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}', '\u{202E}', '\u{2066}', '\u{2067}',
            '\u{2068}', '\u{2069}', '\u{200E}', '\u{200F}', '\u{061C}',
        ];
        for c in all {
            let s = display_name(&format!("a{c}b"));
            assert!(!s.contains(c), "{:04X} survived", c as u32);
            assert!(s.contains(&format!("U+{:04X}", c as u32)), "{s}");
        }
        let spoof = display_name("report\u{202E}fdp.exe");
        assert!(!spoof.contains('\u{202E}'));
    }

    #[test]
    fn invalid_utf8_becomes_hex() {
        assert_eq!(display_name(&b"a\xFFb"[..]), "a\\xFFb");
        assert_eq!(display_name(&b"\xC3"[..]), "\\xC3");
        assert_eq!(display_name(&b"ok\xE2\x82"[..]), "ok\\xE2\\x82");
        // valid multi-byte text next to invalid bytes
        assert_eq!(display_name(&b"\xC3\xA9\x80"[..]), "\u{e9}\\x80");
    }

    #[test]
    fn length_is_capped() {
        let long = "x".repeat(10_000);
        let s = display_name(long.as_str());
        assert_eq!(s.chars().count(), MAX_DISPLAY_CHARS + 1);
        assert!(s.ends_with('\u{2026}'));
        let exact = "y".repeat(MAX_DISPLAY_CHARS);
        assert_eq!(display_name(exact.as_str()), exact);
        let junk = vec![0xFFu8; 100_000];
        assert!(display_name(&junk[..]).chars().count() <= MAX_DISPLAY_CHARS + 1);
        let marks = "\u{202E}".repeat(5000);
        assert!(display_name(marks.as_str()).chars().count() <= MAX_DISPLAY_CHARS + 1);
    }

    #[test]
    fn os_str_and_path_work() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        assert_eq!(display_name(OsStr::from_bytes(b"a\xFF")), "a\\xFF");
        assert_eq!(display_name(std::path::Path::new("p")), "p");
    }
}
