//! Rename and new-folder names: what is refused, what only deserves a
//! warning, and the "Keep both" name for a conflict.

use crate::launch::is_hidden_char;

/// Longest file name the filesystems we write to accept, in bytes.
pub const MAX_NAME_BYTES: usize = 255;
/// Candidates `keep_both_name` tries before giving up.
const MAX_TRIES: u32 = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Invalid {
    Empty,
    Slash,
    Nul,
    DotOrDotDot,
    TooLong,
}

impl Invalid {
    pub fn describe(self) -> &'static str {
        match self {
            Invalid::Empty => "The name can't be empty.",
            Invalid::Slash => "A name can't contain a slash.",
            Invalid::Nul => "A name can't contain a null character.",
            Invalid::DotOrDotDot => "\".\" and \"..\" are not names.",
            Invalid::TooLong => "The name is longer than 255 bytes.",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Warning {
    LeadingWhitespace,
    TrailingWhitespace,
    HiddenCharacters,
    LeadingDash,
    TrailingDot,
}

impl Warning {
    pub fn describe(self) -> &'static str {
        match self {
            Warning::LeadingWhitespace => "The name starts with a space.",
            Warning::TrailingWhitespace => "The name ends with a space.",
            Warning::HiddenCharacters => "The name has control or text-direction characters.",
            Warning::LeadingDash => {
                "A name starting with \"-\" is easy to mistake for an option in a terminal."
            }
            Warning::TrailingDot => "A name ending with \".\" can be a problem on other systems.",
        }
    }
}

/// Checks a typed name. `Err` means it can't be used; `Ok` lists what to
/// warn about (empty when the name is fine), each warning at most once.
pub fn validate(name: &str) -> Result<Vec<Warning>, Invalid> {
    if name.is_empty() {
        return Err(Invalid::Empty);
    }
    if name.len() > MAX_NAME_BYTES {
        return Err(Invalid::TooLong);
    }
    if name.contains('/') {
        return Err(Invalid::Slash);
    }
    if name.contains('\0') {
        return Err(Invalid::Nul);
    }
    if name == "." || name == ".." {
        return Err(Invalid::DotOrDotDot);
    }
    let mut w = Vec::new();
    if name.starts_with(char::is_whitespace) {
        w.push(Warning::LeadingWhitespace);
    }
    if name.ends_with(char::is_whitespace) {
        w.push(Warning::TrailingWhitespace);
    }
    if name.chars().any(is_hidden_char) {
        w.push(Warning::HiddenCharacters);
    }
    if name.starts_with('-') {
        w.push(Warning::LeadingDash);
    }
    if name.ends_with('.') {
        w.push(Warning::TrailingDot);
    }
    Ok(w)
}

/// Splits `name` into stem and extension (with its dot). A leading dot
/// belongs to the stem (`.bashrc` has no extension); `.tar.gz` and its kin
/// count as one extension.
pub(crate) fn split_ext(name: &str) -> (&str, &str) {
    let Some(dot) = name.rfind('.') else {
        return (name, "");
    };
    let ext = &name[dot..];
    if dot == 0 || ext.len() == 1 || ext.len() > 13 || ext.contains(char::is_whitespace) {
        return (name, "");
    }
    let compressed = ["gz", "bz2", "xz", "zst", "lz4", "lz", "lzma", "z", "br"]
        .iter()
        .any(|c| ext[1..].eq_ignore_ascii_case(c));
    if compressed {
        let stem = &name[..dot];
        // Bytes, not a str slice: four bytes back may fall inside a character.
        // ".tar" is ASCII, so a match starts on a character boundary.
        let bytes = stem.as_bytes();
        if bytes.len() > 4 && bytes[bytes.len() - 4..].eq_ignore_ascii_case(b".tar") {
            let cut = bytes.len() - 4;
            return (&name[..cut], &name[cut..]);
        }
    }
    (&name[..dot], ext)
}

/// How many bytes of `name` an in-place rename selects first: all of a
/// folder's name, and a file's name without its extension (`report` of
/// `report.tar.gz`; a leading dot is not an extension).
pub fn stem_len(name: &str, is_dir: bool) -> usize {
    if is_dir {
        name.len()
    } else {
        split_ext(name).0.len()
    }
}

/// A name for the copy that keeps both: "Report (2).pdf", then "(3)" and so
/// on, the first one `exists` says is free. Fits 255 bytes by trimming the
/// stem. After 1000 tries it returns the last candidate anyway, and the
/// caller's own check (the copy itself) then fails in plain words.
pub fn keep_both_name(name: &str, exists: impl Fn(&str) -> bool) -> String {
    let (stem, ext) = split_ext(name);
    let mut last = String::new();
    for n in 2..2 + MAX_TRIES {
        let suffix = format!(" ({n})");
        let room = MAX_NAME_BYTES.saturating_sub(suffix.len() + ext.len());
        let mut cut = stem.len().min(room);
        while !stem.is_char_boundary(cut) {
            cut -= 1;
        }
        let candidate = format!("{}{suffix}{ext}", &stem[..cut]);
        if !exists(&candidate) {
            return candidate;
        }
        last = candidate;
    }
    last
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_names() {
        assert_eq!(validate(""), Err(Invalid::Empty));
        assert_eq!(validate("a/b"), Err(Invalid::Slash));
        assert_eq!(validate("a\0b"), Err(Invalid::Nul));
        assert_eq!(validate("."), Err(Invalid::DotOrDotDot));
        assert_eq!(validate(".."), Err(Invalid::DotOrDotDot));
        assert_eq!(validate(&"a".repeat(256)), Err(Invalid::TooLong));
        assert_eq!(validate(&"a".repeat(255)), Ok(vec![]));
        // Bytes, not characters.
        assert_eq!(validate(&"é".repeat(128)), Err(Invalid::TooLong));
        assert_eq!(validate("..."), Ok(vec![Warning::TrailingDot]));
    }

    #[test]
    fn warnings() {
        assert_eq!(validate("ok.txt"), Ok(vec![]));
        assert_eq!(
            validate(" a "),
            Ok(vec![
                Warning::LeadingWhitespace,
                Warning::TrailingWhitespace
            ])
        );
        assert_eq!(validate("a\u{202E}b"), Ok(vec![Warning::HiddenCharacters]));
        assert_eq!(validate("a\tb"), Ok(vec![Warning::HiddenCharacters]));
        assert_eq!(validate("-rf"), Ok(vec![Warning::LeadingDash]));
        assert_eq!(validate("a."), Ok(vec![Warning::TrailingDot]));
    }

    fn free_after(taken: &'static [&'static str]) -> impl Fn(&str) -> bool {
        move |n| taken.contains(&n)
    }

    #[test]
    fn the_stem_is_what_an_in_place_rename_selects() {
        assert_eq!(stem_len("report.pdf", false), 6);
        assert_eq!(stem_len("a.tar.gz", false), 1);
        assert_eq!(stem_len(".bashrc", false), 7);
        assert_eq!(stem_len("Makefile", false), 8);
        assert_eq!(stem_len("v1.2", true), 4);
        assert_eq!(stem_len("é.txt", false), 2, "bytes, not characters");
    }

    #[test]
    fn keep_both_counts_up() {
        assert_eq!(
            keep_both_name("Report.pdf", free_after(&[])),
            "Report (2).pdf"
        );
        let taken = &["Report (2).pdf", "Report (3).pdf"];
        assert_eq!(
            keep_both_name("Report.pdf", free_after(taken)),
            "Report (4).pdf"
        );
        assert_eq!(keep_both_name("Makefile", free_after(&[])), "Makefile (2)");
    }

    #[test]
    fn keep_both_extensions() {
        assert_eq!(keep_both_name("a.tar.gz", free_after(&[])), "a (2).tar.gz");
        assert_eq!(keep_both_name("a.b.txt", free_after(&[])), "a.b (2).txt");
        assert_eq!(keep_both_name(".bashrc", free_after(&[])), ".bashrc (2)");
        assert_eq!(keep_both_name("x.", free_after(&[])), "x. (2)");
        assert_eq!(keep_both_name(".tar.gz", free_after(&[])), ".tar (2).gz");
    }

    #[test]
    fn keep_both_fits_255_bytes_on_a_char_boundary() {
        let name = format!("{}.txt", "é".repeat(125));
        assert!(name.len() <= 255);
        let out = keep_both_name(&name, free_after(&[]));
        assert!(out.len() <= 255, "{}", out.len());
        assert!(out.ends_with(" (2).txt"));
        assert!(validate(&out).is_ok());
        let long = "a".repeat(255);
        let out = keep_both_name(&long, free_after(&[]));
        assert_eq!(out.len(), 255);
        assert!(out.ends_with(" (2)"));
    }

    #[test]
    fn split_ext_survives_multibyte_names() {
        // Four bytes back from the dot used to land inside a character.
        for name in ["日本.gz", "façade.gz", "日本語.xz", "a日.tar.gz", "é.gz", "日.gz"] {
            let (stem, ext) = split_ext(name);
            assert_eq!(format!("{stem}{ext}"), name);
            assert!(name.is_char_boundary(stem.len()), "{name}");
        }
        assert_eq!(split_ext("日本.gz"), ("日本", ".gz"));
        assert_eq!(split_ext("a日.tar.gz"), ("a日", ".tar.gz"));
        assert_eq!(split_ext("日本.TAR.GZ"), ("日本", ".TAR.GZ"));
        assert_eq!(stem_len("façade.gz", false), "façade".len());
        let _ = keep_both_name("日本語.xz", |_| false);
    }

    #[test]
    fn keep_both_is_bounded() {
        let calls = std::cell::Cell::new(0u32);
        let out = keep_both_name("a", |_| {
            calls.set(calls.get() + 1);
            true
        });
        assert_eq!(calls.get(), MAX_TRIES);
        assert_eq!(out, "a (1001)");
    }

    mod props {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn split_ext_never_panics_and_is_lossless(name in ".{0,64}") {
                let (stem, ext) = split_ext(&name);
                prop_assert_eq!(format!("{stem}{ext}"), name.clone());
                prop_assert!(name.is_char_boundary(stem.len()));
                let _ = stem_len(&name, false);
                let _ = stem_len(&name, true);
            }

            #[test]
            fn split_ext_survives_suffix_names(base in ".{0,12}", sfx in prop::sample::select(vec![".gz", ".tar.gz", ".xz", ".TAR.BZ2", ".zst", ".txt"])) {
                let name = format!("{base}{sfx}");
                let (stem, ext) = split_ext(&name);
                prop_assert_eq!(format!("{stem}{ext}"), name.clone());
                prop_assert!(name.is_char_boundary(stem.len()));
            }

            #[test]
            fn keep_both_never_panics_and_makes_a_name(name in ".{0,300}") {
                let out = keep_both_name(&name, |_| false);
                if validate(&name).is_ok() {
                    prop_assert!(out.len() <= MAX_NAME_BYTES);
                    prop_assert!(validate(&out).is_ok(), "{:?}", out);
                }
            }
        }
    }
}
