//! The address bar: typed text to a URL, and completion of the last part.
//! Pure: the home folder and the current folder are arguments, nothing is
//! read from the environment or the disk. Typed text is untrusted input and
//! follows the launch rules (no control or bidi characters, length capped).

use crate::launch::{MAX_ARG_LEN, file_url, is_hidden_char, normalize, path_to_url, scheme_of};
use crate::sort::name_key;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

/// Most completions offered.
pub const MAX_COMPLETIONS: usize = 50;

/// Schemes that are written without `//`.
const BARE_SCHEMES: [&str; 4] = ["trash", "recent", "network", "home"];

/// A refusal, in plain words.
pub type Refused = &'static str;

fn check_text(text: &str) -> Result<(), Refused> {
    if text.len() > MAX_ARG_LEN {
        return Err("The address is too long.");
    }
    if text.chars().any(is_hidden_char) {
        return Err("The address has control or text-direction characters.");
    }
    Ok(())
}

/// The scheme when `text` is a URL we keep as given: `scheme://...`, or
/// `trash:`, `recent:`, `network:`, `home:`.
fn url_scheme(text: &str) -> Option<&str> {
    let scheme = scheme_of(text)?;
    let rest = &text[scheme.len() + 1..];
    let bare = BARE_SCHEMES.iter().any(|b| scheme.eq_ignore_ascii_case(b));
    (bare || rest.starts_with("//")).then_some(scheme)
}

/// The local path of a `file:///...` URL (percent-decoded), or None for any
/// other URL.
pub fn url_to_path(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    if !rest.starts_with('/') {
        return None;
    }
    let b = rest.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    if out.contains(&0) {
        return None;
    }
    Some(PathBuf::from(OsString::from_vec(out)))
}

/// Reads typed text as a URL. `current` is the URL of the folder shown (for
/// relative names; they need a local one) and `home` the user's home folder.
pub fn parse(text: &str, current: &str, home: &Path) -> Result<String, Refused> {
    let text = text.trim();
    check_text(text)?;
    if text.is_empty() {
        return Err("The address is empty.");
    }
    if let Some(scheme) = url_scheme(text) {
        if scheme.eq_ignore_ascii_case("file") {
            return file_url(text).map_err(|_| "A file address can't name another computer.");
        }
        return Ok(text.to_string());
    }
    let path = if text == "~" {
        home.to_path_buf()
    } else if let Some(rest) = text.strip_prefix("~/") {
        home.join(rest)
    } else if text.starts_with('/') {
        PathBuf::from(text)
    } else {
        match url_to_path(current) {
            Some(dir) if dir.is_absolute() => dir.join(text),
            _ => return Err("A relative name needs a folder on this computer to start from."),
        }
    };
    if !path.is_absolute() {
        return Err("The home folder isn't known.");
    }
    Ok(path_to_url(&normalize(&path)))
}

/// Splits typed text for completion into the folder to list and the name
/// prefix to match: `~/Doc` is (home, "Doc"), `/usr/li` is (`/usr/`, "li"),
/// `ba` is (the current folder, "ba").
pub fn split_for_completion(
    text: &str,
    current: &str,
    home: &Path,
) -> Result<(String, String), Refused> {
    check_text(text)?;
    if text.is_empty() {
        return Ok((current.to_string(), String::new()));
    }
    if text == "~" {
        return Ok((parse("~", current, home)?, String::new()));
    }
    if let Some(scheme) = url_scheme(text)
        && BARE_SCHEMES.iter().any(|b| scheme.eq_ignore_ascii_case(b))
        && !text.contains('/')
    {
        return Ok((format!("{scheme}:/"), String::new()));
    }
    match text.rfind('/') {
        None => Ok((current.to_string(), text.to_string())),
        Some(i) => {
            let (dir, prefix) = text.split_at(i + 1);
            Ok((parse(dir, current, home)?, prefix.to_string()))
        }
    }
}

/// One name in the folder being completed.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub name: String,
    pub is_dir: bool,
    /// Hidden by the filesystem or a `.hidden` file; names starting with a
    /// dot count as hidden too.
    pub hidden: bool,
}

/// Which candidates to offer, best first, as indices into `candidates`:
/// folders only; names that start with `prefix` (any case) before names that
/// merely contain it; natural order within each group; hidden names only when
/// the prefix starts with a dot; names with control or bidi characters never
/// (they could not be typed back); at most 50.
pub fn rank_completions(prefix: &str, candidates: &[Candidate]) -> Vec<usize> {
    let want_hidden = prefix.starts_with('.');
    let needle = prefix.to_lowercase();
    let mut starts: Vec<(Vec<u8>, usize)> = Vec::new();
    let mut contains: Vec<(Vec<u8>, usize)> = Vec::new();
    for (i, c) in candidates.iter().enumerate() {
        if !c.is_dir
            || ((c.hidden || c.name.starts_with('.')) && !want_hidden)
            || c.name.chars().any(is_hidden_char)
        {
            continue;
        }
        let lower = c.name.to_lowercase();
        if lower.starts_with(&needle) {
            starts.push((name_key(c.name.as_bytes()), i));
        } else if lower.contains(&needle) {
            contains.push((name_key(c.name.as_bytes()), i));
        }
    }
    starts.sort_unstable();
    contains.sort_unstable();
    starts
        .into_iter()
        .chain(contains)
        .map(|(_, i)| i)
        .take(MAX_COMPLETIONS)
        .collect()
}

/// The address bar's text after a completion is taken: what was typed up to
/// its last `/` (`~` and `trash:` count as ending in one), then `name` and a
/// closing `/`. In a URL the name is percent-encoded, so a `%` or a space in
/// it can't change the address. `dir` is the folder being listed (as
/// `split_for_completion` gave it): a name typed with no folder part inside a
/// folder that isn't on this computer starts from it, since `parse` refuses
/// relative names there.
pub fn completion_text(typed: &str, name: &str, dir: &str) -> String {
    let scheme = url_scheme(typed);
    let mut url_form = scheme.is_some();
    let base = match typed.rfind('/') {
        Some(i) => typed[..=i].to_string(),
        None if typed == "~" => "~/".to_string(),
        None => match scheme {
            Some(s) => format!("{s}:/"),
            None if !dir.is_empty() && !dir.starts_with("file:") => {
                url_form = true;
                format!("{}/", dir.trim_end_matches('/'))
            }
            None => String::new(),
        },
    };
    let mut out = base;
    if url_form {
        for &b in name.as_bytes() {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                out.push(b as char);
            } else {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    } else {
        out.push_str(name);
    }
    out.push('/');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/home/u";
    const CUR: &str = "file:///home/u/docs";

    fn p(text: &str) -> Result<String, Refused> {
        parse(text, CUR, Path::new(HOME))
    }

    #[test]
    fn home_and_absolute() {
        assert_eq!(p("~").unwrap(), "file:///home/u");
        assert_eq!(p("~/a b").unwrap(), "file:///home/u/a%20b");
        assert_eq!(p("/etc/../tmp/").unwrap(), "file:///tmp");
        assert_eq!(p("  /etc  ").unwrap(), "file:///etc");
        assert_eq!(p("/").unwrap(), "file:///");
    }

    #[test]
    fn relative_to_the_current_folder() {
        assert_eq!(p("..").unwrap(), "file:///home/u");
        assert_eq!(p("../x/./y").unwrap(), "file:///home/u/x/y");
        assert_eq!(p("a:b").unwrap(), "file:///home/u/docs/a%3Ab");
        assert_eq!(p("../../../../..").unwrap(), "file:///");
        let encoded = parse("n", "file:///home/u/a%20b%C3%A9", Path::new(HOME)).unwrap();
        assert_eq!(encoded, "file:///home/u/a%20b%C3%A9/n");
    }

    #[test]
    fn relative_needs_a_local_folder() {
        assert!(parse("x", "trash:/", Path::new(HOME)).is_err());
        assert!(parse("x", "smb://nas/s", Path::new(HOME)).is_err());
        assert!(parse("x", "file://host/x", Path::new(HOME)).is_err());
        assert!(parse("~", "trash:/", Path::new("")).is_err());
    }

    #[test]
    fn urls_are_kept() {
        assert_eq!(p("trash:/").unwrap(), "trash:/");
        assert_eq!(p("trash:").unwrap(), "trash:");
        assert_eq!(p("recent:/").unwrap(), "recent:/");
        assert_eq!(p("network:/").unwrap(), "network:/");
        assert_eq!(p("home:/").unwrap(), "home:/");
        assert_eq!(p("home:").unwrap(), "home:");
        assert_eq!(p("smb://nas/share").unwrap(), "smb://nas/share");
        assert_eq!(p("file:///etc").unwrap(), "file:///etc");
        assert!(p("file://evil/etc").is_err());
    }

    #[test]
    fn untrusted_text_is_refused() {
        assert!(p("").is_err());
        assert!(p("   ").is_err());
        assert!(p("/tmp/a\u{202E}b").is_err());
        assert!(p("/tmp/a\0b").is_err());
        assert!(p("smb://x/\u{1b}[0m").is_err());
        assert!(p(&format!("/{}", "a".repeat(MAX_ARG_LEN))).is_err());
        assert!(split_for_completion("/a\nb", CUR, Path::new(HOME)).is_err());
    }

    #[test]
    fn split() {
        let s = |t: &str| split_for_completion(t, CUR, Path::new(HOME)).unwrap();
        assert_eq!(s("ba"), (CUR.to_string(), "ba".to_string()));
        assert_eq!(s(""), (CUR.to_string(), String::new()));
        assert_eq!(s("~"), ("file:///home/u".into(), String::new()));
        assert_eq!(s("~/Doc"), ("file:///home/u".into(), "Doc".into()));
        assert_eq!(s("/usr/li"), ("file:///usr".into(), "li".into()));
        assert_eq!(s("/usr/"), ("file:///usr".into(), String::new()));
        assert_eq!(s("../x"), ("file:///home/u".into(), "x".into()));
        assert_eq!(s("smb://nas/sh"), ("smb://nas/".into(), "sh".into()));
        assert_eq!(s("trash:"), ("trash:/".into(), String::new()));
    }

    fn cand(name: &str, is_dir: bool) -> Candidate {
        Candidate {
            name: name.into(),
            is_dir,
            hidden: false,
        }
    }

    fn rank(prefix: &str, list: &[Candidate]) -> Vec<String> {
        rank_completions(prefix, list)
            .into_iter()
            .map(|i| list[i].name.clone())
            .collect()
    }

    #[test]
    fn ranking_groups_and_order() {
        let list = [
            cand("my docs", true),
            cand("Docs10", true),
            cand("docs2", true),
            cand("docs.txt", false),
            cand("Downloads", true),
            cand(".docs", true),
        ];
        assert_eq!(rank("doc", &list), ["docs2", "Docs10", "my docs"]);
        assert_eq!(rank("DOC", &list), ["docs2", "Docs10", "my docs"]);
        assert_eq!(rank(".do", &list), [".docs"]);
        assert_eq!(rank("", &list), ["docs2", "Docs10", "Downloads", "my docs"]);
        assert!(rank("zzz", &list).is_empty());
    }

    #[test]
    fn hidden_flag_and_cap() {
        let mut hidden = cand("secret", true);
        hidden.hidden = true;
        assert!(rank("s", &[hidden]).is_empty());
        let many: Vec<Candidate> = (0..200).map(|i| cand(&format!("d{i}"), true)).collect();
        let out = rank_completions("d", &many);
        assert_eq!(out.len(), MAX_COMPLETIONS);
        assert_eq!(out[0], 0);
        assert_eq!(out[10], 10);
    }

    #[test]
    fn names_that_cannot_be_typed_are_not_offered() {
        let list = [
            cand("fine", true),
            cand("new\nline", true),
            cand("bi\u{202E}di", true),
        ];
        assert_eq!(rank_completions("", &list), [0]);
    }

    #[test]
    fn completion_replaces_the_last_part() {
        assert_eq!(completion_text("ba", "bar", "file:///x"), "bar/");
        assert_eq!(completion_text("", "bar", "file:///x"), "bar/");
        assert_eq!(
            completion_text("~", "Documents", "file:///x"),
            "~/Documents/"
        );
        assert_eq!(
            completion_text("~/Do", "Documents", "file:///x"),
            "~/Documents/"
        );
        assert_eq!(completion_text("/usr/li", "lib", "file:///x"), "/usr/lib/");
        assert_eq!(completion_text("../x", "xy z", "file:///x"), "../xy z/");
        assert_eq!(completion_text("trash:", "a", "file:///x"), "trash:/a/");
        assert_eq!(completion_text("trash:/", "a", "file:///x"), "trash:/a/");
    }

    #[test]
    fn a_bare_name_in_a_remote_folder_starts_from_it() {
        assert_eq!(
            completion_text("sh", "my share", "smb://nas/"),
            "smb://nas/my%20share/"
        );
        assert_eq!(completion_text("t", "a", "trash:/"), "trash:/a/");
        assert_eq!(completion_text("t", "a", "file:///x"), "a/");
    }

    #[test]
    fn completion_in_a_url_is_encoded() {
        assert_eq!(
            completion_text("smb://nas/sh", "my share", "file:///x"),
            "smb://nas/my%20share/"
        );
        assert_eq!(
            completion_text("smb://nas/", "50%", "file:///x"),
            "smb://nas/50%25/"
        );
        assert_eq!(
            completion_text("file:///tmp/a", "\u{e9}t\u{e9}", "file:///x"),
            "file:///tmp/%C3%A9t%C3%A9/"
        );
    }
}
