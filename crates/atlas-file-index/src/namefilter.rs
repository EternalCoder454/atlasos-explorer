//! The folder filter's matcher (Ctrl+F): which names of the folder shown
//! stay. Plain words are the index's own: every word must occur in the name,
//! case and accents ignored. With "Use pattern" the text is a regular
//! expression (`atlas_explorer_core::pattern`) matched against the name.
//! Compiled once per change of the text and tested once per name. See
//! docs/DESIGN.md, "Search".

use crate::query::NameMatcher;
use atlas_explorer_core::pattern::{Pattern, PatternError};

pub enum NameFilter {
    /// Nothing typed: every name stays.
    All,
    Words(NameMatcher),
    Pattern(Pattern),
}

impl NameFilter {
    /// The filter for the text typed. An invalid pattern is an error (the
    /// window then shows every item and says why).
    pub fn new(text: &str, pattern: bool) -> Result<NameFilter, PatternError> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(NameFilter::All);
        }
        if pattern {
            Pattern::new(text).map(NameFilter::Pattern)
        } else {
            Ok(NameFilter::Words(NameMatcher::new(text)))
        }
    }

    /// Does everything stay?
    pub fn is_all(&self) -> bool {
        matches!(self, NameFilter::All)
    }

    pub fn matches(&self, name: &[u8]) -> bool {
        match self {
            NameFilter::All => true,
            NameFilter::Words(m) => m.class(name).is_some(),
            NameFilter::Pattern(p) => p.is_match(name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(f: &NameFilter, all: &[&str]) -> Vec<String> {
        all.iter()
            .filter(|n| f.matches(n.as_bytes()))
            .map(|n| (*n).to_string())
            .collect()
    }

    const DIR: &[&str] = &[
        "Report 2024.pdf",
        "report-final.docx",
        "Résumé.odt",
        "IMG_0001.JPG",
        "IMG_0002.png",
        "notes.txt",
        ".hidden",
    ];

    #[test]
    fn nothing_typed_keeps_everything() {
        for text in ["", "   "] {
            let f = NameFilter::new(text, false).unwrap();
            assert!(f.is_all());
            assert_eq!(names(&f, DIR).len(), DIR.len());
            assert!(NameFilter::new(text, true).unwrap().is_all());
        }
    }

    #[test]
    fn words_must_all_occur_ignoring_case_and_accents() {
        let f = NameFilter::new("report", false).unwrap();
        assert_eq!(names(&f, DIR), ["Report 2024.pdf", "report-final.docx"]);
        let f = NameFilter::new("final report", false).unwrap();
        assert_eq!(names(&f, DIR), ["report-final.docx"]);
        let f = NameFilter::new("resume", false).unwrap();
        assert_eq!(names(&f, DIR), ["Résumé.odt"]);
        assert!(names(&NameFilter::new("zzz", false).unwrap(), DIR).is_empty());
    }

    #[test]
    fn a_pattern_matches_the_name() {
        let f = NameFilter::new(r"^img_\d+\.(jpg|png)$", true).unwrap();
        assert_eq!(names(&f, DIR), ["IMG_0001.JPG", "IMG_0002.png"]);
        // Plain words that look like a pattern are plain without the switch.
        let f = NameFilter::new("^img_", false).unwrap();
        assert!(names(&f, DIR).is_empty());
        let f = NameFilter::new(r"\.hidden$", true).unwrap();
        assert_eq!(names(&f, DIR), [".hidden"]);
    }

    #[test]
    fn an_invalid_pattern_is_an_error_not_a_filter() {
        let e = NameFilter::new("(img", true).err().unwrap();
        assert!(e.0.contains("unclosed group"), "{}", e.0);
        // The same text as plain words is fine.
        assert!(NameFilter::new("(img", false).is_ok());
    }

    #[test]
    fn names_that_are_not_utf8_are_tested_as_bytes() {
        let f = NameFilter::new("report", true).unwrap();
        assert!(f.matches(b"\xFFreport\xFE"));
        let f = NameFilter::new("report", false).unwrap();
        assert!(f.matches(b"\xFFreport\xFE"));
    }
}
