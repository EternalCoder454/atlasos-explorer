//! Launch arguments: `telamon-explorer [--new-window] [--select] [--split]
//! [URL|PATH ...]`, from the first launch, a forwarded second launch,
//! `org.freedesktop.Application.Open` and `org.freedesktop.FileManager1`.
//! Every caller is untrusted: arguments are capped, and a location is kept
//! only when it is a plain absolute path or a URL with a well-formed scheme
//! and no control or bidi characters. Only the schemes in [`LAUNCH_SCHEMES`]
//! are opened, so another app can't point Files at an arbitrary KIO worker.

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
    /// The caller is another program (D-Bus), not the person: servers and
    /// devices are not opened for it.
    pub bus: bool,
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
    // `--bus` may come after other options but before the first location.
    let from_bus = args
        .iter()
        .take(MAX_ARGS)
        .take_while(|a| a.starts_with('-') && a.as_str() != "--")
        .any(|a| a == "--bus");
    for arg in args.iter().take(MAX_ARGS) {
        if !options_done && arg.starts_with('-') {
            match arg.as_str() {
                "--" => options_done = true,
                "--new-window" => launch.new_window = true,
                "--select" => launch.select = true,
                "--split" => launch.split = true,
                // Put first by the code that takes a D-Bus call.
                "--bus" => launch.bus = true,
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
        match location(arg, cwd, from_bus) {
            Ok(url) => launch.locations.push(url),
            Err(reason) => launch.refused.push(Refusal {
                arg: shown(arg),
                reason,
            }),
        }
    }
    launch
}

fn location(arg: &str, cwd: &Path, from_bus: bool) -> Result<String, &'static str> {
    if arg.is_empty() {
        return Err("empty location");
    }
    if arg.len() > MAX_ARG_LEN {
        return Err("too long");
    }
    if arg.chars().any(is_hidden_char) || decoded_has_hidden(arg) {
        return Err("holds control or direction characters");
    }
    if let Some(scheme) = scheme_of(arg) {
        if scheme.eq_ignore_ascii_case("file") {
            return file_url(arg);
        }
        if !LAUNCH_SCHEMES
            .iter()
            .any(|k| scheme.eq_ignore_ascii_case(k))
        {
            return Err("not a kind of location Files opens");
        }
        if from_bus
            && (is_server_scheme(scheme)
                || DEVICE_SCHEMES
                    .iter()
                    .any(|k| scheme.eq_ignore_ascii_case(k)))
        {
            return Err("not opened from another program");
        }
        if is_server_scheme(scheme) {
            return clean_server_url(arg);
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

/// The non-`file` schemes a launch (command line or FileManager1) may open:
/// the places, devices, network shares and archives Files browses.
pub const LAUNCH_SCHEMES: &[&str] = &[
    "trash",
    "recentlyused",
    "network",
    "home",
    "remote",
    "desktop",
    "mtp",
    "afc",
    "smb",
    "sftp",
    "fish",
    "ftp",
    "ftps",
    "webdav",
    "webdavs",
    "nfs",
    "zip",
    "tar",
    "sevenz",
    "ar",
    "iso",
];

/// Schemes that name a server on the network.
pub const SERVER_SCHEMES: &[&str] = &[
    "smb", "sftp", "fish", "ftp", "ftps", "webdav", "webdavs", "nfs",
];
/// Schemes that name a device.
pub const DEVICE_SCHEMES: &[&str] = &["mtp", "afc"];

pub(crate) fn is_server_scheme(scheme: &str) -> bool {
    SERVER_SCHEMES
        .iter()
        .any(|k| scheme.eq_ignore_ascii_case(k))
}

/// Whether the percent-decoded form of `text` holds a NUL, a control or a
/// direction character (`%00`, `%0A`, `%E2%80%AE`).
pub(crate) fn decoded_has_hidden(text: &str) -> bool {
    let b = text.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(hex) = b.get(i + 1..i + 3)
            && let Ok(hex) = std::str::from_utf8(hex)
            && let Ok(v) = u8::from_str_radix(hex, 16)
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out.contains(&0) || String::from_utf8_lossy(&out).chars().any(is_hidden_char)
}

/// The authority of a server URL with its password taken out (`u:p@h` is
/// `u@h`); the same text when there is none.
pub(crate) fn authority_without_password(authority: &str) -> String {
    match authority.rsplit_once('@') {
        Some((user, host)) => {
            let user = user.split(':').next().unwrap_or("");
            format!("{user}@{host}")
        }
        None => authority.to_string(),
    }
}

/// `text` with the user information of a `scheme://user:password@host` URL
/// taken out, for showing a refused argument or writing a log line.
pub(crate) fn without_userinfo(text: &str) -> String {
    let Some(i) = text.find("://") else {
        return text.to_string();
    };
    let (head, rest) = text.split_at(i + 3);
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    match rest[..end].rfind('@') {
        Some(at) => format!("{head}{}", &rest[at + 1..]),
        None => text.to_string(),
    }
}

fn plain_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && !host.starts_with('-')
        // letters of any language, as Connect to Server takes them
        && host
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// A server URL as Files opens it: the user information and the host are
/// checked (neither may start with `-`, so nothing reads as an option of a
/// program a worker starts; the host is plain), a password is taken out
/// (KIO asks for it, and the kiod password server keeps it, so it never goes
/// on in a URL that is shown, copied or saved). `smb://` alone (the network)
/// passes.
pub(crate) fn clean_server_url(url: &str) -> Result<String, &'static str> {
    let colon = url.find(':').ok_or("not a server address")?;
    let (scheme, rest) = (&url[..colon], &url[colon + 1..]);
    let Some(rest) = rest.strip_prefix("//") else {
        // `smb:/` and `sftp:/path`: no authority, so nothing to check
        return Ok(url.to_string());
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    if authority.is_empty() {
        return if tail.is_empty() || tail == "/" {
            Ok(format!("{scheme}://{tail}"))
        } else {
            Err("a server address needs a server")
        };
    }
    let (user, hostport) = match authority.rsplit_once('@') {
        Some((u, h)) => (Some(u.split(':').next().unwrap_or("")), h),
        None => (None, authority),
    };
    if let Some(u) = user {
        let decoded = decode_lossy(u);
        if u.starts_with('-') || decoded.starts_with('-') {
            return Err("user name starts with a dash");
        }
    }
    let (host, port) = if let Some(v6) = hostport.strip_prefix('[') {
        let (addr, after) = v6.split_once(']').ok_or("not a valid server address")?;
        if addr.is_empty()
            || !addr.contains(':')
            || !addr
                .bytes()
                .all(|c| c.is_ascii_hexdigit() || c == b':' || c == b'.')
        {
            return Err("not a valid server address");
        }
        // only a port may follow the bracket
        if !after.is_empty() && !after.starts_with(':') {
            return Err("not a valid server address");
        }
        (format!("[{addr}]"), after.strip_prefix(':'))
    } else {
        match hostport.split_once(':') {
            Some((h, p)) => (h.to_string(), Some(p)),
            None => (hostport.to_string(), None),
        }
    };
    if hostport.contains('[') && !host.starts_with('[') {
        return Err("not a valid server address");
    }
    if !host.starts_with('[') && !plain_host(&host) {
        return Err("not a valid server name");
    }
    if let Some(p) = port.filter(|p| !p.is_empty())
        && !(p.len() <= 5
            && p.bytes().all(|c| c.is_ascii_digit())
            && matches!(p.parse::<u32>(), Ok(1..=65535)))
    {
        return Err("not a valid port");
    }
    let mut out = format!("{scheme}://");
    if let Some(u) = user {
        out.push_str(u);
        out.push('@');
    }
    out.push_str(hostport);
    out.push_str(tail);
    Ok(out)
}

fn decode_lossy(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(hex) = b.get(i + 1..i + 3)
            && let Ok(hex) = std::str::from_utf8(hex)
            && let Ok(v) = u8::from_str_radix(hex, 16)
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
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
    let arg = without_userinfo(arg);
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
    fn unknown_schemes_are_refused() {
        let l = p(
            &["http://example.com/", "exec:/bin/sh", "SFTP://host/x"],
            "/x",
        );
        assert_eq!(l.locations, vec!["SFTP://host/x".to_string()]);
        assert_eq!(l.refused.len(), 2);
        assert_eq!(l.refused[0].reason, "not a kind of location Files opens");
    }

    #[test]
    fn files_own_pages_can_be_launched() {
        let l = p(&["home:/", "network:/", "home:"], "/x");
        assert_eq!(l.locations, ["home:/", "network:/", "home:"]);
        assert!(l.refused.is_empty());
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
    fn server_addresses_lose_their_password_and_hostile_ones_are_refused() {
        let l = p(
            &[
                "sftp://u:p%40ss@host.example:2222/dir",
                "smb://",
                "ftp://[::1]/x",
            ],
            "/",
        );
        assert_eq!(
            l.locations,
            ["sftp://u@host.example:2222/dir", "smb://", "ftp://[::1]/x"]
        );
        // a program a worker starts could read these as options
        let l = p(
            &[
                "fish://-oProxyCommand%3Dx@h/",
                "fish://-oProxyCommand=x/",
                "sftp://%2Dx@h/",
                "sftp://u@-h/",
                "sftp://u@h:99999/",
                "sftp://u@h%2Fx/",
                "sftp://u@h h/",
                "sftp:///etc",
            ],
            "/",
        );
        assert!(l.locations.is_empty(), "{:?}", l.locations);
        assert_eq!(l.refused.len(), 8);
    }

    #[test]
    fn a_refused_argument_never_shows_a_password() {
        let l = p(
            &[
                "ssh://u:hunter2@h/",
                "--daemon-activation=x",
                "http://a:hunter2@b/",
            ],
            "/",
        );
        assert!(
            l.refused.iter().all(|r| !r.arg.contains("hunter2")),
            "{:?}",
            l.refused
        );
        assert!(l.refused[0].arg.contains("ssh://h/"));
    }

    #[test]
    fn another_program_may_not_open_servers_or_devices() {
        let l = p(
            &[
                "--bus",
                "--",
                "sftp://h/",
                "smb://h/s",
                "mtp://x/",
                "file:///x",
                "trash:/",
            ],
            "",
        );
        assert!(l.bus);
        assert_eq!(l.locations, ["file:///x", "trash:/"]);
        assert_eq!(l.refused.len(), 3);
        assert_eq!(l.refused[0].reason, "not opened from another program");
        // the person's own launch still can
        let l = p(&["sftp://h/"], "/");
        assert_eq!(l.locations, ["sftp://h/"]);
    }

    #[test]
    fn encoded_controls_and_direction_characters_are_refused() {
        let l = p(
            &[
                "file:///x%00y",
                "file:///a%0Ab",
                "file:///a%E2%80%AEb",
                "smb://h/a%0Ab",
                "file:///ok%20x",
            ],
            "/",
        );
        assert_eq!(l.locations, ["file:///ok%20x"]);
        assert_eq!(l.refused.len(), 4);
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
