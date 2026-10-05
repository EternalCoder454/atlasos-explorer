//! Text folding and word boundaries for the matcher.
//!
//! Folding is NFKD, combining marks dropped, then lower case (`Résumé` and
//! `resume` fold alike). It works one character at a time, so a byte offset in
//! the folded name can be computed from the original name without folding the
//! whole of it (see [`word_starts_folded`]).

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

/// Longest folded name kept, in bytes.
pub const MAX_FOLD: usize = 4096;

fn fold_char_into(c: char, out: &mut String) {
    for d in std::iter::once(c).nfkd() {
        if is_combining_mark(d) {
            continue;
        }
        if d == '\u{DF}' {
            out.push_str("ss");
            continue;
        }
        for l in d.to_lowercase() {
            if !is_combining_mark(l) {
                out.push(l);
            }
        }
    }
}

/// Fold text: NFKD, marks dropped, lower case.
pub fn fold(s: &str) -> String {
    if s.is_ascii() {
        return s.to_ascii_lowercase();
    }
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        fold_char_into(c, &mut out);
    }
    out
}

/// Fold a name that may not be UTF-8 (invalid bytes fold as U+FFFD), capped at
/// [`MAX_FOLD`] bytes on a character boundary.
pub fn fold_bytes(name: &[u8]) -> Vec<u8> {
    let mut out = if name.is_ascii() {
        name.to_ascii_lowercase()
    } else {
        fold(&String::from_utf8_lossy(name)).into_bytes()
    };
    if out.len() > MAX_FOLD {
        let mut n = MAX_FOLD;
        while n > 0 && (out[n] & 0xC0) == 0x80 {
            n -= 1;
        }
        out.truncate(n);
    }
    out
}

/// Word separators inside a name.
#[inline]
pub fn is_sep(b: u8) -> bool {
    matches!(b, b' ' | b'-' | b'_' | b'.')
}

/// Is `cur` (with `prev` before it and `next` after it, 0 at the ends) the
/// start of a word? A word starts after a separator, at a lower-to-upper
/// camelCase step (`fooBar`), at the last capital of a run before a lower-case
/// letter (`HTMLParser`) and where letters meet digits (`report2024`).
#[inline]
fn is_word_start_ascii(prev: u8, cur: u8, next: u8, extra_sep: bool) -> bool {
    let sep = |b: u8| is_sep(b) || (extra_sep && b == b'/');
    if sep(cur) {
        return false;
    }
    if sep(prev) {
        return true;
    }
    (prev.is_ascii_lowercase() && cur.is_ascii_uppercase())
        || (prev.is_ascii_uppercase() && cur.is_ascii_uppercase() && next.is_ascii_lowercase())
        || (prev.is_ascii_alphabetic() && cur.is_ascii_digit())
}

/// Is byte `pos` of an ASCII name the start of a word? (`pos` is also the
/// offset in the folded name, as folding ASCII keeps lengths.)
pub fn is_word_start_at_ascii(raw: &[u8], pos: usize, path_mode: bool) -> bool {
    if pos >= raw.len() {
        return false;
    }
    let cur = raw[pos];
    if pos == 0 {
        return !(is_sep(cur) || (path_mode && cur == b'/'));
    }
    let next = raw.get(pos + 1).copied().unwrap_or(0);
    is_word_start_ascii(raw[pos - 1], cur, next, path_mode)
}

/// Offsets (in the folded text) where a word starts, with the folded first
/// character of that word, for a name that is not plain ASCII.
pub fn word_starts_folded(raw: &[u8], path_mode: bool) -> Vec<(usize, char)> {
    let s = String::from_utf8_lossy(raw);
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut off = 0usize;
    let mut buf = String::new();
    let sep = |c: char| c.is_ascii() && (is_sep(c as u8) || (path_mode && c == '/'));
    for (i, &c) in chars.iter().enumerate() {
        buf.clear();
        fold_char_into(c, &mut buf);
        let start = if sep(c) {
            false
        } else if i == 0 {
            true
        } else {
            let p = chars[i - 1];
            let n = chars.get(i + 1).copied().unwrap_or('\0');
            sep(p)
                || (p.is_lowercase() && c.is_uppercase())
                || (p.is_uppercase() && c.is_uppercase() && n.is_lowercase())
                || (p.is_alphabetic() && c.is_ascii_digit())
        };
        if start && let Some(first) = buf.chars().next() {
            out.push((off, first));
        }
        off += buf.len();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_diacritics_and_case() {
        assert_eq!(fold("Résumé"), "resume");
        assert_eq!(fold("ÅNGSTRÖM"), "angstrom");
        assert_eq!(fold("Straße"), "strasse");
        assert_eq!(fold("ﬁle"), "file");
        assert_eq!(fold("İstanbul"), "istanbul");
        assert_eq!(fold("plain"), "plain");
    }

    #[test]
    fn fold_bytes_handles_invalid_utf8_and_caps() {
        assert_eq!(fold_bytes(b"A\xFFb"), "a\u{FFFD}b".as_bytes());
        let long = "é".repeat(10_000);
        let f = fold_bytes(long.as_bytes());
        assert!(f.len() <= MAX_FOLD);
        assert!(std::str::from_utf8(&f).is_ok());
    }

    #[test]
    fn ascii_word_starts() {
        let raw = b"myBigReport_HTMLParser-2024.txt";
        let starts: Vec<usize> = (0..raw.len())
            .filter(|&i| is_word_start_at_ascii(raw, i, false))
            .collect();
        let words: Vec<&str> = starts
            .iter()
            .map(|&i| std::str::from_utf8(&raw[i..i + 1]).unwrap_or("?"))
            .collect();
        assert_eq!(words, ["m", "B", "R", "H", "P", "2", "t"]);
    }

    #[test]
    fn slow_path_agrees_with_ascii_path() {
        for name in [
            "myBigReport_HTMLParser-2024.txt",
            "Plain file.md",
            "a.b-c_d",
            "X",
        ] {
            let fast: Vec<(usize, char)> = (0..name.len())
                .filter(|&i| is_word_start_at_ascii(name.as_bytes(), i, false))
                .map(|i| (i, name.as_bytes()[i].to_ascii_lowercase() as char))
                .collect();
            assert_eq!(word_starts_folded(name.as_bytes(), false), fast, "{name}");
        }
    }

    #[test]
    fn slow_path_offsets_are_in_folded_text() {
        // "Éa Bc": folded "ea bc"; words start at 0 and 3
        let v = word_starts_folded("Éa Bc".as_bytes(), false);
        assert_eq!(v, vec![(0, 'e'), (3, 'b')]);
    }
}
