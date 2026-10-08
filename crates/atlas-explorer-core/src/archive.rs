//! Archives opened as folders, and the rules that keep what is taken out of
//! one inside the folder it goes to. Pure data in, data out, so it is tested
//! without a display; the window lists archives through KIO's archive worker
//! (kio-extras: `zip:`, `tar:`, `sevenz:`, `ar:`) and asks this before it lets
//! KIO copy anything out. See docs/DESIGN.md, "Archives".
//!
//! What is inside an archive is untrusted: names, links and sizes come from
//! whoever made the file. The rules mirror Telamon Archive's own
//! (`core::path` in its repo): an entry whose path is absolute, holds `..`
//! (split on `/` and on `\`, which DOS and Windows archives use), holds a NUL,
//! or is longer than 256 components or 4,096 bytes is refused. A link is
//! refused when its target is absolute or holds `..`: such a link can only
//! point down or sideways inside the extracted folder, so no chain of links
//! can lead out of it (a link to `.` followed by `x/..` is the reason `..` is
//! refused outright, not resolved).

use crate::display::display_name;
use crate::optext::short_name;

/// The schemes of kio-extras' archive worker.
pub const SCHEMES: &[&str] = &["zip", "tar", "sevenz", "ar"];

/// Components an entry's path may have.
pub const MAX_COMPONENTS: usize = 256;
/// Bytes an entry's path may have.
pub const MAX_PATH_BYTES: usize = 4096;

/// Whether `scheme` is one of the archive worker's.
pub fn is_scheme(scheme: &str) -> bool {
    SCHEMES.iter().any(|s| s.eq_ignore_ascii_case(scheme))
}

/// Why an entry can't be taken out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    /// The path starts at the root (or at a drive).
    Absolute,
    /// The path holds `..`.
    ParentDir,
    /// The path holds a NUL.
    Nul,
    /// More than [`MAX_COMPONENTS`] components.
    TooDeep,
    /// More than [`MAX_PATH_BYTES`] bytes.
    TooLong,
    /// A link to an absolute path.
    LinkAbsolute,
    /// A link whose target holds `..`.
    LinkParentDir,
    /// A link whose target holds a NUL.
    LinkNul,
}

impl Problem {
    /// The problem is with a link's target, not the entry's own path.
    pub fn is_link(self) -> bool {
        matches!(
            self,
            Problem::LinkAbsolute | Problem::LinkParentDir | Problem::LinkNul
        )
    }
}

fn components(path: &[u8]) -> impl Iterator<Item = &[u8]> {
    path.split(|&b| b == b'/' || b == b'\\')
        .filter(|c| !c.is_empty() && *c != b".")
}

/// Whether `path` (as an archive lists it, relative to the archive's top) can
/// be taken out below a folder without leaving it.
pub fn check_path(path: &[u8]) -> Result<(), Problem> {
    if path.contains(&0) {
        return Err(Problem::Nul);
    }
    if path.len() > MAX_PATH_BYTES {
        return Err(Problem::TooLong);
    }
    if path.first().is_some_and(|&b| b == b'/' || b == b'\\') {
        return Err(Problem::Absolute);
    }
    // A drive prefix: `C:` before anything else.
    if path.len() >= 2 && path[0].is_ascii_alphabetic() && path[1] == b':' {
        let rest = &path[2..];
        if rest.is_empty() || rest[0] == b'/' || rest[0] == b'\\' {
            return Err(Problem::Absolute);
        }
    }
    let mut n = 0usize;
    for c in components(path) {
        if c == b".." {
            return Err(Problem::ParentDir);
        }
        n += 1;
        if n > MAX_COMPONENTS {
            return Err(Problem::TooDeep);
        }
    }
    Ok(())
}

/// Whether a link with this target can be made in the extracted folder.
pub fn check_link(target: &[u8]) -> Result<(), Problem> {
    if target.contains(&0) {
        return Err(Problem::LinkNul);
    }
    if target.first().is_some_and(|&b| b == b'/' || b == b'\\') {
        return Err(Problem::LinkAbsolute);
    }
    if components(target).any(|c| c == b"..") {
        return Err(Problem::LinkParentDir);
    }
    Ok(())
}

/// One thing listed in an archive.
#[derive(Debug, Clone, Copy)]
pub struct Entry<'a> {
    /// Its path, relative to what is extracted.
    pub path: &'a [u8],
    /// Where it points, for a symbolic link.
    pub link: Option<&'a [u8]>,
}

/// What looking at a whole listing found.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Report {
    /// Entries looked at.
    pub checked: usize,
    /// Entries that can't be taken out.
    pub problems: usize,
    /// The first of them: its path and why.
    pub first: Option<(Vec<u8>, Problem)>,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.problems == 0
    }
}

/// Checks every entry (the path first, then the link).
pub fn check_entries<'a>(entries: impl IntoIterator<Item = Entry<'a>>) -> Report {
    let mut report = Report::default();
    for e in entries {
        report.checked += 1;
        let bad = check_path(e.path)
            .err()
            .or_else(|| e.link.and_then(|l| check_link(l).err()));
        if let Some(p) = bad {
            report.problems += 1;
            if report.first.is_none() {
                report.first = Some((e.path.to_vec(), p));
            }
        }
    }
    report
}

/// The refusal in plain words. `archive_installed`: Telamon Archive can be
/// named as the way to get the rest out.
pub fn refusal_text(report: &Report, archive_installed: bool) -> String {
    let Some((path, problem)) = &report.first else {
        return String::new();
    };
    let name = short_name(&display_name(path.as_slice()));
    let what = if problem.is_link() {
        format!("\"{name}\" is a link that points outside the folder you chose")
    } else {
        format!("\"{name}\" would be extracted outside the folder you chose")
    };
    let more = match report.problems {
        0 | 1 => String::new(),
        n => format!(" ({} more like it)", n - 1),
    };
    let tail = if archive_installed {
        " Telamon Archive can extract the rest safely."
    } else {
        ""
    };
    format!("{what}{more}, so Files won't extract this archive.{tail}")
}

/// Reads the records the window sends: for each entry a `f` and its path
/// then a NUL, or an `l`, its path, a NUL, the link's target and a NUL.
/// Anything that doesn't parse ends the list and is counted as a bad entry
/// (an empty path with a NUL in it) so the check refuses rather than skips.
pub fn parse_records(buf: &[u8]) -> Vec<Entry<'_>> {
    let mut out = Vec::new();
    let mut rest = buf;
    while let Some((&kind, tail)) = rest.split_first() {
        let Some(end) = tail.iter().position(|&b| b == 0) else {
            out.push(Entry {
                path: b"\0",
                link: None,
            });
            break;
        };
        let path = &tail[..end];
        rest = &tail[end + 1..];
        match kind {
            b'f' => out.push(Entry { path, link: None }),
            b'l' => {
                let Some(end) = rest.iter().position(|&b| b == 0) else {
                    out.push(Entry {
                        path: b"\0",
                        link: None,
                    });
                    break;
                };
                out.push(Entry {
                    path,
                    link: Some(&rest[..end]),
                });
                rest = &rest[end + 1..];
            }
            _ => {
                out.push(Entry {
                    path: b"\0",
                    link: None,
                });
                break;
            }
        }
    }
    out
}

// ---- Where an archive is ----

/// File name endings of archives KIO's worker opens, lower case.
const ARCHIVE_ENDINGS: &[&str] = &[
    ".zip",
    ".tar",
    ".tar.gz",
    ".tgz",
    ".tar.bz2",
    ".tbz",
    ".tbz2",
    ".tar.xz",
    ".txz",
    ".tar.zst",
    ".tzst",
    ".tar.lz",
    ".tar.lzma",
    ".tar.z",
    ".7z",
    ".ar",
    ".deb",
    ".jar",
    ".cbz",
];

fn decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(hex) = b.get(i + 1..i + 3)
            && let Ok(hex) = std::str::from_utf8(hex)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

/// Whether a file name (decoded) ends like an archive.
pub fn looks_like_archive(name: &[u8]) -> bool {
    let lower = String::from_utf8_lossy(name).to_lowercase();
    ARCHIVE_ENDINGS.iter().any(|e| lower.ends_with(e))
}

/// Where a location inside an archive splits: the archive file and the path
/// inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    /// The archive file, `file:///path/a.zip`.
    pub file_url: String,
    /// The top of the archive as browsed, in the form the location was
    /// written (`zip:///path/a.zip`).
    pub root_url: String,
    /// The path inside, percent-encoded, `""` or `"/sub/dir"`.
    pub inner: String,
    /// The archive file's name, decoded.
    pub name: String,
    /// What to call the folder it is extracted to: the name without its
    /// archive ending (`photos.tar.gz` gives `photos`).
    pub folder_name: String,
}

/// `name` without its archive ending; the name itself when that leaves
/// nothing.
pub fn folder_name_for(name: &str) -> String {
    let lower = name.to_lowercase();
    // The longest ending first, so `.tar.gz` goes whole.
    let cut = ARCHIVE_ENDINGS
        .iter()
        .filter(|e| lower.ends_with(**e))
        .map(|e| e.len())
        .max()
        .or_else(|| name.rfind('.').filter(|&i| i > 0).map(|i| name.len() - i))
        .unwrap_or(0);
    let stem = name.get(..name.len() - cut).unwrap_or(name);
    if stem.is_empty() || stem.chars().all(|c| c == '.') {
        name.to_string()
    } else {
        stem.to_string()
    }
}

/// Splits `url` (`zip:/path/a.zip/sub`, `tar:///path/a.tar.gz`) into the
/// archive file and the path in it. The archive is the first part of the path
/// whose name ends like an archive (a folder called `x.zip` above the archive
/// is not told apart from one: KIO's own answer would need the disk); with no
/// such part, the whole path is the archive. None for another scheme or a
/// location that names a host.
pub fn locate(url: &str) -> Option<Location> {
    let colon = url.find(':')?;
    let scheme = &url[..colon];
    if !is_scheme(scheme) {
        return None;
    }
    let rest = &url[colon + 1..];
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    let (prefix_len, path) = match rest.strip_prefix("//") {
        Some(r) => {
            let slash = r.find('/')?;
            let authority = &r[..slash];
            if !authority.is_empty() && !authority.eq_ignore_ascii_case("localhost") {
                return None;
            }
            (colon + 1 + 2 + slash, &r[slash..])
        }
        None => (colon + 1, rest),
    };
    if !path.starts_with('/') {
        return None;
    }
    // (start, end) of each part of the path, in `path`.
    let mut parts: Vec<(usize, usize)> = Vec::new();
    let mut at = 0;
    for p in path.split('/') {
        if !p.is_empty() {
            parts.push((at, at + p.len()));
        }
        at += p.len() + 1;
    }
    if parts.is_empty() {
        return None;
    }
    let archive = parts
        .iter()
        .position(|&(s, e)| looks_like_archive(&decode(&path[s..e])))
        .unwrap_or(parts.len() - 1);
    let archive_end = parts[archive].1;
    let inner_parts = &parts[archive + 1..];
    let inner: String = inner_parts
        .iter()
        .map(|&(s, e)| format!("/{}", &path[s..e]))
        .collect();
    let (name_start, _) = parts[archive];
    let name = String::from_utf8_lossy(&decode(&path[name_start..archive_end])).into_owned();
    Some(Location {
        file_url: format!("file://{}", &path[..archive_end]),
        root_url: format!("{}{}", &url[..prefix_len], &path[..archive_end]),
        inner,
        folder_name: folder_name_for(&name),
        name,
    })
}

/// Where Up goes from a location in an archive: the folder inside it, or at
/// the top the folder the archive file is in. None when `url` isn't one.
pub fn parent(url: &str) -> Option<String> {
    let loc = locate(url)?;
    if !loc.inner.is_empty() {
        let cut = loc.inner.rfind('/')?;
        return Some(format!("{}{}", loc.root_url, &loc.inner[..cut]));
    }
    let path = loc.file_url.strip_prefix("file://")?;
    let cut = path.rfind('/')?;
    let dir = if cut == 0 { "/" } else { &path[..cut] };
    Some(format!("file://{dir}"))
}

/// The name of the archive file in a location in it, as a display name.
pub fn archive_name(url: &str) -> Option<String> {
    let loc = locate(url)?;
    let path = loc.file_url.strip_prefix("file://")?;
    let name = path.rsplit('/').next()?;
    Some(display_name(decode(name).as_slice()))
}

// ---- Zip files that need a password ----
//
// KIO's archive worker (KArchive) can't decrypt: it lists the names of an
// encrypted zip and then hands out the encrypted bytes as if they were the
// files. So Files looks at the zip's central directory (the list at its end)
// for the "encrypted" flag before it opens or copies out of one, and says
// the archive needs a password. Only flags are read, never file data.

/// Bytes at the end of a zip a search for its end record covers (the record
/// is 22 bytes and a comment of up to 65,535 bytes follows it).
pub const ZIP_TAIL: usize = 22 + 65_535;
/// The largest central directory that is read to look for encryption.
pub const ZIP_MAX_DIRECTORY: u64 = 16 * 1024 * 1024;

/// Where a zip's central directory is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZipDirectory {
    pub offset: u64,
    pub size: u64,
    pub entries: u64,
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Finds the end record in `tail` (the last bytes of a file `file_len` long)
/// and says where the central directory is. None when it is not a zip, or a
/// zip64 one (whose record is elsewhere): the caller then can't tell.
pub fn zip_directory(tail: &[u8], file_len: u64) -> Option<ZipDirectory> {
    let mut i = tail.len().checked_sub(22)?;
    loop {
        if tail.get(i..i + 4)? == b"PK\x05\x06" {
            let comment = u64::from(u16_at(tail, i + 20)?);
            // The comment must end the file, or this is bytes inside one.
            if i as u64 + 22 + comment == tail.len() as u64 {
                let entries = u64::from(u16_at(tail, i + 10)?);
                let size = u64::from(u32_at(tail, i + 12)?);
                let offset = u64::from(u32_at(tail, i + 16)?);
                if entries == 0xFFFF || size == 0xFFFF_FFFF || offset == 0xFFFF_FFFF {
                    return None;
                }
                if offset.checked_add(size)? > file_len {
                    return None;
                }
                return Some(ZipDirectory {
                    offset,
                    size,
                    entries,
                });
            }
        }
        i = i.checked_sub(1)?;
    }
}

/// Whether any entry of a central directory is marked encrypted (the
/// general purpose flag's bit 0, or bit 6 for strong encryption).
pub fn zip_directory_encrypted(directory: &[u8]) -> bool {
    let mut at = 0usize;
    while directory.get(at..at + 4) == Some(b"PK\x01\x02") {
        let Some(flags) = u16_at(directory, at + 8) else {
            return false;
        };
        if flags & 0x0041 != 0 {
            return true;
        }
        let (Some(name), Some(extra), Some(comment)) = (
            u16_at(directory, at + 28),
            u16_at(directory, at + 30),
            u16_at(directory, at + 32),
        ) else {
            return false;
        };
        at += 46 + usize::from(name) + usize::from(extra) + usize::from(comment);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e<'a>(path: &'a str, link: Option<&'a str>) -> Entry<'a> {
        Entry {
            path: path.as_bytes(),
            link: link.map(str::as_bytes),
        }
    }

    #[test]
    fn plain_paths_pass() {
        for p in [
            "a.txt",
            "dir/a.txt",
            "dir/sub/deeper/file",
            "./a.txt",
            "dir//a",
            "..name",
            "name..",
            "a...b/c",
            "...",
            ".hidden/x",
            "C:file",
            "a b/ünï/c",
        ] {
            assert_eq!(check_path(p.as_bytes()), Ok(()), "{p}");
        }
    }

    #[test]
    fn zip_slip_is_refused() {
        for (p, why) in [
            ("../evil.txt", Problem::ParentDir),
            ("../../evil.txt", Problem::ParentDir),
            ("sub/../../evil.txt", Problem::ParentDir),
            ("sub/..", Problem::ParentDir),
            ("..", Problem::ParentDir),
            ("a/./../b", Problem::ParentDir),
            ("..\\evil.txt", Problem::ParentDir),
            ("sub\\..\\..\\evil.txt", Problem::ParentDir),
            ("/etc/passwd", Problem::Absolute),
            ("/", Problem::Absolute),
            ("\\windows\\evil", Problem::Absolute),
            ("C:\\evil", Problem::Absolute),
            ("C:/evil", Problem::Absolute),
            ("c:", Problem::Absolute),
        ] {
            assert_eq!(check_path(p.as_bytes()), Err(why), "{p}");
        }
    }

    #[test]
    fn nul_and_size_limits() {
        assert_eq!(check_path(b"a\0b"), Err(Problem::Nul));
        let deep = vec!["d"; MAX_COMPONENTS].join("/");
        assert_eq!(check_path(deep.as_bytes()), Ok(()));
        let deeper = vec!["d"; MAX_COMPONENTS + 1].join("/");
        assert_eq!(check_path(deeper.as_bytes()), Err(Problem::TooDeep));
        // Many `.` parts don't count as components.
        let dots = vec!["."; 1000].join("/");
        assert_eq!(check_path(dots.as_bytes()), Ok(()));
        let long = "a".repeat(MAX_PATH_BYTES + 1);
        assert_eq!(check_path(long.as_bytes()), Err(Problem::TooLong));
        assert_eq!(check_path("a".repeat(MAX_PATH_BYTES).as_bytes()), Ok(()));
        // Not UTF-8 is fine: it is only shown as hex.
        assert_eq!(check_path(b"caf\xe9/x"), Ok(()));
    }

    #[test]
    fn links_must_point_down_or_sideways() {
        for t in ["file", "dir/file", "./file", ".", "sub/deeper", "a..b"] {
            assert_eq!(check_link(t.as_bytes()), Ok(()), "{t}");
        }
        for (t, why) in [
            ("../x", Problem::LinkParentDir),
            ("../..", Problem::LinkParentDir),
            ("a/../b", Problem::LinkParentDir),
            ("a/..", Problem::LinkParentDir),
            ("..\\x", Problem::LinkParentDir),
            ("/etc", Problem::LinkAbsolute),
            ("/", Problem::LinkAbsolute),
            ("\\x", Problem::LinkAbsolute),
            ("a\0b", Problem::LinkNul),
        ] {
            assert_eq!(check_link(t.as_bytes()), Err(why), "{t}");
        }
    }

    #[test]
    fn a_link_to_dot_then_dotdot_is_caught_by_refusing_dotdot() {
        // `p -> .` is allowed; `p/q/l -> ../..` would climb out of the folder
        // once `p` is followed. Refusing every `..` is what stops it.
        let report = check_entries([e("p", Some(".")), e("p/q/l", Some("../.."))]);
        assert_eq!(report.problems, 1);
        assert_eq!(
            report.first,
            Some((b"p/q/l".to_vec(), Problem::LinkParentDir))
        );
    }

    #[test]
    fn a_listing_is_checked_whole() {
        let ok = check_entries([e("a", None), e("d/b", None), e("l", Some("a"))]);
        assert!(ok.ok());
        assert_eq!(ok.checked, 3);
        let bad = check_entries([
            e("a", None),
            e("../x", None),
            e("l", Some("/etc")),
            e("c", None),
        ]);
        assert_eq!(bad.checked, 4);
        assert_eq!(bad.problems, 2);
        assert_eq!(bad.first, Some((b"../x".to_vec(), Problem::ParentDir)));
        assert!(check_entries([]).ok());
    }

    #[test]
    fn refusal_says_what_and_names_the_file() {
        let bad = check_entries([e("../../evil.txt", None)]);
        let t = refusal_text(&bad, false);
        assert!(t.contains("evil.txt"), "{t}");
        assert!(t.contains("outside the folder"), "{t}");
        assert!(!t.contains("Telamon Archive"), "{t}");
        assert!(refusal_text(&bad, true).contains("Telamon Archive"));
        let link = check_entries([e("l", Some("/etc")), e("m", Some("../x"))]);
        let t = refusal_text(&link, false);
        assert!(t.contains("a link"), "{t}");
        assert!(t.contains("1 more"), "{t}");
        assert_eq!(refusal_text(&Report::default(), true), "");
    }

    #[test]
    fn a_hostile_name_is_made_safe_in_the_refusal() {
        let bad = check_entries([e("../a\nb\u{202e}c", None)]);
        let t = refusal_text(&bad, false);
        assert!(!t.contains('\n') && !t.contains('\u{202e}'), "{t:?}");
        let long = format!("../{}", "x".repeat(500));
        assert!(refusal_text(&check_entries([e(&long, None)]), false).len() < 400);
    }

    #[test]
    fn records_round_trip_and_bad_ones_refuse() {
        let buf = b"fa.txt\0lalias\0a.txt\0fd/b\0";
        let list = parse_records(buf);
        assert_eq!(list.len(), 3);
        assert_eq!(list[1].link, Some(&b"a.txt"[..]));
        assert!(check_entries(list).ok());
        // A cut-off record, or an unknown kind, is a problem, never skipped.
        for bad in [&b"fa.txt"[..], b"lx\0y", b"zq\0"] {
            let report = check_entries(parse_records(bad));
            assert!(!report.ok(), "{bad:?}");
        }
        assert!(parse_records(b"").is_empty());
    }

    #[test]
    fn locating_the_archive_in_a_location() {
        let l = locate("zip:/home/u/Docs/a.zip").unwrap();
        assert_eq!(l.file_url, "file:///home/u/Docs/a.zip");
        assert_eq!(l.root_url, "zip:/home/u/Docs/a.zip");
        assert_eq!(l.inner, "");

        let l = locate("zip:///home/u/a%20b.zip/sub/dir").unwrap();
        assert_eq!(l.file_url, "file:///home/u/a%20b.zip");
        assert_eq!(l.root_url, "zip:///home/u/a%20b.zip");
        assert_eq!(l.inner, "/sub/dir");

        let l = locate("tar:/x/y.TAR.GZ/").unwrap();
        assert_eq!(l.file_url, "file:///x/y.TAR.GZ");
        assert_eq!(l.inner, "");

        // The first archive wins: a nested one is a folder of the outer.
        let l = locate("zip:/x/outer.zip/inner.tar.gz/f").unwrap();
        assert_eq!(l.file_url, "file:///x/outer.zip");
        assert_eq!(l.inner, "/inner.tar.gz/f");

        // No known ending: the whole path is the archive.
        let l = locate("sevenz:/x/data.bin").unwrap();
        assert_eq!(l.file_url, "file:///x/data.bin");
        assert_eq!(l.inner, "");

        // Query and fragment are not part of the path.
        assert_eq!(locate("zip:/x/a.zip?x=1#f").unwrap().inner, "");
    }

    #[test]
    fn not_an_archive_location() {
        for u in [
            "file:///x/a.zip",
            "smb://nas/a.zip",
            "zip://host/x/a.zip",
            "zip:",
            "zip:/",
            "zip:x/a.zip",
            "",
            "/x/a.zip",
        ] {
            assert_eq!(locate(u), None, "{u}");
        }
    }

    #[test]
    fn up_goes_to_the_folder_the_archive_is_in() {
        assert_eq!(
            parent("zip:/home/u/a.zip").as_deref(),
            Some("file:///home/u")
        );
        assert_eq!(
            parent("zip:///home/u/a.zip/").as_deref(),
            Some("file:///home/u")
        );
        assert_eq!(parent("zip:/a.zip").as_deref(), Some("file:///"));
        assert_eq!(
            parent("zip:/home/u/a.zip/sub/dir").as_deref(),
            Some("zip:/home/u/a.zip/sub")
        );
        assert_eq!(
            parent("zip:///home/u/a.zip/sub").as_deref(),
            Some("zip:///home/u/a.zip")
        );
        assert_eq!(parent("file:///home"), None);
    }

    #[test]
    fn the_folder_an_archive_extracts_to_is_named_after_it() {
        for (file, folder) in [
            ("photos.zip", "photos"),
            ("photos.tar.gz", "photos"),
            ("Photos.TAR.XZ", "Photos"),
            ("a.b.c.7z", "a.b.c"),
            ("weird name (1).zip", "weird name (1)"),
            ("data.bin", "data"),
            (".zip", ".zip"),
            ("noext", "noext"),
            ("x.tgz", "x"),
        ] {
            assert_eq!(folder_name_for(file), folder, "{file}");
        }
        let l = locate("zip:/x/a%20b.tar.gz/c").unwrap();
        assert_eq!(
            (l.name.as_str(), l.folder_name.as_str()),
            ("a b.tar.gz", "a b")
        );
    }

    #[test]
    fn the_archive_name_is_the_files() {
        assert_eq!(
            archive_name("zip:/x/a%20b.zip/c").as_deref(),
            Some("a b.zip")
        );
        assert_eq!(archive_name("file:///x"), None);
    }

    // A zip with one stored entry per name, the flags of each given.
    fn zip_with(entries: &[(&str, u16)], comment: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut dir = Vec::new();
        for (name, flags) in entries {
            let offset = out.len() as u32;
            out.extend_from_slice(b"PK\x03\x04");
            out.extend_from_slice(&[20, 0]);
            out.extend_from_slice(&flags.to_le_bytes());
            out.extend_from_slice(&[0; 2 + 4 + 4 + 4 + 4]);
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&[0, 0]);
            out.extend_from_slice(name.as_bytes());
            dir.extend_from_slice(b"PK\x01\x02");
            dir.extend_from_slice(&[20, 0, 20, 0]);
            dir.extend_from_slice(&flags.to_le_bytes());
            dir.extend_from_slice(&[0; 2 + 4 + 4 + 4 + 4]);
            dir.extend_from_slice(&(name.len() as u16).to_le_bytes());
            dir.extend_from_slice(&[0; 8]);
            dir.extend_from_slice(&[0; 4]);
            dir.extend_from_slice(&offset.to_le_bytes());
            dir.extend_from_slice(name.as_bytes());
        }
        let dir_offset = out.len() as u32;
        out.extend_from_slice(&dir);
        out.extend_from_slice(b"PK\x05\x06");
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(dir.len() as u32).to_le_bytes());
        out.extend_from_slice(&dir_offset.to_le_bytes());
        out.extend_from_slice(&(comment.len() as u16).to_le_bytes());
        out.extend_from_slice(comment);
        out
    }

    fn encrypted(zip: &[u8]) -> Option<bool> {
        let tail = &zip[zip.len().saturating_sub(ZIP_TAIL)..];
        let d = zip_directory(tail, zip.len() as u64)?;
        let dir = &zip[d.offset as usize..(d.offset + d.size) as usize];
        Some(zip_directory_encrypted(dir))
    }

    #[test]
    fn an_encrypted_zip_is_told_from_a_plain_one() {
        assert_eq!(
            encrypted(&zip_with(&[("a", 0), ("b/c", 0)], b"")),
            Some(false)
        );
        assert_eq!(
            encrypted(&zip_with(&[("a", 0), ("b/c", 1)], b"")),
            Some(true)
        );
        // Strong encryption (bit 6), and the first entry alone.
        assert_eq!(encrypted(&zip_with(&[("a", 0x40)], b"")), Some(true));
        // Other flags (UTF-8 names, data descriptors) are not encryption.
        assert_eq!(encrypted(&zip_with(&[("a", 0x0808)], b"")), Some(false));
        // A zip with no entries is plain.
        assert_eq!(encrypted(&zip_with(&[], b"")), Some(false));
    }

    #[test]
    fn a_comment_does_not_hide_the_end_record() {
        let zip = zip_with(&[("a", 1)], b"PK\x05\x06 not the end, just a comment");
        assert_eq!(encrypted(&zip), Some(true));
        let zip = zip_with(&[("a", 0)], &vec![b'x'; 60_000]);
        assert_eq!(encrypted(&zip), Some(false));
    }

    #[test]
    fn what_is_not_a_zip_is_not_judged() {
        assert_eq!(zip_directory(b"", 0), None);
        assert_eq!(zip_directory(&[0; 100], 100), None);
        assert_eq!(zip_directory(b"PK\x05\x06", 4), None);
        let mut zip = zip_with(&[("a", 1)], b"");
        // A directory that claims to lie past the end of the file.
        let n = zip.len();
        zip[n - 6..n - 2].copy_from_slice(&u32::MAX.to_le_bytes()[..4]);
        assert_eq!(zip_directory(&zip, zip.len() as u64), None);
        // Zip64 markers: the caller can't tell.
        let mut zip = zip_with(&[("a", 1)], b"");
        let n = zip.len();
        zip[n - 12..n - 8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(zip_directory(&zip, zip.len() as u64), None);
    }

    #[test]
    fn a_cut_off_directory_does_not_run_away() {
        let zip = zip_with(&[("name", 0), ("other", 0)], b"");
        let d = zip_directory(&zip, zip.len() as u64).unwrap();
        let dir = &zip[d.offset as usize..(d.offset + d.size) as usize];
        for cut in 0..dir.len() {
            // Whatever is cut off, it ends and says no.
            assert!(!zip_directory_encrypted(&dir[..cut]));
        }
        // A name length that points past the end.
        let mut bad = dir.to_vec();
        bad[28] = 0xFF;
        bad[29] = 0xFF;
        assert!(!zip_directory_encrypted(&bad));
    }

    #[test]
    fn schemes_are_the_workers() {
        assert!(is_scheme("zip") && is_scheme("TAR") && is_scheme("sevenz") && is_scheme("ar"));
        assert!(!is_scheme("file") && !is_scheme("smb") && !is_scheme("7z"));
    }
}
