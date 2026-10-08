//! Command-line parsing. Every argument is untrusted text from the user's shell
//! or a script: lengths are capped and numbers are checked.

use atlas_file_index::category::Category;
use atlas_file_index::uri::path_to_uri;
use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub const USAGE: &str = "\
Usage: telamon-explorer-search [OPTIONS] QUERY

Search file names with Telamon Explorer's index.

Options:
  --kind K        folder, file, or a category: document, spreadsheet,
                  presentation, pdf, image, audio, video, archive, code, text,
                  executable, font, disk-image
  --in DIR        only results under DIR
  --modified 7d   changed within the last 7 days (units: s, m, h, d, w)
  --larger 10M    at least this big (units: K, M, G, T; plain numbers are bytes)
  --smaller 1G    at most this big
  --tag NAME      only files and folders tagged NAME (ignoring case)
  --limit N       at most N results (1 to 500, default 20)
  --json          print the results as a JSON array
  -h, --help      show this help

QUERY is one or more words; every word must match. Names are matched without
regard to case or accents.
";

#[derive(Debug, Default, PartialEq)]
pub struct Args {
    pub query: String,
    /// `folder` or `file`
    pub kind: Option<String>,
    /// category names for `kinds`
    pub kinds: Vec<String>,
    pub root_uri: Option<String>,
    pub modified_after: Option<i64>,
    pub size_min: Option<u64>,
    pub size_max: Option<u64>,
    pub tag: Option<String>,
    pub limit: u32,
    pub json: bool,
}

#[derive(Debug, PartialEq)]
pub enum Parsed {
    Help,
    Run(Args),
}

const MAX_QUERY: usize = 1024;
const MAX_TAG: usize = 512;

/// `7d`, `12h`, `90m`, `30s`, `2w` (a plain number is seconds), in seconds.
pub fn parse_duration(s: &str) -> Result<u64, String> {
    let (num, mult) = match s.char_indices().last() {
        Some((i, c)) if c.is_ascii_alphabetic() => {
            let m = match c.to_ascii_lowercase() {
                's' => 1,
                'm' => 60,
                'h' => 3600,
                'd' => 86_400,
                'w' => 604_800,
                _ => {
                    return Err(format!(
                        "'{s}' is not a time span (use a number and s, m, h, d or w, like 7d)"
                    ));
                }
            };
            (&s[..i], m)
        }
        _ => (s, 1),
    };
    let n: u64 = num.parse().map_err(|_| {
        format!("'{s}' is not a time span (use a number and s, m, h, d or w, like 7d)")
    })?;
    n.checked_mul(mult)
        .ok_or_else(|| format!("'{s}' is too large"))
}

/// `10M`, `1G`, `512K`, `2048` (bytes), in bytes; K, M, G, T are powers of 1024.
pub fn parse_size(s: &str) -> Result<u64, String> {
    let (num, mult): (&str, u64) = match s.char_indices().last() {
        Some((i, c)) if c.is_ascii_alphabetic() => {
            let m = match c.to_ascii_lowercase() {
                'k' => 1 << 10,
                'm' => 1 << 20,
                'g' => 1 << 30,
                't' => 1 << 40,
                'b' => 1,
                _ => {
                    return Err(format!(
                        "'{s}' is not a size (use a number and K, M, G or T, like 10M)"
                    ));
                }
            };
            (&s[..i], m)
        }
        _ => (s, 1),
    };
    let n: u64 = num
        .parse()
        .map_err(|_| format!("'{s}' is not a size (use a number and K, M, G or T, like 10M)"))?;
    n.checked_mul(mult)
        .ok_or_else(|| format!("'{s}' is too large"))
}

fn text(v: Option<OsString>, flag: &str) -> Result<String, String> {
    let v = v.ok_or_else(|| format!("{flag} needs a value"))?;
    v.into_string()
        .map_err(|_| format!("the value of {flag} is not valid text"))
}

pub fn parse(args: impl Iterator<Item = OsString>) -> Result<Parsed, String> {
    parse_at(
        args,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64),
    )
}

/// `parse` with the clock given, for tests.
pub fn parse_at(mut args: impl Iterator<Item = OsString>, now: i64) -> Result<Parsed, String> {
    let mut a = Args {
        limit: 20,
        ..Default::default()
    };
    let mut words: Vec<String> = Vec::new();
    let mut only_words = false;
    while let Some(arg) = args.next() {
        if only_words {
            words.push(
                arg.into_string()
                    .map_err(|_| "the query is not valid text".to_string())?,
            );
            continue;
        }
        let Some(flag) = arg.to_str().map(str::to_owned) else {
            return Err("the query is not valid text".into());
        };
        match flag.as_str() {
            "-h" | "--help" => return Ok(Parsed::Help),
            "--" => only_words = true,
            "--json" => a.json = true,
            "--kind" => {
                let k = text(args.next(), "--kind")?;
                match k.as_str() {
                    "folder" | "file" => a.kind = Some(k),
                    other if Category::from_name(other).is_some() => {
                        if other == "folder" {
                            a.kind = Some("folder".into());
                        } else {
                            a.kinds.push(k);
                        }
                    }
                    _ => {
                        return Err(format!(
                            "'{k}' is not a kind (folder, file, or a category: see --help)"
                        ));
                    }
                }
            }
            "--in" => {
                let d = args.next().ok_or("--in needs a folder")?;
                let abs = std::path::absolute(Path::new(&d))
                    .map_err(|e| format!("cannot use the folder for --in: {e}"))?;
                // lexical `..` removal: the service refuses `..`
                let mut clean = std::path::PathBuf::from("/");
                for c in abs.components() {
                    use std::path::Component::*;
                    match c {
                        Normal(n) => clean.push(n),
                        ParentDir => {
                            clean.pop();
                        }
                        _ => {}
                    }
                }
                if clean.as_os_str().as_bytes().len() > 4096 {
                    return Err("the folder for --in is too long".into());
                }
                a.root_uri = Some(path_to_uri(clean.as_os_str().as_bytes()));
            }
            "--modified" => {
                let secs = parse_duration(&text(args.next(), "--modified")?)?;
                a.modified_after =
                    Some(now.saturating_sub(i64::try_from(secs).unwrap_or(i64::MAX)));
            }
            "--larger" => a.size_min = Some(parse_size(&text(args.next(), "--larger")?)?),
            "--smaller" => a.size_max = Some(parse_size(&text(args.next(), "--smaller")?)?),
            "--tag" => {
                let t = text(args.next(), "--tag")?;
                let t = t.trim();
                if t.is_empty() || t.len() > MAX_TAG {
                    return Err(format!("--tag needs a tag name (at most {MAX_TAG} bytes)"));
                }
                a.tag = Some(t.to_string());
            }
            "--limit" => {
                let v = text(args.next(), "--limit")?;
                let n: u32 = v.parse().map_err(|_| format!("'{v}' is not a number"))?;
                if !(1..=500).contains(&n) {
                    return Err("--limit must be between 1 and 500".into());
                }
                a.limit = n;
            }
            f if f.starts_with("--") => return Err(format!("unknown option {f}")),
            _ => words.push(flag),
        }
    }
    a.query = words.join(" ");
    if a.query.len() > MAX_QUERY {
        return Err(format!("the query is too long (at most {MAX_QUERY} bytes)"));
    }
    let has_filter = a.kind.is_some()
        || !a.kinds.is_empty()
        || a.root_uri.is_some()
        || a.modified_after.is_some()
        || a.size_min.is_some()
        || a.size_max.is_some()
        || a.tag.is_some();
    if a.query.trim().is_empty() && !has_filter {
        return Err(
            "give a query, or at least one filter (--kind, --in, --modified, --larger, --smaller, --tag)"
                .into(),
        );
    }
    Ok(Parsed::Run(a))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(a: &[&str]) -> Result<Parsed, String> {
        parse_at(a.iter().map(OsString::from), 1_000_000)
    }

    fn run(a: &[&str]) -> Args {
        match p(a) {
            Ok(Parsed::Run(a)) => a,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn units() {
        assert_eq!(parse_duration("7d"), Ok(604_800));
        assert_eq!(parse_duration("90m"), Ok(5400));
        assert_eq!(parse_duration("2W"), Ok(1_209_600));
        assert_eq!(parse_duration("45"), Ok(45));
        assert!(parse_duration("d").is_err());
        assert!(parse_duration("7x").is_err());
        assert!(parse_duration("-1d").is_err());
        assert!(parse_duration("99999999999999999999d").is_err());
        assert_eq!(parse_size("10M"), Ok(10 << 20));
        assert_eq!(parse_size("1g"), Ok(1 << 30));
        assert_eq!(parse_size("512"), Ok(512));
        assert!(parse_size("1.5G").is_err());
        assert!(parse_size("M").is_err());
        assert!(parse_size("99999999999T").is_err());
    }

    #[test]
    fn full_command_line() {
        let a = run(&[
            "--kind",
            "image",
            "--in",
            "/home/a/../b/",
            "--modified",
            "7d",
            "--larger",
            "10M",
            "--smaller",
            "1G",
            "--tag",
            " Taxes 2025 ",
            "--limit",
            "5",
            "--json",
            "my",
            "photo",
        ]);
        assert_eq!(a.query, "my photo");
        assert_eq!(a.kinds, ["image"]);
        assert_eq!(a.kind, None);
        assert_eq!(a.root_uri.as_deref(), Some("file:///home/b"));
        assert_eq!(a.modified_after, Some(1_000_000 - 604_800));
        assert_eq!((a.size_min, a.size_max), (Some(10 << 20), Some(1 << 30)));
        assert_eq!(a.limit, 5);
        assert_eq!(a.tag.as_deref(), Some("Taxes 2025"));
        assert!(a.json);
        assert_eq!(
            run(&["--kind", "folder", "x"]).kind.as_deref(),
            Some("folder")
        );
        assert_eq!(run(&["--kind", "file", "x"]).kind.as_deref(), Some("file"));
        assert_eq!(run(&["x"]).limit, 20);
    }

    #[test]
    fn dash_dash_ends_options() {
        assert_eq!(run(&["--", "--json", "x"]).query, "--json x");
    }

    #[test]
    fn errors_are_plain() {
        for bad in [
            &[][..],
            &["--kind"],
            &["--kind", "nonsense", "x"],
            &["--limit", "0", "x"],
            &["--limit", "501", "x"],
            &["--limit", "abc", "x"],
            &["--bogus", "x"],
            &["--modified", "soon", "x"],
            &["--larger", "big", "x"],
            &["--tag"],
            &["--tag", "", "x"],
            &["--tag", "   ", "x"],
            &["  "],
        ] {
            let e = p(bad).expect_err(&format!("{bad:?}"));
            assert!(!e.is_empty());
        }
        assert!(p(&["--kind", "image"]).is_ok(), "a filter alone is a query");
        assert_eq!(
            run(&["--tag", "Red"]).tag.as_deref(),
            Some("Red"),
            "a tag alone is a query"
        );
        assert_eq!(run(&["x"]).tag, None);
        assert!(p(&["x".repeat(2000).as_str()]).is_err());
        assert_eq!(p(&["--help"]), Ok(Parsed::Help));
    }

    #[test]
    fn non_utf8_is_refused() {
        use std::os::unix::ffi::OsStringExt;
        let r = parse_at(vec![OsString::from_vec(vec![0xFF, 0xFE])].into_iter(), 0);
        assert!(r.is_err());
    }
}
