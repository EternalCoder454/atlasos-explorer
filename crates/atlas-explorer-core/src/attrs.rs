//! Changes to an item's tags, star rating and permissions, made by the
//! operation queue on a worker and written down so Undo can take them back.
//! A change knows the value before and after as text (tags: the attribute's
//! comma-separated value; rating: 0 to 10 or empty; mode: octal), and undoing
//! it first checks the item still has the value the change left. Blocking
//! file calls: run from a worker.

use crate::perms;
use crate::tags;
use crate::xattr;
use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Tags,
    Rating,
    Mode,
}

impl Key {
    pub fn name(self) -> &'static str {
        match self {
            Key::Tags => "tags",
            Key::Rating => "rating",
            Key::Mode => "mode",
        }
    }

    pub fn from_name(s: &str) -> Option<Key> {
        match s {
            "tags" => Some(Key::Tags),
            "rating" => Some(Key::Rating),
            "mode" => Some(Key::Mode),
            _ => None,
        }
    }
}

/// One value of one item changed from `before` to `after`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub path: PathBuf,
    pub key: Key,
    pub before: String,
    pub after: String,
}

/// What to do to an item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    Tags {
        add: Vec<String>,
        remove: Vec<String>,
        clear: bool,
    },
    /// 0 to 10; 0 removes the rating.
    Rating(u8),
    /// Bits of the nine to turn on and off.
    Mode { set: u32, clear: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    Xattr(xattr::Error),
    /// The tags are not text, so they are not rewritten.
    Foreign,
    TagLimit(tags::EditProblem),
    /// The permissions of an item are the owner's to change.
    NotOwner,
    /// A link: its permissions are its target's.
    Link,
    /// The item is gone.
    Gone,
    /// The value is not what the change left, so it was not undone.
    Changed,
    Other(i32),
    Cancelled,
}

impl Problem {
    pub fn text(&self) -> String {
        match self {
            Problem::Xattr(e) => e.text().to_string(),
            Problem::Foreign => "The tags of this item are in a form Files doesn't change.".into(),
            Problem::TagLimit(e) => e.text().to_string(),
            Problem::NotOwner => "Only the owner can change who may use this item.".into(),
            Problem::Link => "Links use the permissions of what they point to.".into(),
            Problem::Gone => "The item is gone.".into(),
            Problem::Changed => "It was changed since, so it was left as it is.".into(),
            Problem::Other(_) => "The change couldn't be saved.".into(),
            Problem::Cancelled => "Stopped.".into(),
        }
    }

    /// Whether the file system itself can't keep tags (the one thing the
    /// window says in particular words).
    pub fn is_unsupported(&self) -> bool {
        matches!(self, Problem::Xattr(xattr::Error::Unsupported))
    }
}

fn errno_problem(e: &std::io::Error) -> Problem {
    match e.raw_os_error() {
        Some(libc::EPERM) | Some(libc::EACCES) => Problem::NotOwner,
        Some(libc::ENOENT) | Some(libc::ENOTDIR) => Problem::Gone,
        Some(n) => Problem::Other(n),
        None => Problem::Other(0),
    }
}

// ---- Values ----

fn read_mode(path: &Path) -> Result<u32, Problem> {
    let md = fs::symlink_metadata(path).map_err(|e| errno_problem(&e))?;
    if md.file_type().is_symlink() {
        return Err(Problem::Link);
    }
    Ok(md.mode() & perms::ALL_BITS)
}

/// Changes the mode of the file or folder at `path`, never of what a link
/// there points to. The entry is opened as itself (`O_PATH | O_NOFOLLOW`, so a
/// link is the link), looked at through the descriptor, and changed through
/// it (`/proc/self/fd/<n>` names that very inode): a name that was swapped
/// for a link between the walk that found it and this call is refused, not
/// followed to a file of the person's that nobody meant to change.
fn write_mode(path: &Path, mode: u32) -> Result<(), Problem> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    let handle = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|e| match e.raw_os_error() {
            Some(libc::ELOOP) => Problem::Link,
            _ => errno_problem(&e),
        })?;
    let md = handle.metadata().map_err(|e| errno_problem(&e))?;
    if md.file_type().is_symlink() {
        return Err(Problem::Link);
    }
    let via = std::path::PathBuf::from(format!("/proc/self/fd/{}", handle.as_raw_fd()));
    fs::set_permissions(&via, fs::Permissions::from_mode(mode & perms::ALL_BITS))
        .map_err(|e| errno_problem(&e))
}

/// The value of `key` as text ("" for none).
pub fn read(path: &Path, key: Key) -> Result<String, Problem> {
    match key {
        Key::Tags => match xattr::get(path, xattr::TAGS).map_err(Problem::Xattr)? {
            None => Ok(String::new()),
            Some(raw) if tags::is_clean(&raw) => Ok(String::from_utf8_lossy(&raw).into_owned()),
            Some(_) => Err(Problem::Foreign),
        },
        Key::Rating => match xattr::get(path, xattr::RATING).map_err(Problem::Xattr)? {
            None => Ok(String::new()),
            Some(raw) => Ok(tags::rating_parse(&raw)
                .filter(|r| *r > 0)
                .map(|r| r.to_string())
                .unwrap_or_default()),
        },
        Key::Mode => Ok(perms::octal(read_mode(path)?)),
    }
}

/// Makes the value of `key` `value` (text as `read` gives it).
pub fn write(path: &Path, key: Key, value: &str) -> Result<(), Problem> {
    match key {
        Key::Tags => xattr::set(path, xattr::TAGS, value.as_bytes()).map_err(Problem::Xattr),
        Key::Rating => xattr::set(path, xattr::RATING, value.as_bytes()).map_err(Problem::Xattr),
        Key::Mode => {
            let mode = u32::from_str_radix(value, 8).map_err(|_| Problem::Other(libc::EINVAL))?;
            write_mode(path, mode)
        }
    }
}

// ---- One item ----

/// Makes `edit` to `path`. Returns the change made, `None` when the item
/// already was as asked.
pub fn apply_edit(path: &Path, edit: &Edit) -> Result<Option<Change>, Problem> {
    let (key, before, after) = match edit {
        Edit::Tags { add, remove, clear } => {
            let before = read(path, Key::Tags)?;
            let now = tags::parse(before.as_bytes());
            let next = tags::apply_edit(&now, add, remove, *clear).map_err(Problem::TagLimit)?;
            let after = tags::encode(&next)
                .map(|v| String::from_utf8_lossy(&v).into_owned())
                .unwrap_or_default();
            (Key::Tags, before, after)
        }
        Edit::Rating(r) => {
            let before = read(path, Key::Rating)?;
            let after = tags::rating_encode(*r)
                .map(|v| String::from_utf8_lossy(&v).into_owned())
                .unwrap_or_default();
            (Key::Rating, before, after)
        }
        Edit::Mode { set, clear } => {
            let cur = read_mode(path)?;
            (
                Key::Mode,
                perms::octal(cur),
                perms::octal(perms::edited(cur, *set, *clear)),
            )
        }
    };
    // Compared as the lists they mean: "Red,Blue" is not a change from "Red, Blue".
    let same = match key {
        Key::Tags => tags::parse(before.as_bytes()) == tags::parse(after.as_bytes()),
        _ => before == after,
    };
    if same {
        return Ok(None);
    }
    write(path, key, &after)?;
    Ok(Some(Change {
        path: path.to_path_buf(),
        key,
        before,
        after,
    }))
}

/// Puts `key` of `path` back from `expect` to `set`, only when it still is
/// `expect`.
pub fn apply_set(path: &Path, key: Key, expect: &str, set: &str) -> Result<(), Problem> {
    check_set(path, key, expect)?;
    write(path, key, set)
}

/// Whether `key` of `path` still is `expect`.
pub fn check_set(path: &Path, key: Key, expect: &str) -> Result<(), Problem> {
    if read(path, key)? == expect {
        Ok(())
    } else {
        Err(Problem::Changed)
    }
}

// ---- Many items ----

/// What a run over several items did.
#[derive(Debug, Default)]
pub struct Outcome {
    /// Changes made, in the order of the items (folders before what is in them).
    pub changes: Vec<Change>,
    /// Items looked at.
    pub items: u64,
    /// Items that could not be changed.
    pub failed: u64,
    /// The first reason, for the window.
    pub problem: Option<Problem>,
    /// More changes than can be written down for Undo, or one that can't be
    /// (a value or a name with a control character): `changes` is empty.
    pub overflow: bool,
    pub cancelled: bool,
}

/// The most changes written down for one operation's Undo.
pub const MAX_RECORDED: usize = 20_000;

impl Outcome {
    fn note(&mut self, p: Problem) {
        self.failed += 1;
        if self.problem.is_none() {
            self.problem = Some(p);
        }
    }

    fn push(&mut self, c: Change) {
        if self.overflow {
            return;
        }
        // A value with a control character (a line break in a foreign tag)
        // can't be written down safely: the change is made, not undoable.
        let plain = |s: &str| !s.chars().any(char::is_control);
        if !plain(&c.before)
            || !plain(&c.after)
            || c.path
                .as_os_str()
                .as_bytes()
                .iter()
                .any(|b| b.is_ascii_control())
        {
            self.overflow = true;
            self.changes.clear();
            return;
        }
        if self.changes.len() >= MAX_RECORDED {
            self.overflow = true;
            self.changes.clear();
            return;
        }
        self.changes.push(c);
    }
}

/// Makes `edit` to every path; each is tried, the ones that fail are counted.
pub fn run_edit(paths: &[PathBuf], edit: &Edit, cancel: &AtomicBool) -> Outcome {
    let mut out = Outcome::default();
    for p in paths {
        if cancel.load(Ordering::Relaxed) {
            out.cancelled = true;
            break;
        }
        out.items += 1;
        match apply_edit(p, edit) {
            Ok(Some(c)) => out.push(c),
            Ok(None) => {}
            Err(e) => out.note(e),
        }
    }
    out
}

/// Takes changes back (or makes them again): `rev` swaps before and after.
/// Nothing is left changed unless every item still has the value the change
/// left. Modes are written so that no folder stops being searchable before
/// what is in it is done: they are widened on the way in (so the check can
/// reach everything), and narrowed last, what is in a folder first.
pub fn run_revert(changes: &[Change], rev: bool, cancel: &AtomicBool) -> Outcome {
    let mut out = Outcome::default();
    if cancel.load(Ordering::Relaxed) {
        out.cancelled = true;
        return out;
    }
    let (expect, set): (Vec<&String>, Vec<&String>) = if rev {
        (
            changes.iter().map(|c| &c.after).collect(),
            changes.iter().map(|c| &c.before).collect(),
        )
    } else {
        (
            changes.iter().map(|c| &c.before).collect(),
            changes.iter().map(|c| &c.after).collect(),
        )
    };
    let bits = |s: &String| u32::from_str_radix(s, 8).unwrap_or(0);
    // Pass one, in order (a folder before what is in it): check each item and
    // widen its mode.
    let mut widened: Vec<usize> = Vec::new();
    for (i, c) in changes.iter().enumerate() {
        out.items += 1;
        if let Err(e) = check_set(&c.path, c.key, expect[i]) {
            out.note(e);
            break;
        }
        if c.key == Key::Mode {
            let both = bits(expect[i]) | bits(set[i]);
            if both != bits(expect[i]) {
                if let Err(e) = write(&c.path, Key::Mode, &perms::octal(both)) {
                    out.note(e);
                    break;
                }
                widened.push(i);
            }
        }
    }
    if out.failed > 0 {
        // Nothing is to change: the widened modes go back.
        for &i in widened.iter().rev() {
            let _ = write(&changes[i].path, Key::Mode, expect[i]);
        }
        return out;
    }
    for (i, c) in changes.iter().enumerate() {
        if c.key != Key::Mode
            && let Err(e) = write(&c.path, c.key, set[i])
        {
            out.note(e);
        }
    }
    for (i, c) in changes.iter().enumerate().rev() {
        if c.key == Key::Mode
            && let Err(e) = write(&c.path, Key::Mode, set[i])
        {
            out.note(e);
        }
    }
    out
}

// ---- A folder and what is in it ----

/// Changes the permissions of `root` and everything below it (links are not
/// followed, other file systems are not entered): the bits in `set` are turned
/// on and the bits in `clear` off. Files found inside a folder keep their run
/// bits (only folders get `RUN` changes) so a folder's change doesn't turn
/// documents into programs; a file given as `root` gets what was asked.
pub fn run_mode_tree(root: &Path, set: u32, clear: u32, cancel: &AtomicBool) -> Outcome {
    let mut out = Outcome::default();
    let Ok(md) = fs::symlink_metadata(root) else {
        out.note(Problem::Gone);
        return out;
    };
    let dev = md.dev();
    // Folders whose bits are taken away at the end, parents first.
    let mut later: Vec<(PathBuf, u32)> = Vec::new();
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    // Depth first and in order: pop the last, push its folders reversed.
    while let Some(path) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            out.cancelled = true;
            break;
        }
        let Ok(md) = fs::symlink_metadata(&path) else {
            out.note(Problem::Gone);
            continue;
        };
        if md.file_type().is_symlink() || md.dev() != dev {
            continue;
        }
        out.items += 1;
        let is_dir = md.is_dir();
        let cur = md.mode() & perms::ALL_BITS;
        // A file named on its own keeps what was asked for it; files found
        // inside a folder keep their run bits.
        let (s, c) = if is_dir || path == root {
            (set, clear)
        } else {
            (set & !perms::RUN, clear & !perms::RUN)
        };
        let target = perms::edited(cur, s, c);
        if target != cur {
            // Bits come on first; a folder's bits come off after what is in it.
            let first = if is_dir {
                cur | (target & perms::RWX)
            } else {
                target
            };
            let r = if first != cur {
                write_mode(&path, first)
            } else {
                Ok(())
            };
            match r {
                Ok(()) => {
                    if is_dir && first != target {
                        later.push((path.clone(), target));
                    }
                    out.push(Change {
                        path: path.clone(),
                        key: Key::Mode,
                        before: perms::octal(cur),
                        after: perms::octal(target),
                    });
                }
                Err(e) => out.note(e),
            }
        }
        if is_dir {
            match fs::read_dir(&path) {
                Ok(rd) => {
                    let mut kids: Vec<PathBuf> = Vec::new();
                    for e in rd.flatten() {
                        kids.push(path.join(OsStr::from_bytes(e.file_name().as_bytes())));
                    }
                    kids.sort();
                    stack.extend(kids.into_iter().rev());
                }
                Err(_) => out.note(Problem::Other(libc::EACCES)),
            }
        }
    }
    for (path, target) in later.into_iter().rev() {
        if let Err(e) = write_mode(&path, target) {
            out.note(e);
        }
    }
    out
}

/// The user id of this process.
pub fn current_uid() -> u32 {
    // SAFETY: getuid has no failure and touches no memory.
    unsafe { libc::getuid() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("telamon-attrs-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn keeps_xattrs(dir: &Path) -> bool {
        let ok = xattr::probe(dir).is_ok();
        if !ok {
            eprintln!("skipped: {} keeps no user attributes", dir.display());
        }
        ok
    }

    fn no() -> AtomicBool {
        AtomicBool::new(false)
    }

    fn tag_edit(add: &[&str], remove: &[&str]) -> Edit {
        Edit::Tags {
            add: add.iter().map(|s| s.to_string()).collect(),
            remove: remove.iter().map(|s| s.to_string()).collect(),
            clear: false,
        }
    }

    #[test]
    fn tags_are_added_removed_and_undone() {
        let dir = scratch("tags");
        if !keeps_xattrs(&dir) {
            return;
        }
        let a = dir.join("a");
        let b = dir.join("b");
        fs::write(&a, b"1").unwrap();
        fs::write(&b, b"2").unwrap();
        xattr::set(&b, xattr::TAGS, b"Work").unwrap();
        let out = run_edit(&[a.clone(), b.clone()], &tag_edit(&["Red"], &[]), &no());
        assert_eq!(out.failed, 0);
        assert_eq!(out.changes.len(), 2);
        assert_eq!(read(&a, Key::Tags).unwrap(), "Red");
        assert_eq!(read(&b, Key::Tags).unwrap(), "Work,Red");
        // Already as asked: no change is written down.
        let again = run_edit(std::slice::from_ref(&a), &tag_edit(&["red"], &[]), &no());
        assert!(again.changes.is_empty());
        // Undo, then redo.
        let back = run_revert(&out.changes, true, &no());
        assert_eq!(back.failed, 0, "{:?}", back.problem);
        assert_eq!(read(&a, Key::Tags).unwrap(), "");
        assert_eq!(read(&b, Key::Tags).unwrap(), "Work");
        assert!(xattr::get(&a, xattr::TAGS).unwrap().is_none());
        let fwd = run_revert(&out.changes, false, &no());
        assert_eq!(fwd.failed, 0);
        assert_eq!(read(&b, Key::Tags).unwrap(), "Work,Red");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_undo_is_refused_when_the_value_changed() {
        let dir = scratch("stale");
        if !keeps_xattrs(&dir) {
            return;
        }
        let a = dir.join("a");
        fs::write(&a, b"1").unwrap();
        let out = run_edit(std::slice::from_ref(&a), &tag_edit(&["Red"], &[]), &no());
        xattr::set(&a, xattr::TAGS, b"Red,Blue").unwrap();
        let back = run_revert(&out.changes, true, &no());
        assert_eq!(back.problem, Some(Problem::Changed));
        assert_eq!(read(&a, Key::Tags).unwrap(), "Red,Blue");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn foreign_tags_are_never_rewritten() {
        let dir = scratch("foreign");
        if !keeps_xattrs(&dir) {
            return;
        }
        let a = dir.join("a");
        fs::write(&a, b"1").unwrap();
        xattr::set(&a, xattr::TAGS, b"Re\xffd").unwrap();
        let out = run_edit(std::slice::from_ref(&a), &tag_edit(&["Blue"], &[]), &no());
        assert_eq!(out.problem, Some(Problem::Foreign));
        assert_eq!(xattr::get(&a, xattr::TAGS).unwrap().unwrap(), b"Re\xffd");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn ratings() {
        let dir = scratch("rating");
        if !keeps_xattrs(&dir) {
            return;
        }
        let a = dir.join("a");
        fs::write(&a, b"1").unwrap();
        let out = run_edit(std::slice::from_ref(&a), &Edit::Rating(8), &no());
        assert_eq!(read(&a, Key::Rating).unwrap(), "8");
        // The attribute is the text Baloo writes.
        assert_eq!(xattr::get(&a, xattr::RATING).unwrap().unwrap(), b"8");
        let clear = run_edit(std::slice::from_ref(&a), &Edit::Rating(0), &no());
        assert_eq!(clear.changes.len(), 1);
        assert!(xattr::get(&a, xattr::RATING).unwrap().is_none());
        run_revert(&clear.changes, true, &no());
        assert_eq!(read(&a, Key::Rating).unwrap(), "8");
        let _ = out;
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_mode_is_never_written_through_a_link() {
        use std::os::unix::fs::symlink;
        let dir = scratch("modelink");
        let secret = dir.join("secret");
        fs::write(&secret, b"s").unwrap();
        fs::set_permissions(&secret, fs::Permissions::from_mode(0o600)).unwrap();
        // The name was a file when the walk saw it; now it is a link.
        let swapped = dir.join("swapped");
        symlink(&secret, &swapped).unwrap();
        assert_eq!(write_mode(&swapped, 0o777), Err(Problem::Link));
        assert_eq!(fs::metadata(&secret).unwrap().mode() & 0o777, 0o600);
        // a link to a folder, and a dangling one
        let tree = dir.join("tree");
        fs::create_dir_all(&tree).unwrap();
        symlink(&secret, tree.join("l")).unwrap();
        symlink(dir.join("nope"), tree.join("dangling")).unwrap();
        fs::write(tree.join("f"), b"x").unwrap();
        fs::set_permissions(tree.join("f"), fs::Permissions::from_mode(0o600)).unwrap();
        let out = run_mode_tree(&tree, 0o004, 0, &AtomicBool::new(false));
        assert_eq!(
            fs::metadata(&secret).unwrap().mode() & 0o777,
            0o600,
            "{out:?}"
        );
        assert_eq!(fs::metadata(tree.join("f")).unwrap().mode() & 0o777, 0o604);
        // a real file still works
        write_mode(&tree.join("f"), 0o640).unwrap();
        assert_eq!(fs::metadata(tree.join("f")).unwrap().mode() & 0o777, 0o640);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn modes_of_files() {
        let dir = scratch("mode");
        let a = dir.join("a");
        fs::write(&a, b"1").unwrap();
        write_mode(&a, 0o644).unwrap();
        let out = run_edit(
            std::slice::from_ref(&a),
            &Edit::Mode {
                set: 0o020,
                clear: 0o004,
            },
            &no(),
        );
        assert_eq!(read_mode(&a).unwrap(), 0o660);
        assert_eq!(out.changes[0].before, "644");
        assert_eq!(out.changes[0].after, "660");
        run_revert(&out.changes, true, &no());
        assert_eq!(read_mode(&a).unwrap(), 0o644);
        // A link is not changed.
        let l = dir.join("l");
        std::os::unix::fs::symlink(&a, &l).unwrap();
        let out = run_edit(
            std::slice::from_ref(&l),
            &Edit::Mode {
                set: 0o001,
                clear: 0,
            },
            &no(),
        );
        assert_eq!(out.problem, Some(Problem::Link));
        assert_eq!(read_mode(&a).unwrap(), 0o644);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn modes_through_a_tree() {
        let dir = scratch("tree");
        let top = dir.join("top");
        fs::create_dir_all(top.join("sub/deep")).unwrap();
        fs::write(top.join("f.txt"), b"1").unwrap();
        fs::write(top.join("sub/g.sh"), b"2").unwrap();
        fs::write(top.join("sub/deep/h"), b"3").unwrap();
        std::os::unix::fs::symlink("/", top.join("link")).unwrap();
        for p in ["top", "top/sub", "top/sub/deep"] {
            write_mode(&dir.join(p), 0o755).unwrap();
        }
        write_mode(&top.join("f.txt"), 0o644).unwrap();
        write_mode(&top.join("sub/g.sh"), 0o755).unwrap();
        write_mode(&top.join("sub/deep/h"), 0o644).unwrap();
        // Others lose all access: folders and files alike; files keep their run bits.
        let (set, clear) = perms::difference(0o755, 0o750);
        let out = run_mode_tree(&top, set, clear, &no());
        assert_eq!(out.failed, 0, "{:?}", out.problem);
        assert_eq!(read_mode(&top).unwrap(), 0o750);
        assert_eq!(read_mode(&top.join("sub/deep")).unwrap(), 0o750);
        assert_eq!(read_mode(&top.join("f.txt")).unwrap(), 0o640);
        // Files keep their run bits.
        assert_eq!(read_mode(&top.join("sub/g.sh")).unwrap(), 0o751);
        assert_eq!(out.changes.len(), 6, "{:?}", out.changes);
        // Undo puts back every mode.
        let back = run_revert(&out.changes, true, &no());
        assert_eq!(back.failed, 0, "{:?}", back.problem);
        assert_eq!(read_mode(&top).unwrap(), 0o755);
        assert_eq!(read_mode(&top.join("sub/g.sh")).unwrap(), 0o755);
        assert_eq!(read_mode(&top.join("f.txt")).unwrap(), 0o644);
        // Redo takes them away again, folder by folder in a safe order.
        let again = run_revert(&out.changes, false, &no());
        assert_eq!(again.failed, 0, "{:?}", again.problem);
        assert_eq!(read_mode(&top.join("sub/deep/h")).unwrap(), 0o640);
        // The folder's own search bit can go without breaking the walk.
        let (set, clear) = perms::difference(0o750, 0o640);
        let out = run_mode_tree(&top, set, clear, &no());
        assert_eq!(out.failed, 0, "{:?}", out.problem);
        assert_eq!(read_mode(&top).unwrap(), 0o640);
        // The dir has no search bit now: put them back to clean up.
        let back = run_revert(&out.changes, true, &no());
        assert_eq!(back.failed, 0, "{:?}", back.problem);
        assert_eq!(read_mode(&top).unwrap(), 0o750);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn values_that_cant_be_written_down_are_done_but_not_undoable() {
        let dir = scratch("ctrl");
        if !keeps_xattrs(&dir) {
            return;
        }
        let a = dir.join("a");
        fs::write(&a, b"1").unwrap();
        xattr::set(&a, xattr::TAGS, b"Odd\tone").unwrap();
        let out = run_edit(std::slice::from_ref(&a), &tag_edit(&["Red"], &[]), &no());
        assert_eq!(out.failed, 0);
        assert!(out.overflow && out.changes.is_empty());
        assert_eq!(read(&a, Key::Tags).unwrap(), "Odd\tone,Red");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_file_given_on_its_own_gets_its_run_bit_changed() {
        let dir = scratch("rootrun");
        let f = dir.join("f");
        fs::write(&f, b"1").unwrap();
        write_mode(&f, 0o644).unwrap();
        let out = run_mode_tree(&f, 0o100, 0, &no());
        assert_eq!(read_mode(&f).unwrap(), 0o744);
        assert_eq!(out.changes.len(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_tree_run_stops_when_cancelled() {
        let dir = scratch("cancel");
        fs::write(dir.join("a"), b"1").unwrap();
        let yes = AtomicBool::new(true);
        let out = run_mode_tree(&dir, 0o020, 0, &yes);
        assert!(out.cancelled);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn keys_have_names() {
        for k in [Key::Tags, Key::Rating, Key::Mode] {
            assert_eq!(Key::from_name(k.name()), Some(k));
        }
        assert_eq!(Key::from_name("x"), None);
        assert!(Problem::Xattr(xattr::Error::Unsupported).is_unsupported());
    }
}
