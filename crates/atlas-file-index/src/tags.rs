//! File tags, the freedesktop/Baloo convention: the extended attribute
//! `user.xdg.tags` holds a UTF-8 comma-separated list of names (`Red,Work,Taxes
//! 2025`). This module is the one place that reads and cleans that value, so
//! the index, the snapshot check and the live walk all agree on what a tag is.
//!
//! A cleaned value is the names joined by `,`: split on `,`, whitespace
//! trimmed, empty names and names with control characters dropped, names over
//! [`MAX_NAME_CHARS`] characters dropped, repeats (ignoring case and accents,
//! as the search folds text) dropped keeping the first, and at most
//! [`MAX_TAGS`] names. Invalid UTF-8 is read lossily. Cleaning twice changes
//! nothing, which is how [`is_clean`] checks a value from a snapshot.

use crate::sys;
use crate::text::fold;
use std::collections::HashSet;
use std::ffi::CStr;
use std::path::Path;

/// The attribute that holds the tags.
pub const XATTR: &CStr = c"user.xdg.tags";
/// Bytes of the attribute that are read; a longer value is treated as having no tags.
pub const MAX_READ: usize = 4096;
/// Longest tag name, in characters.
pub const MAX_NAME_CHARS: usize = 64;
/// Most tags kept per file or folder.
pub const MAX_TAGS: usize = 32;

/// The raw `user.xdg.tags` value of `path`, never following a symlink. Any
/// failure (no attribute, no xattr support, no permission, a value over
/// [`MAX_READ`] bytes) is an empty value: a file without tags.
pub fn read_raw(path: &Path) -> Vec<u8> {
    let mut buf = [0u8; MAX_READ];
    match sys::lgetxattr(path, XATTR, &mut buf) {
        Ok(n) => buf[..n].to_vec(),
        Err(_) => Vec::new(),
    }
}

/// Clean a raw attribute value into the stored form (see the module docs);
/// empty when the value holds no usable tag.
pub fn clean(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = String::new();
    let mut n = 0;
    for part in text.split(',') {
        let name = part.trim();
        if name.is_empty()
            // controls and the characters that reorder text: a tag is shown
            // to the person, and the names of other people's files carry them
            || name.chars().any(atlas_explorer_core::launch::is_hidden_char)
            || name.chars().count() > MAX_NAME_CHARS
        {
            continue;
        }
        if !seen.insert(fold(name)) {
            continue;
        }
        if n > 0 {
            out.push(',');
        }
        out.push_str(name);
        n += 1;
        if n == MAX_TAGS {
            break;
        }
    }
    out
}

/// Is `text` exactly what [`clean`] would store, and not empty? (A snapshot's
/// tag text is untrusted: anything else in it is refused.)
pub fn is_clean(text: &str) -> bool {
    !text.is_empty() && clean(text.as_bytes()) == text
}

/// The names in a cleaned value.
pub fn names(clean: &str) -> impl Iterator<Item = &str> {
    clean.split(',').filter(|n| !n.is_empty())
}

/// The form a tag is compared in: folded like names are (case and accents
/// ignored) and trimmed.
pub fn folded(tag: &str) -> String {
    fold(tag.trim())
}

/// Does the cleaned value `clean` hold the tag whose [`folded`] form is `wanted`?
pub fn contains(clean: &str, wanted: &str) -> bool {
    !wanted.is_empty() && names(clean).any(|n| fold(n) == wanted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_with_direction_or_control_characters_are_dropped() {
        assert_eq!(clean("a\u{202E}b,ok,x\u{2066}y,\u{200F}z,li\nne".as_bytes()), "ok");
    }

    #[test]
    fn splits_and_trims() {
        assert_eq!(clean(b"Red,Work,Taxes 2025"), "Red,Work,Taxes 2025");
        assert_eq!(
            clean(b"  Red ,\tWork\n, Taxes 2025  "),
            "Red,Work,Taxes 2025"
        );
        assert_eq!(clean(b"one"), "one");
        assert_eq!(clean("Zoë,Ünï".as_bytes()), "Zoë,Ünï");
    }

    #[test]
    fn empties_are_dropped() {
        assert_eq!(clean(b""), "");
        assert_eq!(clean(b",,, ,\t,"), "");
        assert_eq!(clean(b",Red,,Blue,"), "Red,Blue");
    }

    #[test]
    fn control_characters_drop_the_name() {
        assert_eq!(clean(b"Red,Bl\x01ue,Green"), "Red,Green");
        assert_eq!(clean(b"Red,Bl\nue,Green"), "Red,Green"); // inside the name
        assert_eq!(clean(b"a\x00b,c"), "c");
        assert_eq!(clean("x\u{85}y,z".as_bytes()), "z"); // C1 control
        // controls at the ends are whitespace and trimmed
        assert_eq!(clean(b"\nRed\n"), "Red");
    }

    #[test]
    fn long_names_are_dropped() {
        let ok = "a".repeat(MAX_NAME_CHARS);
        let long = "b".repeat(MAX_NAME_CHARS + 1);
        assert_eq!(
            clean(format!("{ok},{long},c").as_bytes()),
            format!("{ok},c")
        );
        // counted in characters, not bytes
        let wide = "é".repeat(MAX_NAME_CHARS);
        assert_eq!(clean(wide.as_bytes()), wide);
        let wide = "é".repeat(MAX_NAME_CHARS + 1);
        assert_eq!(clean(wide.as_bytes()), "");
    }

    #[test]
    fn repeats_are_dropped_ignoring_case_keeping_the_first() {
        assert_eq!(clean(b"Red,red,RED,Blue,blue"), "Red,Blue");
        assert_eq!(clean(b"red,Red"), "red");
        assert_eq!(clean("Café,cafe,CAFÉ".as_bytes()), "Café");
        // a dropped name does not use up a place
        assert_eq!(clean(b"a,A,b"), "a,b");
    }

    #[test]
    fn at_most_32_tags() {
        let many: Vec<String> = (0..50).map(|i| format!("t{i}")).collect();
        let c = clean(many.join(",").as_bytes());
        let got: Vec<&str> = names(&c).collect();
        assert_eq!(got.len(), MAX_TAGS);
        assert_eq!(got[0], "t0");
        assert_eq!(got[MAX_TAGS - 1], "t31");
    }

    #[test]
    fn invalid_utf8_is_read_lossily() {
        assert_eq!(clean(b"Re\xFFd,Blue"), "Re\u{FFFD}d,Blue");
        assert_eq!(clean(b"\xFF,\xFE"), "\u{FFFD}");
        // the replacement text is itself clean
        assert!(is_clean(&clean(b"\xC3\x28,ok")));
    }

    #[test]
    fn compare_is_case_insensitive() {
        let c = clean(b"Red,Taxes 2025,Zo\xC3\xAB");
        assert!(contains(&c, &folded("red")));
        assert!(contains(&c, &folded("  RED ")));
        assert!(contains(&c, &folded("taxes 2025")));
        assert!(contains(&c, &folded("ZOE")));
        assert!(!contains(&c, &folded("Re")));
        assert!(!contains(&c, &folded("Blue")));
        assert!(!contains(&c, &folded("")));
        assert!(!contains("", &folded("red")));
    }

    #[test]
    fn is_clean_accepts_only_stored_form() {
        assert!(is_clean("Red,Blue"));
        assert!(!is_clean(""));
        assert!(!is_clean("Red,,Blue"));
        assert!(!is_clean(" Red"));
        assert!(!is_clean("Red,red"));
        assert!(!is_clean("Re\nd"));
        assert!(!is_clean(&"a".repeat(MAX_NAME_CHARS + 1)));
        let many: Vec<String> = (0..MAX_TAGS + 1).map(|i| format!("t{i}")).collect();
        assert!(!is_clean(&many.join(",")));
    }

    #[test]
    fn clean_is_idempotent() {
        for raw in [
            &b"Red, red ,\x01,Blue,,  x  "[..],
            "ÅNGSTRÖM,angstrom,Zoë".as_bytes(),
            b"\xFF\xFE,a",
        ] {
            let once = clean(raw);
            assert_eq!(clean(once.as_bytes()), once);
        }
    }
}
