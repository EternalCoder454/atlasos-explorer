//! Git status badges (off by default): in a folder inside a git work tree, the
//! items that are modified, new, ignored or in conflict get a small badge.
//!
//! `git` is run, and nothing else is: by argument list (never a shell), in the
//! folder, with a timeout, on a worker thread of the caller, with a **safe
//! environment and safe options**, because a work tree can come from anywhere
//! (an archive that was extracted, a clone of somebody else's project) and
//! `git status` can run programs that the repository's own configuration
//! names:
//!
//! * `core.fsmonitor` (a hook that `status` runs) is turned off on the command
//!   line, and `core.hooksPath` points nowhere, so no hook runs;
//! * the system and the global configuration and the system attributes are not
//!   read (`GIT_CONFIG_NOSYSTEM`, `GIT_CONFIG_GLOBAL=/dev/null`), nor is any
//!   `GIT_*` variable of the caller's environment (the environment is cleared
//!   first);
//! * `status` compares a file with the index by running its `filter.*.clean`
//!   command when the work tree's attributes name a filter, so a repository
//!   whose own configuration defines a filter, or includes another file, is
//!   **skipped** ([`Skip::UnsafeConfig`]) rather than trusted;
//! * a repository owned by another user is skipped ([`Skip::OtherOwner`]), as
//!   git's own `safe.directory` rule does: its work tree and its git
//!   directory must belong to the user running Files;
//! * no locks are taken, no index is written (`--no-optional-locks`), no
//!   missing object is fetched (`GIT_NO_LAZY_FETCH`), submodules are not
//!   entered, no question is asked (`GIT_TERMINAL_PROMPT=0`).
//!
//! The answer is `git status --porcelain=v2 -z`, read here as untrusted bytes.

use std::collections::BTreeMap;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// The user Files runs as: whose repositories are looked at.
pub fn effective_uid() -> u32 {
    // SAFETY: geteuid has no arguments and cannot fail.
    unsafe { libc::geteuid() }
}

/// How long `git status` may take before it is stopped.
pub const TIMEOUT: Duration = Duration::from_secs(5);
/// The most output read; more is an error (a work tree this big gets no badges).
pub const MAX_OUTPUT: usize = 16 << 20;
/// The biggest `config` file looked at.
const MAX_CONFIG: u64 = 256 << 10;

/// What an item is marked with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Badge {
    /// Ignored (the lowest: anything else wins).
    Ignored = 3,
    /// Not in the repository yet (untracked, or added to the index).
    New = 2,
    /// Changed, deleted or renamed, or a folder with such things inside.
    Modified = 1,
    /// A merge conflict.
    Conflict = 4,
}

impl Badge {
    /// The number the window uses (0 is no badge).
    pub fn code(self) -> u8 {
        self as u8
    }

    /// Which badge wins when an item has two reasons for one.
    fn rank(self) -> u8 {
        match self {
            Badge::Conflict => 4,
            Badge::Modified => 3,
            Badge::New => 2,
            Badge::Ignored => 1,
        }
    }
}

/// Why a folder gets no badges.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Skip {
    /// Not in a git work tree (the usual case, and not an error).
    NotARepo,
    /// The work tree or its git directory belongs to another user.
    OtherOwner,
    /// The repository's own configuration names a filter or includes a file.
    UnsafeConfig,
    /// `git` is not installed (or not found in `PATH`).
    NoGit,
    /// `git` did not answer in time and was stopped.
    Timeout,
    /// `git` failed, or its answer was too big.
    Failed(String),
}

/// The badges of one folder's items.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FolderBadges {
    /// Item name (bytes, as the file system has it) to its badge.
    pub items: BTreeMap<Vec<u8>, Badge>,
    /// The folder itself is ignored: every item in it is, unless listed.
    pub all_ignored: bool,
}

// ---- Reading the answer ----

/// The entries of `git status --porcelain=v2 -z`: the path (relative to the
/// top of the work tree, as git gives it) and its badge. Records are
/// separated by NUL. Header lines (`#`) and anything not understood are
/// skipped; the extra path that follows a rename or a copy is skipped too.
pub fn parse_porcelain_v2(out: &[u8]) -> Vec<(Vec<u8>, Badge)> {
    let mut entries = Vec::new();
    let mut records = out.split(|&b| b == 0).filter(|r| !r.is_empty());
    while let Some(rec) = records.next() {
        let Some(&kind) = rec.first() else { continue };
        match kind {
            b'?' | b'!' => {
                // "? path" / "! path"
                if let Some(path) = rec.get(2..) {
                    let b = if kind == b'?' {
                        Badge::New
                    } else {
                        Badge::Ignored
                    };
                    entries.push((path.to_vec(), b));
                }
            }
            b'1' | b'2' | b'u' => {
                // 1 XY sub mH mI mW hH hI path           (8 fields before the path)
                // 2 XY sub mH mI mW hH hI Xscore path    (9; the original path is the next record)
                // u XY sub m1 m2 m3 mW h1 h2 h3 path     (10)
                let before = match kind {
                    b'1' => 8,
                    b'2' => 9,
                    _ => 10,
                };
                if kind == b'2' {
                    // Its original path comes as a record of its own.
                    let _ = records.next();
                }
                let mut rest = rec;
                let mut xy: &[u8] = b"";
                for i in 0..before {
                    let Some(sp) = rest.iter().position(|&b| b == b' ') else {
                        rest = b"";
                        break;
                    };
                    if i == 1 {
                        xy = &rest[..sp];
                    }
                    rest = &rest[sp + 1..];
                }
                if rest.is_empty() || xy.len() != 2 {
                    continue;
                }
                let badge = match kind {
                    b'u' => Badge::Conflict,
                    // Added to the index and not changed since: new.
                    _ if xy[0] == b'A' => Badge::New,
                    _ => Badge::Modified,
                };
                entries.push((rest.to_vec(), badge));
            }
            _ => {}
        }
    }
    entries
}

fn merge(items: &mut BTreeMap<Vec<u8>, Badge>, name: Vec<u8>, b: Badge) {
    match items.get_mut(&name) {
        Some(old) => {
            if b.rank() > old.rank() {
                *old = b;
            }
        }
        None => {
            items.insert(name, b);
        }
    }
}

/// The badges of the items directly inside a folder. `prefix` is the folder's
/// path relative to the top of the work tree (no slash at either end, empty at
/// the top). An entry exactly at an item gives that item its badge; an entry
/// deeper inside a folder marks the folder as modified (a folder with only
/// ignored things inside gets no badge for it); an ignored folder that holds
/// this one makes everything here ignored.
pub fn badges_for(entries: &[(Vec<u8>, Badge)], prefix: &[u8]) -> FolderBadges {
    let mut out = FolderBadges::default();
    for (path, badge) in entries {
        let path = path.strip_suffix(b"/").unwrap_or(path);
        if path.is_empty() {
            continue;
        }
        // The folder itself, or a folder around it, is ignored.
        if *badge == Badge::Ignored
            && (path == prefix
                || (prefix.len() > path.len()
                    && prefix.starts_with(path)
                    && prefix[path.len()] == b'/'))
        {
            out.all_ignored = true;
            continue;
        }
        let rest = if prefix.is_empty() {
            path
        } else {
            match path.strip_prefix(prefix) {
                Some(r) if r.first() == Some(&b'/') => &r[1..],
                _ => continue,
            }
        };
        if rest.is_empty() {
            continue;
        }
        match rest.iter().position(|&b| b == b'/') {
            None => merge(&mut out.items, rest.to_vec(), *badge),
            Some(slash) => {
                // Something inside a folder of this one.
                let folder = rest[..slash].to_vec();
                let inside = match badge {
                    Badge::Ignored => continue,
                    Badge::Conflict => Badge::Conflict,
                    _ => Badge::Modified,
                };
                merge(&mut out.items, folder, inside);
            }
        }
    }
    out
}

// ---- Finding the work tree and checking it ----

/// The top of the work tree that holds `folder`: the nearest folder above (or
/// itself) that has a `.git` entry. At most 64 levels are tried.
pub fn find_repo(folder: &Path) -> Option<PathBuf> {
    let mut cur = folder.to_path_buf();
    for _ in 0..64 {
        if std::fs::symlink_metadata(cur.join(".git")).is_ok() {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
    None
}

/// The git directory of a work tree: `.git` itself, or the folder a `.git`
/// file names (`gitdir: ...`), which is what a linked work tree has.
fn git_dir(root: &Path) -> Option<PathBuf> {
    let dot = root.join(".git");
    let meta = std::fs::symlink_metadata(&dot).ok()?;
    if meta.is_dir() {
        return Some(dot);
    }
    if !meta.is_file() || meta.len() > 4096 {
        return None;
    }
    let text = std::fs::read_to_string(&dot).ok()?;
    let target = text.lines().next()?.strip_prefix("gitdir:")?.trim();
    if target.is_empty() || target.contains('\0') {
        return None;
    }
    let p = Path::new(target);
    Some(if p.is_absolute() {
        p.to_path_buf()
    } else {
        root.join(p)
    })
}

fn owned_by(path: &Path, uid: u32) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.uid() == uid)
}

/// Whether a configuration file names something git would run or pull in:
/// a `[filter ...]` section, or an `include` / `includeIf` section.
pub fn config_is_unsafe(text: &str) -> bool {
    text.lines().any(|line| {
        let l = line.trim_start().to_ascii_lowercase();
        l.starts_with("[filter") || l.starts_with("[include")
    })
}

/// Checks the work tree at `root` for the user `uid`: its top folder, its git
/// directory (and the shared one of a linked work tree) are theirs, and the
/// configuration is one git can read without running anything.
pub fn check_repo(root: &Path, uid: u32) -> Result<(), Skip> {
    let Some(dir) = git_dir(root) else {
        return Err(Skip::NotARepo);
    };
    if !owned_by(root, uid) || !owned_by(&dir, uid) {
        return Err(Skip::OtherOwner);
    }
    // A linked work tree shares the main repository's configuration.
    let mut dirs = vec![dir.clone()];
    if let Ok(common) = std::fs::read_to_string(dir.join("commondir")) {
        let c = common.lines().next().unwrap_or("").trim();
        if !c.is_empty() && !c.contains('\0') {
            let p = Path::new(c);
            let p = if p.is_absolute() {
                p.to_path_buf()
            } else {
                dir.join(p)
            };
            if !owned_by(&p, uid) {
                return Err(Skip::OtherOwner);
            }
            dirs.push(p);
        }
    }
    for d in dirs {
        for name in ["config", "config.worktree"] {
            let f = d.join(name);
            let Ok(meta) = std::fs::metadata(&f) else {
                continue;
            };
            if meta.len() > MAX_CONFIG {
                return Err(Skip::UnsafeConfig);
            }
            let Ok(text) = std::fs::read(&f) else {
                return Err(Skip::UnsafeConfig);
            };
            if config_is_unsafe(&String::from_utf8_lossy(&text)) {
                return Err(Skip::UnsafeConfig);
            }
        }
    }
    Ok(())
}

// ---- Running git ----

/// The arguments of the one command that is run (after `git`).
pub fn status_args() -> Vec<&'static str> {
    vec![
        "--no-optional-locks",
        "-c",
        "core.fsmonitor=false",
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "core.attributesFile=/dev/null",
        "-c",
        "status.submoduleSummary=false",
        "status",
        "--porcelain=v2",
        "-z",
        "--untracked-files=normal",
        "--ignored=matching",
        "--ignore-submodules=all",
        "--no-renames",
        "--",
        ".",
    ]
}

/// The environment `git` runs in: only what it needs, and nothing of the
/// caller's `GIT_*` variables or configuration.
pub fn status_env(path: &str) -> Vec<(&'static str, String)> {
    vec![
        (
            "PATH",
            if path.is_empty() {
                "/usr/local/bin:/usr/bin:/bin".to_string()
            } else {
                path.to_string()
            },
        ),
        ("LC_ALL", "C".into()),
        ("HOME", "/nonexistent".into()),
        ("GIT_CONFIG_NOSYSTEM", "1".into()),
        ("GIT_CONFIG_GLOBAL", "/dev/null".into()),
        ("GIT_ATTR_NOSYSTEM", "1".into()),
        ("GIT_OPTIONAL_LOCKS", "0".into()),
        ("GIT_NO_LAZY_FETCH", "1".into()),
        ("GIT_TERMINAL_PROMPT", "0".into()),
        ("GIT_PAGER", "cat".into()),
    ]
}

/// The work tree's folder relative to its top, with no slash at either end.
fn relative(root: &Path, folder: &Path) -> Option<Vec<u8>> {
    let rel = folder.strip_prefix(root).ok()?;
    Some(rel.as_os_str().as_bytes().to_vec())
}

/// Runs `git status` for `folder` and gives the badges of its items. Does the
/// checks above first; blocks for at most `timeout` (and reads at most
/// [`MAX_OUTPUT`] bytes), so it belongs on a worker thread. `path` is the
/// `PATH` to look for `git` in; `uid` the user the work tree must belong to.
pub fn status(
    folder: &Path,
    uid: u32,
    path: &str,
    timeout: Duration,
) -> Result<FolderBadges, Skip> {
    // Links are followed once, here, so the relative path below is honest.
    let folder = std::fs::canonicalize(folder).map_err(|_| Skip::NotARepo)?;
    let root = find_repo(&folder).ok_or(Skip::NotARepo)?;
    check_repo(&root, uid)?;
    let prefix = relative(&root, &folder).ok_or(Skip::NotARepo)?;
    let mut cmd = Command::new("git");
    cmd.args(status_args())
        .current_dir(&folder)
        .env_clear()
        .envs(status_env(path))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Skip::NoGit
        } else {
            Skip::Failed(e.to_string())
        }
    })?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| Skip::Failed("no output".into()))?;
    // Read on a thread, so a full pipe never stops git and the wait below is the only clock.
    let big = Arc::new(AtomicBool::new(false));
    let big_flag = Arc::clone(&big);
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 16384];
        loop {
            match stdout.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if buf.len() + n > MAX_OUTPUT {
                        // Too much: say so and stop reading (the caller kills git).
                        big_flag.store(true, Ordering::SeqCst);
                        buf.clear();
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
            }
        }
        buf
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) => {
                if big.load(Ordering::SeqCst) || Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err(Skip::Failed(e.to_string()));
            }
        }
    };
    let out = reader.join().unwrap_or_default();
    if big.load(Ordering::SeqCst) {
        return Err(Skip::Failed("too many changes to show".into()));
    }
    let Some(status) = status else {
        return Err(Skip::Timeout);
    };
    if !status.success() {
        return Err(Skip::Failed(format!("git status ended with {status}")));
    }
    Ok(badges_for(&parse_porcelain_v2(&out), &prefix))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn git_available() -> bool {
        Command::new("git")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    fn me() -> u32 {
        // SAFETY: geteuid has no arguments and cannot fail.
        unsafe { libc::geteuid() }
    }

    fn path_env() -> String {
        std::env::var("PATH").unwrap_or_default()
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("telamon-git-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// A git command for building a fixture: the user's own configuration and
    /// environment play no part.
    fn git(dir: &Path, args: &[&str]) {
        let st = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env_clear()
            .env("PATH", path_env())
            .env("HOME", "/nonexistent")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(st.success(), "git {args:?}");
    }

    fn fixture(name: &str) -> PathBuf {
        let d = tmp(name);
        git(&d, &["init", "-q", "-b", "main"]);
        fs::write(d.join("tracked.txt"), "one\n").unwrap();
        fs::write(d.join("changed.txt"), "one\n").unwrap();
        fs::write(d.join("gone.txt"), "one\n").unwrap();
        fs::create_dir_all(d.join("sub/deep")).unwrap();
        fs::write(d.join("sub/deep/inner.txt"), "one\n").unwrap();
        fs::write(d.join("sub/quiet.txt"), "one\n").unwrap();
        fs::write(d.join(".gitignore"), "*.log\nbuild/\n").unwrap();
        git(&d, &["add", "-A"]);
        git(&d, &["commit", "-q", "-m", "first"]);
        d
    }

    fn names(b: &FolderBadges) -> Vec<(String, u8)> {
        b.items
            .iter()
            .map(|(k, v)| (String::from_utf8_lossy(k).into_owned(), v.code()))
            .collect()
    }

    #[test]
    fn porcelain_v2_is_read() {
        let out = b"# branch.oid abc\0\
1 .M N... 100644 100644 100644 aaa bbb changed.txt\0\
1 A. N... 000000 100644 100644 000 bbb staged new.txt\0\
1 .D N... 100644 100644 000000 aaa bbb gone.txt\0\
2 R. N... 100644 100644 100644 aaa bbb R100 renamed.txt\0old name.txt\0\
u UU N... 100644 100644 100644 100644 a b c conflict.txt\0\
? untracked dir/\0\
? spaced \xff name\0\
! build/\0\
! x.log\0\
X weird record\0";
        let e = parse_porcelain_v2(out);
        let get = |p: &[u8]| e.iter().find(|(q, _)| q == p).map(|(_, b)| *b);
        assert_eq!(get(b"changed.txt"), Some(Badge::Modified));
        assert_eq!(get(b"staged new.txt"), Some(Badge::New));
        assert_eq!(get(b"gone.txt"), Some(Badge::Modified));
        assert_eq!(get(b"renamed.txt"), Some(Badge::Modified));
        // The original path of a rename is not an entry of its own.
        assert_eq!(get(b"old name.txt"), None);
        assert_eq!(get(b"conflict.txt"), Some(Badge::Conflict));
        assert_eq!(get(b"untracked dir/"), Some(Badge::New));
        assert_eq!(get(b"spaced \xff name"), Some(Badge::New));
        assert_eq!(get(b"build/"), Some(Badge::Ignored));
        assert_eq!(get(b"x.log"), Some(Badge::Ignored));
        assert_eq!(e.len(), 10 - 1);
    }

    #[test]
    fn hostile_or_damaged_output_is_only_skipped() {
        for out in [
            &b""[..],
            b"\0\0\0",
            b"1",
            b"1 .M",
            b"1 .M N... 1 2 3 4 5",
            b"u UU N...",
            b"2 R. N... 1 2 3 4 5 R100",
            b"?",
            b"? ",
            b"1 X N... 100644 100644 100644 aaa bbb p",
            b"\xff\xfe\0\x01\x02",
        ] {
            let _ = parse_porcelain_v2(out);
        }
        // A path may hold anything, including a line break or a NUL-free control byte.
        let e = parse_porcelain_v2(b"? line\nbreak\x1b[31m\0");
        assert_eq!(e, vec![(b"line\nbreak\x1b[31m".to_vec(), Badge::New)]);
    }

    #[test]
    fn badges_follow_the_folder() {
        let e = vec![
            (b"top.txt".to_vec(), Badge::Modified),
            (b"sub/deep/inner.txt".to_vec(), Badge::Modified),
            (b"sub/new.txt".to_vec(), Badge::New),
            (b"sub/x.log".to_vec(), Badge::Ignored),
            (b"other/deep/ignored.log".to_vec(), Badge::Ignored),
            (b"newdir/".to_vec(), Badge::New),
            (b"build/".to_vec(), Badge::Ignored),
            (b"clash/inside".to_vec(), Badge::New),
            (b"clash/inside2".to_vec(), Badge::Conflict),
        ];
        let top = badges_for(&e, b"");
        assert_eq!(
            names(&top),
            vec![
                ("build".to_string(), 3),
                ("clash".to_string(), 4),
                ("newdir".to_string(), 2),
                ("sub".to_string(), 1),
                ("top.txt".to_string(), 1),
            ]
        );
        assert!(!top.all_ignored);
        // A folder with only ignored things inside gets nothing.
        assert!(!top.items.contains_key(&b"other"[..]));
        let sub = badges_for(&e, b"sub");
        assert_eq!(
            names(&sub),
            vec![
                ("deep".to_string(), 1),
                ("new.txt".to_string(), 2),
                ("x.log".to_string(), 3)
            ]
        );
        // "sub" is not "sub2": a name that merely starts the same is another folder.
        assert!(badges_for(&e, b"su").items.is_empty());
        let deep = badges_for(&e, b"sub/deep");
        assert_eq!(names(&deep), vec![("inner.txt".to_string(), 1)]);
        // Inside an ignored folder everything is ignored.
        let inside = badges_for(&[(b"build/".to_vec(), Badge::Ignored)], b"build/out");
        assert!(inside.all_ignored);
        assert!(inside.items.is_empty());
        let same = badges_for(&[(b"build/".to_vec(), Badge::Ignored)], b"build");
        assert!(same.all_ignored);
        // One item with two reasons shows the stronger.
        let both = badges_for(
            &[
                (b"a".to_vec(), Badge::Ignored),
                (b"a/b".to_vec(), Badge::Modified),
            ],
            b"",
        );
        assert_eq!(both.items[&b"a"[..]], Badge::Modified);
    }

    #[test]
    fn a_configuration_that_runs_things_is_unsafe() {
        assert!(config_is_unsafe(
            "[core]\n\tbare = false\n[filter \"lfs\"]\n\tclean = x\n"
        ));
        assert!(config_is_unsafe("  [FILTER \"x\"]\n"));
        assert!(config_is_unsafe("[include]\n\tpath = ../x\n"));
        assert!(config_is_unsafe("[includeIf \"gitdir:/\"]\n\tpath = x\n"));
        assert!(!config_is_unsafe(
            "[core]\n\trepositoryformatversion = 0\n[remote \"origin\"]\n\turl = x\n"
        ));
        assert!(!config_is_unsafe("# [filter \"x\"] is only a comment\n"));
    }

    #[test]
    fn the_command_is_a_list_with_no_shell_and_safe_options() {
        let a = status_args();
        assert!(a.contains(&"core.fsmonitor=false"));
        assert!(a.contains(&"core.hooksPath=/dev/null"));
        assert!(a.contains(&"--porcelain=v2"));
        assert!(a.contains(&"-z"));
        assert!(a.contains(&"--ignore-submodules=all"));
        assert!(a.contains(&"--no-optional-locks"));
        assert!(a.iter().all(|s| !s.contains(['`', '$', ';', '|', '&'])));
        let env = status_env("/usr/bin");
        assert!(
            env.iter()
                .any(|(k, v)| *k == "GIT_CONFIG_GLOBAL" && v == "/dev/null")
        );
        assert!(
            env.iter()
                .any(|(k, v)| *k == "GIT_CONFIG_NOSYSTEM" && v == "1")
        );
        assert!(
            env.iter()
                .any(|(k, v)| *k == "GIT_TERMINAL_PROMPT" && v == "0")
        );
        // Nothing of the caller's environment is passed on.
        assert!(
            env.iter()
                .all(|(k, _)| !k.starts_with("GIT_DIR") && *k != "SHELL")
        );
    }

    #[test]
    fn a_real_work_tree_gets_its_badges() {
        if !git_available() {
            eprintln!("git is not installed: skipped");
            return;
        }
        let d = fixture("real");
        fs::write(d.join("changed.txt"), "two\n").unwrap();
        fs::remove_file(d.join("gone.txt")).unwrap();
        fs::write(d.join("new file.txt"), "n\n").unwrap();
        fs::write(d.join("noise.log"), "n\n").unwrap();
        fs::create_dir_all(d.join("build")).unwrap();
        fs::write(d.join("build/out.o"), "n\n").unwrap();
        fs::write(d.join("sub/deep/inner.txt"), "two\n").unwrap();
        fs::write(d.join("-rf $(x);`y`.txt"), "n\n").unwrap();
        let top = status(&d, me(), &path_env(), TIMEOUT).unwrap();
        let n = names(&top);
        let has = |name: &str, code: u8| n.contains(&(name.to_string(), code));
        assert!(has("changed.txt", 1), "{n:?}");
        assert!(has("gone.txt", 1), "{n:?}");
        assert!(has("new file.txt", 2), "{n:?}");
        assert!(has("-rf $(x);`y`.txt", 2), "{n:?}");
        assert!(has("noise.log", 3), "{n:?}");
        assert!(has("build", 3), "{n:?}");
        assert!(has("sub", 1), "{n:?}");
        assert!(!n.iter().any(|(k, _)| k == "tracked.txt"), "{n:?}");
        // From inside a sub-folder only that folder's items.
        let sub = status(&d.join("sub"), me(), &path_env(), TIMEOUT).unwrap();
        assert_eq!(names(&sub), vec![("deep".to_string(), 1)]);
        // Inside an ignored folder everything is ignored.
        let ign = status(&d.join("build"), me(), &path_env(), TIMEOUT).unwrap();
        assert!(ign.all_ignored || ign.items.values().all(|b| *b == Badge::Ignored));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_folder_that_is_not_in_a_work_tree_is_not_looked_at() {
        let d = tmp("plain");
        assert_eq!(
            status(&d, me(), &path_env(), TIMEOUT).unwrap_err(),
            Skip::NotARepo
        );
        assert_eq!(
            status(&d.join("missing"), me(), &path_env(), TIMEOUT).unwrap_err(),
            Skip::NotARepo
        );
        assert_eq!(find_repo(&d), None);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn git_missing_from_path_is_reported_not_run() {
        if !git_available() {
            return;
        }
        let d = fixture("nogit");
        // PATH is a folder without git: it is not found, and nothing else is tried.
        let empty = tmp("nogit-path");
        assert_eq!(
            status(&d, me(), empty.to_str().unwrap(), TIMEOUT).unwrap_err(),
            Skip::NoGit
        );
        let _ = fs::remove_dir_all(&d);
        let _ = fs::remove_dir_all(&empty);
    }

    #[test]
    fn a_repository_of_another_user_is_skipped() {
        if !git_available() {
            return;
        }
        let d = fixture("owner");
        // Another user's uid: the work tree is not theirs.
        assert_eq!(
            status(&d, me().wrapping_add(4242), &path_env(), TIMEOUT).unwrap_err(),
            Skip::OtherOwner
        );
        // As root, hand the repository to somebody else for real.
        if me() == 0 {
            // SAFETY: chown on paths of this test's own temporary folder.
            let c = std::ffi::CString::new(d.join(".git").as_os_str().as_bytes()).unwrap();
            let rc = unsafe { libc::chown(c.as_ptr(), 4242, 4242) };
            assert_eq!(rc, 0);
            assert_eq!(
                status(&d, me(), &path_env(), TIMEOUT).unwrap_err(),
                Skip::OtherOwner
            );
        }
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_hostile_repository_runs_nothing() {
        if !git_available() {
            return;
        }
        // The program a repository's own configuration names must not run.
        let d = fixture("hostile");
        let marker = d.join("PWNED");
        let script = d.join("evil.sh");
        fs::write(
            &script,
            format!("#!/bin/sh\ntouch '{}'\nexit 0\n", marker.display()),
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        // 1. A fast-file-system-monitor hook, a hook path, and a hook in the repository.
        let cfg = d.join(".git/config");
        let mut text = fs::read_to_string(&cfg).unwrap();
        text.push_str(&format!(
            "[core]\n\tfsmonitor = {s}\n\thooksPath = {h}\n",
            s = script.display(),
            h = d.join(".git/hooks").display()
        ));
        fs::write(&cfg, text).unwrap();
        for hook in ["post-index-change", "pre-commit", "post-checkout"] {
            let p = d.join(".git/hooks").join(hook);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
            fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        }
        // Make the index stale, so that an unprotected status would rewrite it.
        fs::write(d.join("changed.txt"), "touched\n").unwrap();
        let r = status(&d, me(), &path_env(), TIMEOUT).unwrap();
        assert!(!marker.exists(), "a program of the repository ran");
        assert_eq!(r.items.get(&b"changed.txt"[..]), Some(&Badge::Modified));
        // 2. A clean filter named for every file: the repository is skipped, and the filter does not run.
        fs::write(d.join(".gitattributes"), "* filter=evil\n").unwrap();
        let mut text = fs::read_to_string(&cfg).unwrap();
        text.push_str(&format!(
            "[filter \"evil\"]\n\tclean = {}\n",
            script.display()
        ));
        fs::write(&cfg, text).unwrap();
        fs::write(d.join("changed.txt"), "touched again\n").unwrap();
        assert_eq!(
            status(&d, me(), &path_env(), TIMEOUT).unwrap_err(),
            Skip::UnsafeConfig
        );
        assert!(!marker.exists(), "a filter of the repository ran");
        // 3. An include.
        let mut text = fs::read_to_string(&cfg).unwrap();
        text = text.replace("[filter \"evil\"]", "[include]\n\tpath = elsewhere\n[x]");
        fs::write(&cfg, text).unwrap();
        assert_eq!(
            status(&d, me(), &path_env(), TIMEOUT).unwrap_err(),
            Skip::UnsafeConfig
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_git_that_never_answers_is_stopped() {
        // A fake `git` that sleeps: the timeout ends it and says so.
        let d = tmp("slow");
        let bin = tmp("slow-bin");
        let repo = d.join("r");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::write(repo.join(".git/config"), "[core]\n").unwrap();
        let fake = bin.join("git");
        fs::write(&fake, "#!/bin/sh\nexec sleep 30\n").unwrap();
        fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
        let t0 = Instant::now();
        let r = status(
            &repo,
            me(),
            &format!("{}:/usr/bin:/bin", bin.display()),
            Duration::from_millis(300),
        );
        assert_eq!(r.unwrap_err(), Skip::Timeout);
        assert!(t0.elapsed() < Duration::from_secs(5));
        // A git that fails.
        fs::write(&fake, "#!/bin/sh\nexit 3\n").unwrap();
        assert!(matches!(
            status(
                &repo,
                me(),
                &format!("{}:/usr/bin:/bin", bin.display()),
                TIMEOUT
            ),
            Err(Skip::Failed(_))
        ));
        // A git that prints far too much is cut off.
        fs::write(&fake, "#!/bin/sh\nexec yes '? x'\n").unwrap();
        let r = status(
            &repo,
            me(),
            &format!("{}:/usr/bin:/bin", bin.display()),
            Duration::from_secs(20),
        );
        assert!(matches!(r, Err(Skip::Failed(_))), "{r:?}");
        let _ = fs::remove_dir_all(&d);
        let _ = fs::remove_dir_all(&bin);
    }

    #[test]
    fn a_linked_work_tree_is_followed() {
        if !git_available() {
            return;
        }
        let d = fixture("linked");
        let wt = d
            .parent()
            .unwrap()
            .join(format!("telamon-git-{}-linked-wt", std::process::id()));
        let _ = fs::remove_dir_all(&wt);
        git(
            &d,
            &["worktree", "add", "-q", "-b", "other", wt.to_str().unwrap()],
        );
        fs::write(wt.join("fresh.txt"), "x\n").unwrap();
        let r = status(&wt, me(), &path_env(), TIMEOUT).unwrap();
        assert_eq!(r.items.get(&b"fresh.txt"[..]), Some(&Badge::New));
        let _ = fs::remove_dir_all(&d);
        let _ = fs::remove_dir_all(&wt);
    }
}
