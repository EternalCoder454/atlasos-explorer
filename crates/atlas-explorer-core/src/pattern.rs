//! Patterns: the regular expressions of the folder filter, the name search and
//! the search inside files. The `regex` crate runs in time linear in the text
//! it looks at (no backtracking), so a hostile pattern cannot make a search
//! hang; what it can be is big, and that is capped here (the length of the
//! pattern, and the size of what it compiles to). Case is ignored unless the
//! pattern says `(?-i)`. Invalid input comes back as a short reason in plain
//! words, never run. See docs/DESIGN.md, "Search".

use crate::display::display_name;
use regex::Error;
use regex::bytes::{Regex, RegexBuilder};

/// Longest pattern, in bytes.
pub const MAX_PATTERN_BYTES: usize = 512;
/// Largest compiled pattern, and the largest lazy-DFA cache, in bytes.
const SIZE_LIMIT: usize = 1 << 20;
/// Deepest nesting of groups.
const NEST_LIMIT: u32 = 50;

/// A compiled pattern, matched against bytes (names and file content need not
/// be UTF-8).
#[derive(Clone, Debug)]
pub struct Pattern {
    re: Regex,
}

/// Why a pattern was refused, as a sentence that can be shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatternError(pub String);

impl std::fmt::Display for PatternError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl Pattern {
    /// Compiles `text` as a regular expression.
    pub fn new(text: &str) -> Result<Pattern, PatternError> {
        if text.len() > MAX_PATTERN_BYTES {
            return Err(PatternError(
                "That pattern is too long. Use a shorter one.".into(),
            ));
        }
        build(text).map(|re| Pattern { re })
    }

    /// The words typed, taken literally (every character stands for itself).
    pub fn literal(text: &str) -> Result<Pattern, PatternError> {
        if text.len() > MAX_PATTERN_BYTES {
            return Err(PatternError(
                "That text is too long. Use fewer words.".into(),
            ));
        }
        build(&regex::escape(text)).map(|re| Pattern { re })
    }

    /// Does the pattern match anywhere in `hay`?
    pub fn is_match(&self, hay: &[u8]) -> bool {
        self.re.is_match(hay)
    }

    /// The first match, as byte offsets.
    pub fn find(&self, hay: &[u8]) -> Option<(usize, usize)> {
        self.re.find(hay).map(|m| (m.start(), m.end()))
    }
}

fn build(text: &str) -> Result<Regex, PatternError> {
    RegexBuilder::new(text)
        .case_insensitive(true)
        .size_limit(SIZE_LIMIT)
        .dfa_size_limit(SIZE_LIMIT)
        .nest_limit(NEST_LIMIT)
        .build()
        .map_err(reason)
}

/// The reason in plain words: the parser's own last line ("unclosed group"),
/// made safe to show.
fn reason(e: Error) -> PatternError {
    let why = match &e {
        Error::CompiledTooBig(_) => "it is too complex".to_string(),
        Error::Syntax(text) => text
            .lines()
            .rev()
            .find_map(|l| l.trim().strip_prefix("error:"))
            .map(|r| display_name(r.trim()))
            .filter(|r| !r.is_empty())
            .unwrap_or_else(|| "it can't be read".to_string()),
        _ => "it can't be read".to_string(),
    };
    PatternError(format!("That isn't a valid pattern: {why}"))
}

/// Checks `text` as a pattern without keeping it: `None` when it is fine (or
/// empty, which asks for nothing), else the reason.
pub fn check(text: &str) -> Option<PatternError> {
    if text.trim().is_empty() {
        return None;
    }
    Pattern::new(text).err()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_names_ignoring_case() {
        let p = Pattern::new(r"^img_\d+\.jpe?g$").unwrap();
        assert!(p.is_match(b"IMG_0042.JPG"));
        assert!(p.is_match(b"img_7.jpeg"));
        assert!(!p.is_match(b"img_x.jpg"));
        assert!(!p.is_match(b"my img_1.jpg"));
        // The pattern can ask for case.
        let p = Pattern::new(r"(?-i)^README$").unwrap();
        assert!(p.is_match(b"README") && !p.is_match(b"readme"));
    }

    #[test]
    fn matches_bytes_that_are_not_utf8() {
        let p = Pattern::new("report").unwrap();
        assert!(p.is_match(b"re\xFF report \xFE.txt"));
        assert!(!p.is_match(b"\xFF\xFE"));
    }

    #[test]
    fn invalid_patterns_say_why() {
        for bad in ["(abc", "abc)", "[z-a]", "*x", r"\p{Nope}", "a{2,1}"] {
            let e = check(bad).unwrap_or_else(|| panic!("{bad} was accepted"));
            assert!(
                e.0.starts_with("That isn't a valid pattern: "),
                "{bad}: {}",
                e.0
            );
            assert!(e.0.len() < 120 && !e.0.contains('\n'), "{bad}: {}", e.0);
        }
        assert!(check("(abc").unwrap().0.contains("unclosed group"));
        // Nothing to look for is not an error.
        assert!(check("").is_none() && check("   ").is_none());
    }

    #[test]
    fn size_is_capped() {
        let long = "a".repeat(MAX_PATTERN_BYTES + 1);
        assert!(check(&long).unwrap().0.contains("too long"));
        assert!(check(&"a".repeat(MAX_PATTERN_BYTES)).is_none());
        // Small text that compiles to a huge program is refused, not built.
        let e = check(r"(\w{100}){100}").unwrap();
        assert!(e.0.contains("too complex"), "{}", e.0);
        // Deep nesting is refused.
        let deep = format!("{}a{}", "(".repeat(100), ")".repeat(100));
        assert!(check(&deep).is_some());
    }

    #[test]
    fn a_hostile_pattern_runs_in_linear_time() {
        // Exponential for a backtracking engine; instant here.
        let p = Pattern::new("(a*)*b").unwrap();
        let hay = vec![b'a'; 200_000];
        let t = std::time::Instant::now();
        assert!(!p.is_match(&hay));
        assert!(
            t.elapsed() < std::time::Duration::from_secs(5),
            "{:?}",
            t.elapsed()
        );
        let p = Pattern::new("(x+x+)+y").unwrap();
        assert!(!p.is_match(&vec![b'x'; 100_000]));
    }

    #[test]
    fn literal_text_stands_for_itself() {
        let p = Pattern::literal("a.b (c)+[x]").unwrap();
        assert!(p.is_match(b"xx A.B (C)+[X] yy"));
        assert!(!p.is_match(b"axb (c)+[x]"));
        assert_eq!(p.find(b"zz a.b (c)+[x]"), Some((3, 14)));
        assert!(Pattern::literal(&"a".repeat(MAX_PATTERN_BYTES + 1)).is_err());
    }
}
