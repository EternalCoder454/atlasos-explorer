//! `~/.config/telamon-explorer/indexrc`, a plain INI file:
//!
//! ```ini
//! [Index]
//! Roots=~;/mnt/data
//! Exclude=target;*.tmp
//! ```
//!
//! `Roots` (default: the home folder; an empty value turns the index off) and
//! `Exclude` (extra names to skip, or absolute paths) are lists separated by
//! `;`. The file is user input: size and line lengths are capped, unknown
//! sections and keys are ignored, and a bad value is skipped with a warning.

use std::collections::HashSet;
use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

const MAX_FILE: u64 = 64 * 1024;
const MAX_ITEMS: usize = 256;

/// Names never indexed, besides everything starting with a dot.
pub const BUILTIN_EXCLUDES: &[&str] = &["node_modules", "__pycache__"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// Absolute folders to index. Empty means the index is disabled.
    pub roots: Vec<PathBuf>,
    /// Extra excluded names, and absolute paths, from `Exclude=`.
    pub exclude: Vec<OsString>,
}

impl Config {
    pub fn defaults(home: &Path) -> Config {
        Config {
            roots: vec![home.to_path_buf()],
            exclude: Vec::new(),
        }
    }

    /// Read `<config_home>/telamon-explorer/indexrc`. A missing file gives the
    /// defaults; an unreadable one gives the defaults and a warning.
    pub fn load(config_home: &Path, home: &Path) -> Config {
        let path = config_home.join("telamon-explorer").join("indexrc");
        let mut text = read_capped(&path);
        // Before the rename it was `atlas-explorer/indexrc`: read it until the
        // app has moved it (this service can't: it writes only its cache).
        if matches!(text, Ok(None))
            && let Some(old) = atlas_explorer_core::legacy::legacy_of(&path)
        {
            text = read_capped(&old);
        }
        let text = match text {
            Ok(Some(t)) => t,
            Ok(None) => return Config::defaults(home),
            Err(e) => {
                log::warn!("index settings not used: {e}");
                log::debug!(
                    "index settings file: {}",
                    atlas_explorer_core::display::display_name(&path)
                );
                return Config::defaults(home);
            }
        };
        Config::parse(&text, home)
    }

    pub fn parse(text: &str, home: &Path) -> Config {
        let mut cfg = Config::defaults(home);
        let mut in_index = false;
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            if line.len() > 8192 {
                log::warn!("indexrc line {} is too long, ignored", n + 1);
                continue;
            }
            if let Some(sec) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                in_index = sec == "Index";
                continue;
            }
            if !in_index {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key.trim() {
                "Roots" => {
                    let mut roots: Vec<PathBuf> = Vec::new();
                    for item in split_list(value) {
                        match expand(item, home) {
                            Some(p) if !roots.contains(&p) => roots.push(p),
                            Some(_) => {}
                            None => log::warn!(
                                "indexrc: root '{item}' is not an absolute path, ignored"
                            ),
                        }
                    }
                    cfg.roots = roots;
                }
                "Exclude" => {
                    cfg.exclude = split_list(value)
                        .filter_map(|i| {
                            if i.contains('/') || i.starts_with('~') {
                                expand(i, home).map(PathBuf::into_os_string)
                            } else {
                                Some(OsString::from(i))
                            }
                        })
                        .collect();
                }
                _ => {}
            }
        }
        cfg
    }
}

/// `indexrc` text with `Roots=` set to `roots` (absolute folders), every other
/// line kept as it is. The `Roots=` line of `[Index]` is replaced, or added to
/// that section, or the section is added. `Err` for a root that is not an
/// absolute path, holds a `;` or a control character, or has `.` or `..` parts,
/// and for more than the file reads.
pub fn with_roots(text: &str, roots: &[String]) -> Result<String, String> {
    if roots.len() > MAX_ITEMS {
        return Err("too many folders".into());
    }
    let mut clean: Vec<String> = Vec::new();
    for r in roots {
        let r = r.trim();
        let p = Path::new(r);
        if !p.is_absolute()
            || r.contains(';')
            || r.chars().any(char::is_control)
            || r.split('/').any(|seg| seg == "." || seg == "..")
        {
            return Err(format!("'{r}' is not a folder the index can hold"));
        }
        let norm = if r.len() > 1 {
            r.trim_end_matches('/')
        } else {
            r
        };
        if !clean.iter().any(|c| c == norm) {
            clean.push(norm.to_string());
        }
    }
    let line = format!("Roots={}", clean.join(";"));
    // The service skips a line over 8192 bytes: a list that long would be written and then ignored.
    if line.len() > 8192 {
        return Err("too many folders, or paths too long, for one line".into());
    }
    let mut out: Vec<String> = Vec::new();
    let (mut in_index, mut replaced, mut index_end) = (false, false, None);
    for l in text.lines() {
        let t = l.trim();
        if let Some(sec) = t.strip_prefix('[').and_then(|x| x.strip_suffix(']')) {
            if in_index && index_end.is_none() {
                index_end = Some(out.len());
            }
            in_index = sec == "Index";
            out.push(l.to_string());
            continue;
        }
        if in_index && t.split_once('=').is_some_and(|(k, _)| k.trim() == "Roots") {
            if !replaced {
                out.push(line.clone());
                replaced = true;
            }
            continue;
        }
        out.push(l.to_string());
    }
    if !replaced {
        if in_index && index_end.is_none() {
            index_end = Some(out.len());
        }
        match index_end {
            Some(at) => {
                // After the last non-blank line of the section.
                let mut at = at;
                while at > 0 && out[at - 1].trim().is_empty() {
                    at -= 1;
                }
                out.insert(at, line);
            }
            None => {
                if out.last().is_some_and(|l| !l.trim().is_empty()) {
                    out.push(String::new());
                }
                out.push("[Index]".to_string());
                out.push(line);
            }
        }
    }
    let mut s = out.join("\n");
    s.push('\n');
    Ok(s)
}

fn split_list(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .take(MAX_ITEMS)
}

/// `~` and `~/x` become paths under `home`; the rest must be absolute. `.` and
/// `..` parts are refused, and a trailing `/` dropped.
fn expand(item: &str, home: &Path) -> Option<PathBuf> {
    let p = if item == "~" {
        home.to_path_buf()
    } else if let Some(rest) = item.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(item)
    };
    if !p.is_absolute() || p.as_os_str().as_bytes().contains(&0) {
        return None;
    }
    let mut out = PathBuf::from("/");
    for c in p.components() {
        use std::path::Component::*;
        match c {
            RootDir => {}
            Normal(n) => out.push(n),
            CurDir => {}
            ParentDir | Prefix(_) => return None,
        }
    }
    Some(out)
}

/// Opens a file that must be a regular one, without blocking on a named pipe
/// (a FIFO where a settings file should be would stall the start otherwise).
/// A link is followed unless `follow` is false: the user's own settings may be
/// links into a dotfiles folder, a `CACHEDIR.TAG` in a tree from elsewhere may
/// not.
pub(crate) fn open_regular(path: &Path, follow: bool) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    let flags = libc::O_NONBLOCK | libc::O_CLOEXEC | if follow { 0 } else { libc::O_NOFOLLOW };
    let f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(flags)
        .open(path)?;
    if !f.metadata()?.is_file() {
        return Err(std::io::Error::other("not a regular file"));
    }
    Ok(f)
}

fn read_capped(path: &Path) -> std::io::Result<Option<String>> {
    use std::io::Read;
    let f = match open_regular(path, true) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut buf = Vec::new();
    f.take(MAX_FILE + 1).read_to_end(&mut buf)?;
    if buf.len() as u64 > MAX_FILE {
        return Err(std::io::Error::other("file is larger than 64 KiB"));
    }
    String::from_utf8(buf)
        .map(Some)
        .map_err(|_| std::io::Error::other("not UTF-8"))
}

/// The decision data of the scanner: which names and paths are never indexed.
#[derive(Clone, Debug)]
pub struct Excludes {
    names: HashSet<Vec<u8>>,
    paths: HashSet<Vec<u8>>,
}

/// What to do with a folder found while listing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirDecision {
    Index,
    /// A dot-folder: never entered.
    SkipHidden,
    /// `node_modules`, `__pycache__`, or in `Exclude=`.
    SkipExcluded,
    /// On another filesystem than the root: mounts are not crossed.
    SkipOtherFs,
}

impl Excludes {
    pub fn new(user: &[OsString]) -> Excludes {
        let mut names: HashSet<Vec<u8>> = BUILTIN_EXCLUDES
            .iter()
            .map(|s| s.as_bytes().to_vec())
            .collect();
        let mut paths = HashSet::new();
        for u in user {
            let b = u.as_bytes();
            if b.first() == Some(&b'/') {
                paths.insert(b.to_vec());
            } else {
                names.insert(b.to_vec());
            }
        }
        Excludes { names, paths }
    }

    /// A hash of the whole exclusion set (built-in and user names and paths),
    /// stored in the snapshot: when the rules change, the snapshot is not used.
    pub fn fingerprint(&self) -> u32 {
        let mut names: Vec<&Vec<u8>> = self.names.iter().collect();
        let mut paths: Vec<&Vec<u8>> = self.paths.iter().collect();
        names.sort_unstable();
        paths.sort_unstable();
        let mut h = crc32fast::Hasher::new();
        for (tag, list) in [(b'n', names), (b'p', paths)] {
            for item in list {
                h.update(&[tag]);
                h.update(&(item.len() as u32).to_le_bytes());
                h.update(item);
            }
        }
        h.finalize()
    }

    pub fn name_excluded(&self, name: &[u8]) -> bool {
        self.names.contains(name)
    }

    pub fn has_paths(&self) -> bool {
        !self.paths.is_empty()
    }

    pub fn path_excluded(&self, path: &[u8]) -> bool {
        !self.paths.is_empty() && self.paths.contains(path)
    }

    /// The decision for a folder: its name, its device and the root's device.
    pub fn dir_decision(&self, name: &[u8], dev: u64, root_dev: u64) -> DirDecision {
        if name.first() == Some(&b'.') {
            DirDecision::SkipHidden
        } else if self.name_excluded(name) {
            DirDecision::SkipExcluded
        } else if dev != root_dev {
            DirDecision::SkipOtherFs
        } else {
            DirDecision::Index
        }
    }
}

/// The first line of a valid `CACHEDIR.TAG`.
pub const CACHEDIR_SIGNATURE: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55";

/// Does the start of a `CACHEDIR.TAG` file carry the signature?
pub fn is_cachedir_tag(head: &[u8]) -> bool {
    head.starts_with(CACHEDIR_SIGNATURE)
}

/// Does a folder with these entry names hold a marker that excludes it?
/// (`CACHEDIR.TAG` still needs its signature checked: pass what it said.)
pub fn marker_excludes(has_cachedir_tag: Option<bool>, has_pyvenv: bool) -> bool {
    has_cachedir_tag == Some(true) || has_pyvenv
}

/// Parse a folder's `.hidden` file: one name per line.
pub fn parse_hidden_file(bytes: &[u8]) -> HashSet<Vec<u8>> {
    bytes
        .split(|&b| b == b'\n')
        .map(|l| l.strip_suffix(b"\r").unwrap_or(l))
        .filter(|l| !l.is_empty() && l.len() <= 255 && !l.contains(&b'/'))
        .take(100_000)
        .map(<[u8]>::to_vec)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/home/t";

    #[test]
    fn fingerprint_follows_the_rules() {
        let a = Excludes::new(&[OsString::from("target")]);
        let b = Excludes::new(&[OsString::from("target")]);
        let c = Excludes::new(&[OsString::from("target"), OsString::from("/x/y")]);
        assert_eq!(a.fingerprint(), b.fingerprint());
        assert_ne!(a.fingerprint(), c.fingerprint());
    }

    #[test]
    fn defaults_without_a_file() {
        let c = Config::parse("", Path::new(HOME));
        assert_eq!(c.roots, vec![PathBuf::from(HOME)]);
        assert!(c.exclude.is_empty());
    }

    #[test]
    fn parses_roots_and_excludes() {
        let t = "# c\n[Other]\nRoots=/bad\n[Index]\nRoots=~; /mnt/d/ ;rel;~/x/../y;/a/./b\nExclude=target;/srv/x;~/tmp\nWhat=1\n";
        let c = Config::parse(t, Path::new(HOME));
        assert_eq!(
            c.roots,
            vec![
                PathBuf::from(HOME),
                PathBuf::from("/mnt/d"),
                PathBuf::from("/a/b")
            ]
        );
        assert_eq!(
            c.exclude,
            vec![
                OsString::from("target"),
                OsString::from("/srv/x"),
                OsString::from("/home/t/tmp")
            ]
        );
    }

    #[test]
    fn roots_are_written_into_the_file() {
        let home = Path::new(HOME);
        // No file: a section is made.
        let t = with_roots("", &["/mnt/data".into(), "/home/t".into()]).unwrap();
        assert_eq!(t, "[Index]\nRoots=/mnt/data;/home/t\n");
        assert_eq!(
            Config::parse(&t, home).roots,
            vec![PathBuf::from("/mnt/data"), PathBuf::from(HOME)]
        );
        // The line is replaced, the rest is kept as it is.
        let old =
            "# mine\n[Other]\nRoots=/keep\n[Index]\nRoots=~\nExclude=target;*.tmp\n\n[Last]\nx=1\n";
        let t = with_roots(old, &["/srv/a".into(), "/srv/a/".into(), "/srv/b".into()]).unwrap();
        assert_eq!(
            t,
            "# mine\n[Other]\nRoots=/keep\n[Index]\nRoots=/srv/a;/srv/b\nExclude=target;*.tmp\n\n[Last]\nx=1\n"
        );
        let c = Config::parse(&t, home);
        assert_eq!(
            c.roots,
            vec![PathBuf::from("/srv/a"), PathBuf::from("/srv/b")]
        );
        assert_eq!(
            c.exclude,
            vec![OsString::from("target"), OsString::from("*.tmp")]
        );
        // A section without the key gets it; one at the end too.
        let t = with_roots("[Index]\nExclude=x\n\n[Other]\n", &["/a".into()]).unwrap();
        assert_eq!(t, "[Index]\nExclude=x\nRoots=/a\n\n[Other]\n");
        let t = with_roots("[Index]\nExclude=x", &["/a".into()]).unwrap();
        assert_eq!(t, "[Index]\nExclude=x\nRoots=/a\n");
        let t = with_roots("[Other]\nk=v\n", &["/a".into()]).unwrap();
        assert_eq!(t, "[Other]\nk=v\n\n[Index]\nRoots=/a\n");
        // No folders turns the index off.
        let t = with_roots("[Index]\nRoots=~\n", &[]).unwrap();
        assert!(Config::parse(&t, home).roots.is_empty());
        // Twice the same key keeps one.
        let t = with_roots("[Index]\nRoots=/a\nRoots=/b\n", &["/c".into()]).unwrap();
        assert_eq!(t, "[Index]\nRoots=/c\n");
        // Refusals.
        for bad in [
            "rel/path",
            "~",
            "/a;b",
            "/a/../b",
            "/a/./b",
            "/a\nExclude=/",
            "/a\u{7}",
        ] {
            assert!(with_roots("", &[bad.to_string()]).is_err(), "{bad:?}");
        }
        let long: Vec<String> = (0..10)
            .map(|i| format!("/{}{i}", "x".repeat(1000)))
            .collect();
        assert!(with_roots("", &long).is_err());
        let many: Vec<String> = (0..300).map(|i| format!("/r{i}")).collect();
        assert!(with_roots("", &many).is_err());
    }

    #[test]
    fn empty_roots_disables() {
        let c = Config::parse("[Index]\nRoots=\n", Path::new(HOME));
        assert!(c.roots.is_empty());
    }

    #[test]
    fn garbage_is_survived() {
        let c = Config::parse("[Index\nRoots\n=\n[Index]\nRoots=\u{0}\n", Path::new(HOME));
        assert!(c.roots.is_empty());
    }

    fn mkfifo(p: &Path) {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(p.as_os_str().as_bytes()).unwrap();
        // SAFETY: a NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    }

    #[test]
    fn a_named_pipe_in_place_of_the_settings_file_does_not_stall_the_start() {
        let base = crate::testdir::new("config-fifo");
        let d = base.join("telamon-explorer");
        std::fs::create_dir_all(&d).unwrap();
        mkfifo(&d.join("indexrc"));
        let (tx, rx) = std::sync::mpsc::channel();
        let b = base.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Config::load(&b, Path::new(HOME)).roots);
        });
        let roots = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("Config::load blocked on a named pipe");
        assert_eq!(roots, vec![PathBuf::from(HOME)]);
    }

    #[test]
    fn open_regular_refuses_pipes_and_optionally_links() {
        let base = crate::testdir::new("config-open");
        mkfifo(&base.join("p"));
        assert!(open_regular(&base.join("p"), true).is_err());
        std::fs::write(base.join("f"), b"x").unwrap();
        std::os::unix::fs::symlink(base.join("f"), base.join("l")).unwrap();
        assert!(open_regular(&base.join("l"), true).is_ok());
        assert!(open_regular(&base.join("l"), false).is_err());
    }

    #[test]
    fn load_missing_and_oversized() {
        let base = crate::testdir::new("config");
        assert_eq!(
            Config::load(&base, Path::new(HOME)).roots,
            vec![PathBuf::from(HOME)]
        );
        let d = base.join("telamon-explorer");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("indexrc"), "[Index]\nRoots=/x\n").unwrap();
        assert_eq!(
            Config::load(&base, Path::new(HOME)).roots,
            vec![PathBuf::from("/x")]
        );
        std::fs::write(d.join("indexrc"), vec![b'#'; 70_000]).unwrap();
        assert_eq!(
            Config::load(&base, Path::new(HOME)).roots,
            vec![PathBuf::from(HOME)]
        );
    }

    #[test]
    fn load_reads_the_old_folder_until_it_is_moved() {
        let base = crate::testdir::new("config-legacy");
        let old = base.join("atlas-explorer");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("indexrc"), "[Index]\nRoots=/old\n").unwrap();
        assert_eq!(
            Config::load(&base, Path::new(HOME)).roots,
            vec![PathBuf::from("/old")]
        );
        // The new file wins as soon as there is one.
        let new = base.join("telamon-explorer");
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(new.join("indexrc"), "[Index]\nRoots=/new\n").unwrap();
        assert_eq!(
            Config::load(&base, Path::new(HOME)).roots,
            vec![PathBuf::from("/new")]
        );
    }

    #[test]
    fn directory_decisions() {
        let e = Excludes::new(&[OsString::from("build"), OsString::from("/srv/skip")]);
        assert_eq!(e.dir_decision(b"Documents", 1, 1), DirDecision::Index);
        assert_eq!(e.dir_decision(b".git", 1, 1), DirDecision::SkipHidden);
        assert_eq!(e.dir_decision(b".cache", 1, 1), DirDecision::SkipHidden);
        assert_eq!(
            e.dir_decision(b"node_modules", 1, 1),
            DirDecision::SkipExcluded
        );
        assert_eq!(
            e.dir_decision(b"__pycache__", 1, 1),
            DirDecision::SkipExcluded
        );
        assert_eq!(e.dir_decision(b"build", 1, 1), DirDecision::SkipExcluded);
        assert_eq!(e.dir_decision(b"mnt", 2, 1), DirDecision::SkipOtherFs);
        // a hidden name wins over a different filesystem
        assert_eq!(e.dir_decision(b".x", 2, 1), DirDecision::SkipHidden);
        assert!(e.path_excluded(b"/srv/skip"));
        assert!(!e.path_excluded(b"/srv/other"));
    }

    #[test]
    fn markers() {
        assert!(is_cachedir_tag(
            b"Signature: 8a477f597d28d172789f06886806bc55\n# comment"
        ));
        assert!(!is_cachedir_tag(b"Signature: 0000"));
        assert!(!is_cachedir_tag(b""));
        assert!(marker_excludes(Some(true), false));
        assert!(!marker_excludes(Some(false), false));
        assert!(!marker_excludes(None, false));
        assert!(marker_excludes(None, true));
    }

    #[test]
    fn hidden_file() {
        let h = parse_hidden_file(b"a\r\nb c\n\n../x\nd/e\n");
        assert!(h.contains(b"a".as_slice()));
        assert!(h.contains(b"b c".as_slice()));
        assert!(!h.contains(b"../x".as_slice()));
        assert!(!h.contains(b"d/e".as_slice()));
    }
}
