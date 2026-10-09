//! Connect to Server: the address of a server built from what the user typed
//! (protocol, server, folder, user), the list of recent servers, and which
//! locations are not encrypted.
//!
//! What is typed is untrusted text: it is checked field by field and the URL
//! is built from the checked parts, percent-encoding everything that is not
//! plain, so a field can never add a user, a password, a port, a query or
//! another host to the address. A password is never part of it: KIO asks for
//! it when the server wants one, and neither the recent list nor the sidebar
//! place holds one.

use crate::launch::is_hidden_char;

/// Most servers kept in the recent list.
pub const MAX_RECENT: usize = 10;
/// Longest server name or address (a DNS name), in bytes.
pub const MAX_SERVER: usize = 253;
/// Longest user name, in bytes.
pub const MAX_USER: usize = 128;
/// Longest folder, in bytes (before it is encoded).
pub const MAX_FOLDER: usize = 1024;
/// Longest address kept in the recent list, in bytes.
pub const MAX_URL: usize = 2048;

/// A refusal, in plain words.
pub type Refused = &'static str;

/// The protocols Connect to Server offers, in the order of its list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    Sftp = 0,
    Smb = 1,
    Ftp = 2,
    Webdavs = 3,
    Webdav = 4,
    Nfs = 5,
}

pub const PROTOCOLS: [Protocol; 6] = [
    Protocol::Sftp,
    Protocol::Smb,
    Protocol::Ftp,
    Protocol::Webdavs,
    Protocol::Webdav,
    Protocol::Nfs,
];

impl Protocol {
    pub fn from_code(code: u32) -> Option<Protocol> {
        PROTOCOLS.get(code as usize).copied()
    }

    /// The KIO scheme.
    pub fn scheme(self) -> &'static str {
        match self {
            Protocol::Sftp => "sftp",
            Protocol::Smb => "smb",
            Protocol::Ftp => "ftp",
            Protocol::Webdavs => "webdavs",
            Protocol::Webdav => "webdav",
            Protocol::Nfs => "nfs",
        }
    }

    pub fn from_scheme(scheme: &str) -> Option<Protocol> {
        PROTOCOLS
            .into_iter()
            .find(|p| p.scheme().eq_ignore_ascii_case(scheme))
    }

    /// The name in the list.
    pub fn label(self) -> &'static str {
        match self {
            Protocol::Sftp => "SFTP (SSH)",
            Protocol::Smb => "Windows Share (SMB)",
            Protocol::Ftp => "FTP",
            Protocol::Webdavs => "WebDAV (Secure)",
            Protocol::Webdav => "WebDAV",
            Protocol::Nfs => "NFS",
        }
    }

    /// The port the protocol uses when none is typed.
    pub fn default_port(self) -> u16 {
        match self {
            Protocol::Sftp => 22,
            Protocol::Smb => 445,
            Protocol::Ftp => 21,
            Protocol::Webdavs => 443,
            Protocol::Webdav => 80,
            Protocol::Nfs => 2049,
        }
    }

    /// Whether the protocol keeps what it carries from being read on the way.
    /// FTP, plain WebDAV (HTTP) and NFS do not; SFTP and WebDAV over TLS do;
    /// SMB 3 (what servers speak today) can, and Files does not warn about it.
    pub fn encrypted(self) -> bool {
        matches!(self, Protocol::Sftp | Protocol::Smb | Protocol::Webdavs)
    }
}

/// What the window says about a location that is not encrypted.
pub const NOT_ENCRYPTED: &str = "Not encrypted";

/// "Not encrypted" for a location on a server that does not protect what it
/// carries (`ftp:`, `webdav:`, `nfs:`, plain `http:`), nothing for the rest.
pub fn security_note(url: &str) -> Option<&'static str> {
    let colon = url.find(':')?;
    let scheme = &url[..colon];
    let plain = matches!(
        scheme.to_ascii_lowercase().as_str(),
        "ftp" | "webdav" | "dav" | "nfs" | "http"
    );
    plain.then_some(NOT_ENCRYPTED)
}

fn plain_text(s: &str) -> bool {
    !s.chars().any(|c| c.is_control() || is_hidden_char(c))
}

/// Percent-encodes everything but letters, digits and `-._~`.
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(char::from(b"0123456789ABCDEF"[usize::from(b >> 4)]));
            out.push(char::from(b"0123456789ABCDEF"[usize::from(b & 15)]));
        }
    }
    out
}

fn decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let h = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(h, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// A checked server: the host as it goes into the address (an IPv6 address in
/// brackets) and the port, if one was typed.
#[derive(Debug, PartialEq, Eq)]
struct Host {
    host: String,
    port: Option<u16>,
}

fn host_name_ok(host: &str) -> bool {
    // One trailing dot (a fully qualified name) is fine.
    let host = host.strip_suffix('.').unwrap_or(host);
    if host.is_empty() || host.len() > MAX_SERVER {
        return false;
    }
    host.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            // Letters of any language, digits, hyphens and (for NetBIOS names) underscores.
            && label
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    })
}

fn parse_port(text: &str) -> Result<u16, Refused> {
    if text.is_empty() || text.len() > 5 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err("The port must be a number from 1 to 65535.");
    }
    match text.parse::<u32>() {
        Ok(p) if (1..=65535).contains(&p) => Ok(p as u16),
        _ => Err("The port must be a number from 1 to 65535."),
    }
}

/// `name`, `name:port`, `1.2.3.4`, `[::1]` or `[::1]:port`.
fn check_server(text: &str) -> Result<Host, Refused> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Type the name or address of the server.");
    }
    if text.len() > MAX_SERVER + 6 {
        return Err("That server name is too long.");
    }
    if !plain_text(text) {
        return Err("The server name has control or text-direction characters.");
    }
    if text.contains("://") {
        return Err("Choose the protocol in the list and type only the server's name.");
    }
    if text.contains([
        '@', '/', '\\', '?', '#', '%', ' ', '"', '\'', '<', '>', '|', '`',
    ]) {
        return Err(
            "Type only the server's name or address here. The user name and the folder have their own fields.",
        );
    }
    if let Some(rest) = text.strip_prefix('[') {
        // An IPv6 address.
        let Some((addr, after)) = rest.split_once(']') else {
            return Err("That server address isn't valid.");
        };
        if addr.is_empty()
            || addr.len() > 45
            || !addr.contains(':')
            || !addr
                .chars()
                .all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.')
        {
            return Err("That server address isn't valid.");
        }
        let port = match after {
            "" => None,
            p => Some(parse_port(
                p.strip_prefix(':')
                    .ok_or("That server address isn't valid.")?,
            )?),
        };
        return Ok(Host {
            host: format!("[{addr}]"),
            port,
        });
    }
    let (name, port) = match text.split_once(':') {
        None => (text, None),
        Some((n, p)) => {
            if p.contains(':') {
                return Err("Put an IPv6 address in square brackets, like [::1].");
            }
            (n, Some(parse_port(p)?))
        }
    };
    if !host_name_ok(name) {
        return Err("That server name isn't valid.");
    }
    Ok(Host {
        host: name.to_string(),
        port,
    })
}

fn check_user(text: &str) -> Result<String, Refused> {
    let text = text.trim();
    if text.len() > MAX_USER {
        return Err("That user name is too long.");
    }
    if !plain_text(text) {
        return Err("The user name has control or text-direction characters.");
    }
    if text.contains(':') {
        return Err("Don't type a password here. Files asks for it when you connect.");
    }
    if text.contains('/') {
        return Err("A user name can't hold a slash.");
    }
    // A program a worker starts could read it as one of its own options.
    if text.starts_with('-') {
        return Err("A user name can't start with a dash.");
    }
    Ok(text.to_string())
}

/// The folder as clean path segments: empty parts and `.` dropped, `..` goes
/// up but never above the top.
fn check_folder(text: &str) -> Result<Vec<String>, Refused> {
    let text = text.trim();
    // (one more than MAX_FOLDER: reading an address back gives the folder a
    // leading slash, and what was built must be what is read)
    if text.len() > MAX_FOLDER + 1 {
        return Err("That folder is too long.");
    }
    if !plain_text(text) {
        return Err("The folder has control or text-direction characters.");
    }
    let mut parts: Vec<&str> = Vec::new();
    for p in text.split(['/', '\\']) {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    Ok(parts.into_iter().map(encode).collect())
}

/// The address of a server, from what Connect to Server holds: the protocol,
/// the server (`name`, `name:port`, an IPv4 address or an IPv6 one in brackets),
/// the folder to open (may be empty) and the user (may be empty). The result
/// never holds a password.
pub fn build(
    protocol: Protocol,
    server: &str,
    folder: &str,
    user: &str,
) -> Result<String, Refused> {
    let host = check_server(server)?;
    let user = check_user(user)?;
    let parts = check_folder(folder)?;
    let mut url = String::with_capacity(64);
    url.push_str(protocol.scheme());
    url.push_str("://");
    if !user.is_empty() {
        url.push_str(&encode(&user));
        url.push('@');
    }
    url.push_str(&host.host.to_ascii_lowercase());
    if let Some(p) = host.port.filter(|p| *p != protocol.default_port()) {
        url.push(':');
        url.push_str(&p.to_string());
    }
    if parts.is_empty() {
        // SFTP with no folder at all opens the user's home folder; a folder
        // that is only a slash is the server's top, as for the others.
        if protocol != Protocol::Sftp || !folder.trim().is_empty() {
            url.push('/');
        }
    } else {
        for p in &parts {
            url.push('/');
            url.push_str(p);
        }
    }
    // An address longer than the list keeps (`parse_url` reads no more) is
    // not built: it would not come back as the same address.
    if url.len() > MAX_URL {
        return Err("That address is too long.");
    }
    Ok(url)
}

/// What an address says, in the fields of Connect to Server.
#[derive(Debug, PartialEq, Eq)]
pub struct Parts {
    pub protocol: Protocol,
    /// `name` or `name:port`.
    pub server: String,
    pub folder: String,
    pub user: String,
}

/// Takes an address apart, refusing one Connect to Server could not have
/// built: another scheme, a password, a query or fragment, an odd host.
pub fn parse_url(url: &str) -> Option<Parts> {
    if url.is_empty() || url.len() > MAX_URL || !plain_text(url) {
        return None;
    }
    let colon = url.find(':')?;
    let protocol = Protocol::from_scheme(&url[..colon])?;
    let rest = url[colon + 1..].strip_prefix("//")?;
    if rest.contains(['?', '#']) {
        return None;
    }
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let (user, hostport) = match authority.rsplit_once('@') {
        Some((u, h)) => {
            if u.contains(':') {
                // A password: never kept.
                return None;
            }
            (decode(u)?, h)
        }
        None => (String::new(), authority),
    };
    let host = check_server(hostport).ok()?;
    let user = check_user(&user).ok()?;
    let mut folder = String::new();
    for seg in path.split('/').filter(|s| !s.is_empty()) {
        folder.push('/');
        folder.push_str(&decode(seg)?);
    }
    // A lone slash is the server's top, which for SFTP differs from no folder.
    if folder.is_empty() && path.starts_with('/') {
        folder.push('/');
    }
    // `build` trims the folder it is given, as it trims what is typed: a last
    // name that ends in a space would come back as another folder. A slash
    // after it keeps the space (`/dir /`), and builds the same address.
    if folder.trim() != folder {
        folder.push('/');
    }
    let server = match host.port {
        Some(p) => format!("{}:{p}", host.host),
        None => host.host.clone(),
    };
    // Only what Connect to Server could have built is an address of ours.
    build(protocol, &server, &folder, &user).ok()?;
    Some(Parts {
        protocol,
        server,
        folder,
        user,
    })
}

/// An address as the recent list keeps it: the one Connect to Server would
/// build for it (so a hand-edited line can't hold anything else), `None` when
/// it is not one of ours.
pub fn clean_recent(url: &str) -> Option<String> {
    let p = parse_url(url.trim())?;
    build(p.protocol, &p.server, &p.folder, &p.user).ok()
}

/// The recent servers, the newest first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Recents {
    urls: Vec<String>,
}

impl Recents {
    /// Reads a saved list (one address per line). Lines that are not one of
    /// our addresses (any with a password among them) are dropped, a server
    /// listed twice keeps its first line, and only [`MAX_RECENT`] are kept.
    pub fn parse(text: &str) -> Recents {
        let mut urls: Vec<String> = Vec::new();
        for line in text.lines() {
            if urls.len() >= MAX_RECENT {
                break;
            }
            if let Some(u) = clean_recent(line)
                && !urls.contains(&u)
            {
                urls.push(u);
            }
        }
        Recents { urls }
    }

    pub fn to_text(&self) -> String {
        self.urls.iter().map(|u| format!("{u}\n")).collect()
    }

    pub fn urls(&self) -> &[String] {
        &self.urls
    }

    /// `url` becomes the newest. Returns whether it was kept.
    pub fn push(&mut self, url: &str) -> bool {
        let Some(u) = clean_recent(url) else {
            return false;
        };
        self.urls.retain(|x| *x != u);
        self.urls.insert(0, u);
        self.urls.truncate(MAX_RECENT);
        true
    }

    pub fn remove(&mut self, url: &str) -> bool {
        let before = self.urls.len();
        self.urls.retain(|x| x != url);
        self.urls.len() != before
    }

    pub fn clear(&mut self) {
        self.urls.clear();
    }
}

/// How a recent server is listed: the address without its scheme's slashes
/// being decoded (names are made safe to show by the caller).
pub fn recent_label(url: &str) -> String {
    match parse_url(url) {
        Some(p) => {
            let mut s = format!("{}://", p.protocol.scheme());
            if !p.user.is_empty() {
                s.push_str(&p.user);
                s.push('@');
            }
            s.push_str(&p.server);
            if p.folder != "/" {
                s.push_str(&p.folder);
            }
            s
        }
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_user_name_cannot_read_as_an_option() {
        assert!(build(Protocol::Sftp, "h", "", "-oProxyCommand=x").is_err());
        assert!(build(Protocol::Sftp, "h", "", "-x").is_err());
        // an address with one is not one Connect to Server could have built
        assert_eq!(parse_url("sftp://-oProxyCommand%3Dx@h/"), None);
        assert_eq!(parse_url("sftp://%2Dx@h/"), None);
        assert!(build(Protocol::Sftp, "h", "", "a-b").is_ok());
    }

    #[test]
    fn an_address_too_long_to_read_back_is_not_built() {
        // found by fuzzing: 900 bytes of odd text encode to more than MAX_URL
        let folder = "\u{FFFD}".repeat(300);
        assert_eq!(
            build(Protocol::Sftp, "z", &folder, ""),
            Err("That address is too long.")
        );
        let ok = build(Protocol::Sftp, "z", &"a".repeat(MAX_FOLDER - 10), "").unwrap();
        assert!(parse_url(&ok).is_some());
    }

    #[test]
    fn a_folder_ending_in_a_space_is_read_back_as_the_same_folder() {
        // found by fuzzing: `build` trims, so the address read back and built
        // again named another folder
        let url = "sftp://z/dir%20";
        let p = parse_url(url).unwrap();
        assert_eq!(
            build(p.protocol, &p.server, &p.folder, &p.user).unwrap(),
            url
        );
        assert_eq!(clean_recent(url).as_deref(), Some(url));
        // a space inside is a name like any other, and survives the round trip
        let ok = build(Protocol::Sftp, "z", "/a b/c d", "").unwrap();
        let p = parse_url(&ok).unwrap();
        assert_eq!(
            build(p.protocol, &p.server, &p.folder, &p.user).unwrap(),
            ok
        );
    }

    fn b(p: Protocol, server: &str, folder: &str, user: &str) -> Result<String, Refused> {
        build(p, server, folder, user)
    }

    #[test]
    fn builds_the_address_of_each_protocol() {
        assert_eq!(
            b(Protocol::Sftp, "nas.lan", "", "").unwrap(),
            "sftp://nas.lan"
        );
        assert_eq!(
            b(Protocol::Sftp, "nas.lan", "/srv/data", "me").unwrap(),
            "sftp://me@nas.lan/srv/data"
        );
        assert_eq!(
            b(Protocol::Smb, "NAS", "share/Photos", "").unwrap(),
            "smb://nas/share/Photos"
        );
        assert_eq!(b(Protocol::Smb, "nas", "", "").unwrap(), "smb://nas/");
        assert_eq!(
            b(Protocol::Ftp, "ftp.example.org", "pub", "anonymous").unwrap(),
            "ftp://anonymous@ftp.example.org/pub"
        );
        assert_eq!(
            b(
                Protocol::Webdavs,
                "cloud.example.org",
                "remote.php/dav",
                "me"
            )
            .unwrap(),
            "webdavs://me@cloud.example.org/remote.php/dav"
        );
        assert_eq!(
            b(Protocol::Webdav, "10.0.0.5", "", "").unwrap(),
            "webdav://10.0.0.5/"
        );
        assert_eq!(
            b(Protocol::Nfs, "nas", "/export/home", "").unwrap(),
            "nfs://nas/export/home"
        );
    }

    #[test]
    fn sftp_slash_is_the_top_and_nothing_is_the_home_folder() {
        assert_eq!(b(Protocol::Sftp, "h", "/", "").unwrap(), "sftp://h/");
        assert_eq!(b(Protocol::Sftp, "h", "a/..", "").unwrap(), "sftp://h/");
        assert_eq!(b(Protocol::Sftp, "h", "  ", "").unwrap(), "sftp://h");
        assert_eq!(clean_recent("sftp://h/").as_deref(), Some("sftp://h/"));
        assert_eq!(clean_recent("sftp://h").as_deref(), Some("sftp://h"));
        assert_eq!(b(Protocol::Sftp, "h", "", "").unwrap(), "sftp://h");
    }

    #[test]
    fn ports_and_ipv6() {
        assert_eq!(
            b(Protocol::Sftp, "host:2222", "/x", "").unwrap(),
            "sftp://host:2222/x"
        );
        // the protocol's own port is left out
        assert_eq!(
            b(Protocol::Sftp, "host:22", "/x", "").unwrap(),
            "sftp://host/x"
        );
        assert_eq!(
            b(Protocol::Webdav, "host:8080", "", "").unwrap(),
            "webdav://host:8080/"
        );
        assert_eq!(
            b(Protocol::Sftp, "[::1]", "/x", "").unwrap(),
            "sftp://[::1]/x"
        );
        assert_eq!(
            b(Protocol::Sftp, "[fe80::1]:2200", "/x", "").unwrap(),
            "sftp://[fe80::1]:2200/x"
        );
        for bad in [
            "host:0",
            "host:65536",
            "host:",
            "host:x",
            "host:99999999999",
            "::1",
            "[::1",
            "[]",
            "[zz::1]",
            "[::1]x",
            "[::1]:",
            "[nocolon]",
        ] {
            assert!(b(Protocol::Sftp, bad, "", "").is_err(), "{bad}");
        }
    }

    #[test]
    fn the_folder_is_cleaned_and_encoded() {
        assert_eq!(
            b(Protocol::Sftp, "h", "a//b/./c/", "").unwrap(),
            "sftp://h/a/b/c"
        );
        assert_eq!(b(Protocol::Sftp, "h", "a/../b", "").unwrap(), "sftp://h/b");
        // `..` can't climb above the top
        assert_eq!(
            b(Protocol::Sftp, "h", "../../etc", "").unwrap(),
            "sftp://h/etc"
        );
        assert_eq!(b(Protocol::Smb, "h", "..", "").unwrap(), "smb://h/");
        assert_eq!(
            b(Protocol::Sftp, "h", "My Files/50%", "").unwrap(),
            "sftp://h/My%20Files/50%25"
        );
        // special characters in a folder name can't start a query or fragment
        assert_eq!(
            b(Protocol::Sftp, "h", "a?b#c", "").unwrap(),
            "sftp://h/a%3Fb%23c"
        );
        assert_eq!(b(Protocol::Sftp, "h", "a\\b", "").unwrap(), "sftp://h/a/b");
        assert_eq!(
            b(Protocol::Sftp, "h", "ünï", "").unwrap(),
            "sftp://h/%C3%BCn%C3%AF"
        );
        assert!(b(Protocol::Sftp, "h", "a\nb", "").is_err());
        assert!(b(Protocol::Sftp, "h", "a\u{202e}b", "").is_err());
        assert!(b(Protocol::Sftp, "h", &"a/".repeat(MAX_FOLDER), "").is_err());
    }

    #[test]
    fn hostile_server_fields_are_refused() {
        let long = "a".repeat(MAX_SERVER + 10);
        let long_label = format!("{}.com", "a".repeat(64));
        for bad in [
            "",
            "   ",
            "user@host",
            "user:pass@host",
            "host/path",
            "host\\share",
            "host?x=1",
            "host#frag",
            "host%2Fx",
            "sftp://host",
            "http://evil",
            "host name",
            "ho\u{0}st",
            "ho\nst",
            "host\u{202e}",
            "-host",
            "host-",
            "a..b",
            ".host",
            "host..",
            "a b",
            "ho<st",
            "ho\"st",
            "host|x",
            "ho`st",
            "ho'st",
            long.as_str(),
            long_label.as_str(),
            "host:22:22",
        ] {
            assert!(b(Protocol::Sftp, bad, "", "").is_err(), "{bad:?}");
        }
        // the message says what to do, never repeats the input
        let e = b(Protocol::Sftp, "user:pass@host", "", "").unwrap_err();
        assert!(!e.contains("pass"));
        // fine ones
        for ok in [
            "h",
            "nas.lan",
            "NAS-01",
            "my_nas",
            "10.0.0.5",
            "bücher.example",
            "host.",
        ] {
            assert!(b(Protocol::Sftp, ok, "", "").is_ok(), "{ok}");
        }
    }

    #[test]
    fn user_names_never_carry_a_password() {
        assert!(b(Protocol::Sftp, "h", "", "me:secret").is_err());
        assert!(b(Protocol::Sftp, "h", "", ":").is_err());
        assert!(b(Protocol::Sftp, "h", "", "a/b").is_err());
        assert!(b(Protocol::Sftp, "h", "", "a\nb").is_err());
        assert!(b(Protocol::Sftp, "h", "", &"u".repeat(MAX_USER + 1)).is_err());
        // an e-mail style or domain user is encoded, so it can't end the user part
        assert_eq!(
            b(Protocol::Smb, "h", "", "me@corp.example").unwrap(),
            "smb://me%40corp.example@h/"
        );
        assert_eq!(
            b(Protocol::Smb, "h", "", "CORP\\me").unwrap(),
            "smb://CORP%5Cme@h/"
        );
        assert_eq!(b(Protocol::Sftp, "h", "", "  me  ").unwrap(), "sftp://me@h");
        for p in PROTOCOLS {
            let u = b(p, "h", "x", "me").unwrap();
            assert!(!u.contains("secret"));
        }
        // the address never has a second `@`, a query or a fragment, whatever the fields hold
        for field in [
            "a@b", "a?b", "a#b", "a:b", "@", "?", "#", "%", "%40", "..", "\u{85}",
        ] {
            for p in PROTOCOLS {
                for u in [
                    b(p, "h", field, ""),
                    b(p, "h", "", field),
                    b(p, field, "", ""),
                ]
                .into_iter()
                .flatten()
                {
                    let rest = u.split_once("://").unwrap().1;
                    let authority = rest.split('/').next().unwrap();
                    assert!(authority.matches('@').count() <= 1, "{u}");
                    assert!(
                        !authority.contains(':')
                            || authority
                                .rsplit(':')
                                .next()
                                .unwrap()
                                .bytes()
                                .all(|b| b.is_ascii_digit()),
                        "{u}"
                    );
                    assert!(!u.contains(['?', '#', ' ']), "{u}");
                }
            }
        }
    }

    #[test]
    fn which_locations_are_not_encrypted() {
        assert_eq!(security_note("ftp://h/x"), Some(NOT_ENCRYPTED));
        assert_eq!(security_note("FTP://h/x"), Some(NOT_ENCRYPTED));
        assert_eq!(security_note("webdav://h/"), Some(NOT_ENCRYPTED));
        assert_eq!(security_note("nfs://h/x"), Some(NOT_ENCRYPTED));
        assert_eq!(security_note("sftp://h/x"), None);
        assert_eq!(security_note("webdavs://h/"), None);
        assert_eq!(security_note("smb://h/s"), None);
        assert_eq!(security_note("file:///x"), None);
        assert_eq!(security_note("trash:/"), None);
        assert_eq!(security_note("no colon"), None);
        assert!(!Protocol::Ftp.encrypted());
        assert!(!Protocol::Webdav.encrypted());
        assert!(!Protocol::Nfs.encrypted());
        assert!(Protocol::Sftp.encrypted());
        assert!(Protocol::Webdavs.encrypted());
        assert!(Protocol::Smb.encrypted());
        // what the dialog says and what the window says agree for every protocol
        for p in PROTOCOLS {
            let u = b(p, "h", "", "").unwrap();
            assert_eq!(security_note(&u).is_none(), p.encrypted(), "{u}");
        }
    }

    #[test]
    fn addresses_come_apart_and_back() {
        for (p, server, folder, user) in [
            (Protocol::Sftp, "nas.lan", "/srv/My Files", "me"),
            (Protocol::Smb, "nas", "/share/a b", ""),
            (Protocol::Webdav, "host:8080", "/dav", "u@x"),
            (Protocol::Sftp, "[::1]:2200", "", ""),
            (Protocol::Ftp, "ftp.example.org", "/pub", "anonymous"),
            (Protocol::Nfs, "nas", "/export", ""),
        ] {
            let url = b(p, server, folder, user).unwrap();
            let parts = parse_url(&url).unwrap_or_else(|| panic!("{url}"));
            assert_eq!(parts.protocol, p, "{url}");
            assert_eq!(parts.server, server, "{url}");
            assert_eq!(parts.folder, folder, "{url}");
            assert_eq!(parts.user, user, "{url}");
            assert_eq!(clean_recent(&url).as_deref(), Some(url.as_str()));
        }
        for bad in [
            "sftp://me:secret@h/x",
            "sftp://:@h/x",
            "sftp://h/x?y",
            "sftp://h/x#y",
            "file:///x",
            "http://h/",
            "trash:/",
            "sftp:h",
            "sftp://",
            "sftp://h/%zz",
            "sftp://us%00er@h/",
            "",
        ] {
            assert!(parse_url(bad).is_none(), "{bad:?}");
            assert!(clean_recent(bad).is_none(), "{bad:?}");
        }
        // an address that climbs is kept as the folder it comes to
        assert_eq!(
            clean_recent("sftp://h/..%2f..").as_deref(),
            Some("sftp://h/")
        );
        assert!(parse_url("sftp://h/x\n").is_none());
        assert!(parse_url("sftp://h/x\ty").is_none());
        // the scheme and host are case-insensitive; the list keeps them lower-case
        assert_eq!(
            clean_recent("SFTP://Me@NAS/x").as_deref(),
            Some("sftp://Me@nas/x")
        );
    }

    #[test]
    fn recents_are_bounded_unique_and_hold_no_password() {
        let mut r = Recents::default();
        for i in 0..MAX_RECENT + 5 {
            assert!(r.push(&format!("sftp://h{i}/")));
        }
        assert_eq!(r.urls().len(), MAX_RECENT);
        assert_eq!(r.urls()[0], format!("sftp://h{}/", MAX_RECENT + 4));
        // the same server again moves to the front, once
        assert!(r.push("sftp://h7"));
        assert_eq!(r.urls()[0], "sftp://h7");
        assert_eq!(r.urls().iter().filter(|u| *u == "sftp://h7").count(), 1);
        assert!(!r.push("sftp://me:secret@h/x"));
        assert!(!r.push("smb://h/%00"));
        assert!(!r.to_text().contains("secret"));
        // reading a file that was edited by hand
        let text = "sftp://a/x\nsftp://u:pw@b/x\nnonsense\nftp://c/\nsftp://a/x\nsmb://d/s\n";
        let r = Recents::parse(text);
        assert_eq!(r.urls(), ["sftp://a/x", "ftp://c/", "smb://d/s"]);
        assert!(!r.to_text().contains("pw"));
        let mut r = r;
        assert!(r.remove("ftp://c/"));
        assert!(!r.remove("ftp://c/"));
        r.clear();
        assert!(r.urls().is_empty());
        let many: String = (0..100).map(|i| format!("sftp://m{i}/\n")).collect();
        assert_eq!(Recents::parse(&many).urls().len(), MAX_RECENT);
    }

    #[test]
    fn recents_are_listed_by_what_they_say() {
        assert_eq!(
            recent_label("sftp://me@nas/srv/My%20Files"),
            "sftp://me@nas/srv/My Files"
        );
        assert_eq!(recent_label("smb://nas/"), "smb://nas");
        assert_eq!(recent_label("garbage"), "");
    }
}
