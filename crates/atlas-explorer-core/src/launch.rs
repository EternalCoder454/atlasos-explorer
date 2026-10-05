//! Launch arguments: `atlas-explorer [--new-window] [--select] [--split]
//! [URL|PATH ...]`, from the first launch, a forwarded second launch,
//! `org.freedesktop.Application.Open` and `org.freedesktop.FileManager1`.
//! Every caller is untrusted: arguments are capped, and a location is kept
//! only when it is a plain absolute path or a URL with a well-formed scheme
//! and no control or bidi characters. Whether KIO knows the scheme is checked
//! by the app (`KProtocolInfo`), which this crate cannot ask.

use std::path::{Component, Path, PathBuf};

/// Arguments looked at per launch; the rest are counted, not read.
pub const MAX_ARGS: usize = 64;
/// Longest argument kept (a path or URL), in bytes.
pub const MAX_ARG_LEN: usize = 8192;

/// What a launch asks for.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Launch {
    /// Open in a new window instead of new tabs of the active one.
    pub new_window: bool,
    /// Open each location's parent with the location selected (ShowItems).
    pub select: bool,
    /// Open the first two locations side by side in one tab.
    pub split: bool,
    /// Locations, in order: `file://` URLs for local paths, other URLs as
    /// given.
    pub locations: Vec<String>,
    /// Arguments refused, with the reason in plain words.
    pub refused: Vec<Refusal>,
    /// Arguments past `MAX_ARGS`, not looked at.
    pub dropped: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Refusal {
    /// The argument as it may be shown: made visible, shortened.
    pub arg: String,
    pub reason: &'static str,
}

/// Reads one launch's arguments (without the program name). Relative paths
/// are read against `cwd`; an empty `cwd` (D-Bus callers) refuses them.
pub fn parse(args: &[String], cwd: &Path) -> Launch {
    let mut launch = Launch {
        dropped: args.len().saturating_sub(MAX_ARGS),
        ..Launch::default()
    };
    let mut options_done = false;
    for arg in args.iter().take(MAX_ARGS) {
        if !options_done && arg.starts_with('-') {
            match arg.as_str() {
                "--" => options_done = true,
                "--new-window" => launch.new_window = true,
                "--select" => launch.select = true,
                "--split" => launch.split = true,
                // Sent by D-Bus activation; nothing to do.
                "--daemon-activation" => {}
                _ => launch.refused.push(Refusal {
                    // An option is shown without any value glued to it.
                    arg: shown(arg.split('=').next().unwrap_or("")),
                    reason: "unknown option",
                }),
            }
            continue;
        }
        match location(arg, cwd) {
            Ok(url) => launch.locations.push(url),
            Err(reason) => launch.refused.push(Refusal {
                arg: shown(arg),
                reason,
            }),
        }
    }
    launch
}

fn location(arg: &str, cwd: &Path) -> Result<String, &'static str> {
    if arg.is_empty() {
        return Err("empty location");
    }
    if arg.len() > MAX_ARG_LEN {
        return Err("too long");
    }
    if arg.chars().any(is_hidden_char) {
        return Err("holds control or direction characters");
    }
    if let Some(scheme) = scheme_of(arg) {
        if scheme.eq_ignore_ascii_case("file") {
            return file_url(arg);
        }
        return Ok(arg.to_string());
    }
    let path = Path::new(arg);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else if cwd.as_os_str().is_empty() || !cwd.is_absolute() {
        return Err("relative path with no folder to read it from");
    } else {
        cwd.join(path)
    };
    Ok(path_to_url(&normalize(&absolute)))
}

/// The scheme of `arg` when it starts like a URL (`scheme:`), per RFC 3986:
/// a letter, then letters, digits, `+`, `-` or `.`.
pub(crate) fn scheme_of(arg: &str) -> Option<&str> {
    let colon = arg.find(':')?;
    let scheme = &arg[..colon];
    let mut chars = scheme.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic()
        || !chars.all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
    {
        return None;
    }
    Some(scheme)
}

/// A `file:` URL is kept only as `file:///abs/path` (or `file://localhost/`),
/// so a host can't sneak in.
pub(crate) fn file_url(arg: &str) -> Result<String, &'static str> {
    let rest = &arg[5..];
    let path = if let Some(p) = rest.strip_prefix("//localhost/") {
        format!("/{p}")
    } else if let Some(p) = rest.strip_prefix("///") {
        format!("/{p}")
    } else if rest.starts_with('/') && !rest.starts_with("//") {
        rest.to_string()
    } else {
        return Err("file URL with a host");
    };
    Ok(format!("file://{path}"))
}

/// Removes `.` and resolves `..` lexically, as a shell would for a typed
/// path; symlinks are left to KIO.
pub(crate) fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        out.push("/");
    }
    out
}

/// `file://` URL for an absolute path, percent-encoding everything but
/// unreserved characters and `/`.
pub fn path_to_url(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut url = String::from("file://");
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            url.push(b as char);
        } else {
            url.push_str(&format!("%{b:02X}"));
        }
    }
    url
}

/// Characters that must never be shown raw: C0 and C1 controls, DEL, and the
/// bidi controls that reorder text around them.
pub fn is_hidden_char(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{200E}' | '\u{200F}' | '\u{061C}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// An argument as it may be shown in a refusal: hidden characters made
/// visible, at most 120 characters.
fn shown(arg: &str) -> String {
    let mut out = String::new();
    for (i, c) in arg.chars().enumerate() {
        if i == 120 {
            out.push('…');
            break;
        }
        if is_hidden_char(c) {
            out.push_str(&format!("\\u{{{:x}}}", c as u32));
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str], cwd: &str) -> Launch {
        parse(
            &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            Path::new(cwd),
        )
    }

    #[test]
    fn options_and_paths() {
        let l = p(
            &[
                "--select",
                "docs/a b.txt",
                "/etc/../tmp/./x",
                "smb://nas/share",
            ],
            "/home/u",
        );
        assert!(l.select && !l.new_window && !l.split);
        assert_eq!(
            l.locations,
            [
                "file:///home/u/docs/a%20b.txt",
                "file:///tmp/x",
                "smb://nas/share"
            ]
        );
        assert!(l.refused.is_empty());
    }

    #[test]
    fn relative_paths_need_a_folder() {
        let l = p(&["a"], "");
        assert!(l.locations.is_empty());
        assert_eq!(
            l.refused[0].reason,
            "relative path with no folder to read it from"
        );
    }

    #[test]
    fn options_after_double_dash_are_locations() {
        let l = p(&["--", "--new-window"], "/x");
        assert!(!l.new_window);
        assert_eq!(l.locations, ["file:///x/--new-window"]);
    }

    #[test]
    fn hidden_characters_are_refused_and_shown_safely() {
        let l = p(&["/tmp/a\u{202E}gpj.exe", "--bad=secret\n"], "/");
        assert!(l.locations.is_empty());
        assert_eq!(l.refused[0].arg, "/tmp/a\\u{202e}gpj.exe");
        assert_eq!(l.refused[1].arg, "--bad");
    }

    #[test]
    fn file_urls_with_hosts_are_refused() {
        let l = p(
            &[
                "file://evil/etc",
                "file:///etc",
                "file://localhost/etc",
                "file:/etc",
            ],
            "/",
        );
        assert_eq!(l.locations, ["file:///etc", "file:///etc", "file:///etc"]);
        assert_eq!(l.refused[0].reason, "file URL with a host");
    }

    #[test]
    fn arguments_are_capped() {
        let args: Vec<String> = (0..70).map(|i| format!("/f{i}")).collect();
        let l = parse(&args, Path::new("/"));
        assert_eq!(l.locations.len(), MAX_ARGS);
        assert_eq!(l.dropped, 6);
    }

    #[test]
    fn non_utf8_bytes_are_percent_encoded() {
        use std::os::unix::ffi::OsStrExt;
        let path = Path::new(std::ffi::OsStr::from_bytes(b"/a\xff"));
        assert_eq!(path_to_url(path), "file:///a%FF");
    }
}
