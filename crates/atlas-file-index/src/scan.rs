//! The scanner: builds an [`Index`] from the file system, or rebuilds one with
//! some folders listed again. Both are one routine, so the rules (what is
//! excluded, never crossing filesystems, never following symlinks) are in one
//! place.
//!
//! A folder's record keeps its modification time and, in `size`, the
//! nanoseconds of it: a folder's size is not shown, and the extra precision lets
//! the reconcile walk see a change made in the same second as the scan.

use crate::category::{Category, category_of};
use crate::config::{
    CACHEDIR_SIGNATURE, DirDecision, Excludes, is_cachedir_tag, marker_excludes, parse_hidden_file,
};
use crate::index::{
    FLAG_DIR, FLAG_EXEC, FLAG_HIDDEN, FLAG_RECENT, Index, IndexBuilder, MAX_DEPTH, NONE,
    RECENT_SECS,
};
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

// Known and accepted: the scanner works on paths, not on open directory
// handles, so a folder swapped for a symlink between a check and a listing can
// be followed once. It is bounded by MAX_DEPTH, the record cap and the device
// check, and it can only show names the user can already read.
/// Error messages kept per scan.
const MAX_ERRORS: usize = 20;
const HIDDEN_FILE_MAX: u64 = 1 << 20;

pub struct ScanResult {
    pub index: Index,
    /// For a rebuild: the new id of every old folder, `NONE` for folders that
    /// are gone (files of a listed folder are made again and not mapped).
    pub map: Vec<u32>,
    /// Plain-words problems (folders that could not be read).
    pub errors: Vec<String>,
    /// How many configured roots could not be scanned.
    pub failed_roots: usize,
    /// The scan stopped on request; the index is partial and must not be used.
    pub stopped: bool,
    /// The index reached its size limit.
    pub full: bool,
}

enum Listing {
    Done,
    /// A marker file (`CACHEDIR.TAG`, `pyvenv.cfg`) excludes the folder.
    Excluded,
    Failed(std::io::Error),
}

struct Scanner<'a> {
    b: IndexBuilder,
    excl: &'a Excludes,
    stop: &'a AtomicBool,
    old: Option<&'a Index>,
    /// Folders to list again (old ids, sorted).
    dirty: &'a [u32],
    map: Vec<u32>,
    errors: Vec<String>,
    stopped: bool,
    full: bool,
}

/// Index every root from scratch. `used` is the last-use table (see `recent`).
pub fn scan_full(
    roots: &[PathBuf],
    excl: &Excludes,
    stop: &AtomicBool,
    used: &HashMap<u64, i64>,
) -> ScanResult {
    let mut s = Scanner::new(excl, stop, None, &[]);
    let mut failed = 0;
    for root in roots {
        if s.stopped || s.full {
            break;
        }
        if !s.scan_root(root) {
            failed += 1;
        }
    }
    s.finish(used, failed)
}

/// A new index from `old` with the folders `dirty` (sorted old ids) listed
/// again; the subtrees of the others are copied.
pub fn rebuild(
    old: &Index,
    dirty: &[u32],
    excl: &Excludes,
    stop: &AtomicBool,
    used: &HashMap<u64, i64>,
) -> ScanResult {
    let mut s = Scanner::new(excl, stop, Some(old), dirty);
    let mut path = PathBuf::new();
    for root in old.roots() {
        if s.stopped || s.full {
            break;
        }
        s.visit_old(root, NONE, &mut path);
    }
    s.finish(used, 0)
}

impl<'a> Scanner<'a> {
    fn new(
        excl: &'a Excludes,
        stop: &'a AtomicBool,
        old: Option<&'a Index>,
        dirty: &'a [u32],
    ) -> Self {
        Scanner {
            b: IndexBuilder::new(),
            excl,
            stop,
            old,
            dirty,
            map: vec![NONE; old.map_or(0, Index::len)],
            errors: Vec::new(),
            stopped: false,
            full: false,
        }
    }

    fn finish(self, used: &HashMap<u64, i64>, failed_roots: usize) -> ScanResult {
        ScanResult {
            index: self.b.finish(used),
            map: self.map,
            errors: self.errors,
            failed_roots,
            stopped: self.stopped,
            full: self.full,
        }
    }

    fn error(&mut self, msg: String) {
        log::debug!("{msg}");
        if self.errors.len() < MAX_ERRORS {
            self.errors.push(msg);
        }
    }

    fn check_stop(&mut self) -> bool {
        if self.stop.load(Ordering::Relaxed) {
            self.stopped = true;
        }
        self.stopped
    }

    /// Returns false when the root could not be scanned at all.
    fn scan_root(&mut self, root: &Path) -> bool {
        let md = match fs::metadata(root) {
            Ok(m) if m.is_dir() => m,
            Ok(_) => {
                self.error(format!("{} is not a folder", root.display()));
                return false;
            }
            Err(e) => {
                self.error(format!("{} cannot be read: {e}", root.display()));
                return false;
            }
        };
        let Some(id) = self.b.push(
            NONE,
            root.as_os_str().as_bytes(),
            FLAG_DIR | recent_flag(md.mtime()),
            Category::Folder,
            md.mtime(),
            md.mtime_nsec() as u64,
        ) else {
            self.full = true;
            return false;
        };
        let mut path = root.to_path_buf();
        if let Listing::Failed(e) = self.list_dir(id, &mut path, md.dev(), None, true, 0) {
            self.error(format!("{} cannot be read: {e}", root.display()));
            return false;
        }
        true
    }

    /// Is any dirty folder in the old id range `from..to`?
    fn dirty_in(&self, from: u32, to: u32) -> bool {
        let i = self.dirty.partition_point(|&d| d < from);
        self.dirty.get(i).is_some_and(|&d| d < to)
    }

    /// Carry old record `old_id` and its subtree over, listing again the dirty
    /// folders in it. `path` is the parent's path (empty for a root).
    fn visit_old(&mut self, old_id: u32, new_parent: u32, path: &mut PathBuf) {
        if self.check_stop() || self.full {
            return;
        }
        let Some(old) = self.old else { return };
        let end = old.end(old_id);
        if !self.dirty_in(old_id, end) {
            self.copy_block(old_id, end, new_parent);
            return;
        }
        let keep = path.as_os_str().len();
        path.push(OsStr::from_bytes(old.name(old_id)));
        let is_dirty = self.dirty.binary_search(&old_id).is_ok();
        if is_dirty {
            self.relist(old_id, end, new_parent, path);
        } else if let Some(id) = self.b.push_copy(old, old_id, new_parent) {
            self.map[old_id as usize] = id;
            let mut c = old_id + 1;
            while c < end {
                self.visit_old(c, id, path);
                c = old.end(c);
            }
        } else {
            self.full = true;
        }
        truncate_path(path, keep);
    }

    fn relist(&mut self, old_id: u32, end: u32, new_parent: u32, path: &mut PathBuf) {
        let Some(old) = self.old else { return };
        let md = match fs::symlink_metadata(&*path) {
            Ok(m) if m.is_dir() => m,
            Ok(_) => return, // now something else: its parent sees the change
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return, // gone
            Err(e) => {
                // cannot look at it now: keep what was known
                self.error(format!("{} cannot be read: {e}", path.display()));
                self.copy_block(old_id, end, new_parent);
                return;
            }
        };
        let mark = self.b.mark();
        let Some(id) = self.b.push_copy(old, old_id, new_parent) else {
            self.full = true;
            return;
        };
        self.b.set_times(
            id,
            md.mtime(),
            md.mtime_nsec() as u64,
            recent_flag(md.mtime()) != 0,
        );
        self.map[old_id as usize] = id;
        let is_root = new_parent == NONE;
        match self.list_dir(id, path, md.dev(), Some(old_id), is_root, 0) {
            Listing::Done => {}
            Listing::Excluded => {
                self.b.rollback(mark);
                self.map[old_id as usize..end as usize].fill(NONE);
            }
            Listing::Failed(e) => {
                self.error(format!("{} cannot be read: {e}", path.display()));
            }
        }
    }

    fn copy_block(&mut self, from: u32, to: u32, new_parent: u32) {
        let Some(old) = self.old else { return };
        for i in from..to {
            let parent = if i == from {
                new_parent
            } else {
                self.map[old.record(i).parent as usize]
            };
            match self.b.push_copy(old, i, parent) {
                Some(id) => self.map[i as usize] = id,
                None => {
                    self.full = true;
                    return;
                }
            }
        }
    }

    /// List folder `id` (at `path`, on device `dev`) and add its children.
    /// `old_dir` is the folder's old id when rebuilding: old subfolders are
    /// carried over by name.
    fn list_dir(
        &mut self,
        id: u32,
        path: &mut PathBuf,
        dev: u64,
        old_dir: Option<u32>,
        is_root: bool,
        depth: usize,
    ) -> Listing {
        if self.check_stop() {
            return Listing::Done;
        }
        let rd = match fs::read_dir(&*path) {
            Ok(rd) => rd,
            Err(e) => return Listing::Failed(e),
        };
        let mut entries: Vec<fs::DirEntry> = Vec::new();
        let (mut has_tag, mut has_pyvenv, mut has_hidden) = (false, false, false);
        for e in rd {
            match e {
                Ok(e) => {
                    match e.file_name().as_bytes() {
                        b"CACHEDIR.TAG" => has_tag = true,
                        b"pyvenv.cfg" => has_pyvenv = true,
                        b".hidden" => has_hidden = true,
                        _ => {}
                    }
                    entries.push(e);
                }
                Err(e) => {
                    self.error(format!("{} cannot be read completely: {e}", path.display()));
                    break;
                }
            }
        }
        if !is_root {
            let tag = if has_tag {
                Some(read_cachedir_tag(&path.join("CACHEDIR.TAG")))
            } else {
                None
            };
            if marker_excludes(tag, has_pyvenv) {
                return Listing::Excluded;
            }
        }
        let hidden_names: HashSet<Vec<u8>> = if has_hidden {
            read_hidden_file(&path.join(".hidden"))
        } else {
            HashSet::new()
        };
        let old_children: HashMap<&[u8], u32> = match (self.old, old_dir) {
            (Some(old), Some(d)) => {
                let end = old.end(d);
                let mut m = HashMap::new();
                let mut c = d + 1;
                while c < end {
                    if old.record(c).is_dir() {
                        m.insert(old.name(c), c);
                    }
                    c = old.end(c);
                }
                m
            }
            _ => HashMap::new(),
        };
        let keep = path.as_os_str().len();
        for e in entries {
            if self.full || self.check_stop() {
                break;
            }
            let name_os = e.file_name();
            let name = name_os.as_bytes();
            if name.is_empty() || hidden_names.contains(name) {
                continue;
            }
            let Ok(ft) = e.file_type() else { continue };
            path.push(&name_os);
            if self.excl.has_paths() && self.excl.path_excluded(path.as_os_str().as_bytes()) {
                truncate_path(path, keep);
                continue;
            }
            if ft.is_dir() {
                self.child_dir(id, name, path, keep, dev, &old_children, depth);
            } else if (ft.is_file() || ft.is_symlink()) && !self.excl.name_excluded(name) {
                self.child_file(id, name, &e);
            }
            truncate_path(path, keep);
        }
        Listing::Done
    }

    #[allow(clippy::too_many_arguments)]
    fn child_dir(
        &mut self,
        parent: u32,
        name: &[u8],
        path: &mut PathBuf,
        parent_len: usize,
        dev: u64,
        old_children: &HashMap<&[u8], u32>,
        depth: usize,
    ) {
        if self.excl.dir_decision(name, dev, dev) != DirDecision::Index {
            return;
        }
        if depth >= MAX_DEPTH {
            self.error(format!(
                "{} is nested too deeply, not entered",
                path.display()
            ));
            return;
        }
        if let Some(&oc) = old_children.get(name) {
            // keep the old subtree (or walk it, if a folder in it is dirty);
            // visit_old appends the name itself, so give it the parent's path
            truncate_path(path, parent_len);
            self.visit_old(oc, parent, path);
            path.push(OsStr::from_bytes(name));
            return;
        }
        let md = match fs::symlink_metadata(&*path) {
            Ok(m) if m.is_dir() => m,
            Ok(_) => return,
            Err(e) => {
                self.error(format!("{} cannot be read: {e}", path.display()));
                return;
            }
        };
        // mounts below a root are not crossed: the device must be the parent's
        if self.excl.dir_decision(name, md.dev(), dev) != DirDecision::Index {
            return;
        }
        let mark = self.b.mark();
        let Some(id) = self.b.push(
            parent,
            name,
            FLAG_DIR | recent_flag(md.mtime()),
            Category::Folder,
            md.mtime(),
            md.mtime_nsec() as u64,
        ) else {
            self.full = true;
            return;
        };
        match self.list_dir(id, path, md.dev(), None, false, depth + 1) {
            Listing::Done => {}
            Listing::Excluded => self.b.rollback(mark),
            Listing::Failed(e) => self.error(format!("{} cannot be read: {e}", path.display())),
        }
    }

    fn child_file(&mut self, parent: u32, name: &[u8], e: &fs::DirEntry) {
        let md = match e.metadata() {
            Ok(m) => m,
            Err(_) => return, // gone between the listing and now
        };
        let ft = md.file_type();
        if !(ft.is_file() || ft.is_symlink()) {
            return;
        }
        let exec = ft.is_file() && md.mode() & 0o111 != 0;
        let mut flags = 0;
        if name.first() == Some(&b'.') {
            flags |= FLAG_HIDDEN;
        }
        if exec {
            flags |= FLAG_EXEC;
        }
        if self
            .b
            .push(
                parent,
                name,
                flags,
                category_of(name, false, exec),
                md.mtime(),
                md.len(),
            )
            .is_none()
        {
            self.full = true;
        }
    }
}

/// [`FLAG_RECENT`] when `mtime` is within [`RECENT_SECS`] of now (or ahead of it).
fn recent_flag(mtime: i64) -> u8 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    if now - mtime <= RECENT_SECS {
        FLAG_RECENT
    } else {
        0
    }
}

/// Cut `path` back to its first `len` bytes (the buffer is reused).
fn truncate_path(path: &mut PathBuf, len: usize) {
    use std::os::unix::ffi::OsStringExt;
    let mut v = std::mem::take(path).into_os_string().into_vec();
    v.truncate(len);
    *path = PathBuf::from(std::ffi::OsString::from_vec(v));
}

fn read_cachedir_tag(path: &Path) -> bool {
    let Ok(mut f) = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    else {
        return false;
    };
    let mut head = [0u8; CACHEDIR_SIGNATURE.len()];
    f.read_exact(&mut head).is_ok() && is_cachedir_tag(&head)
}

fn read_hidden_file(path: &Path) -> HashSet<Vec<u8>> {
    let Ok(f) = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    else {
        return HashSet::new();
    };
    let mut buf = Vec::new();
    if f.take(HIDDEN_FILE_MAX).read_to_end(&mut buf).is_err() {
        return HashSet::new();
    }
    parse_hidden_file(&buf)
}

/// Folders whose modification time differs from the index's, or that are gone
/// (sorted ids). `only` limits the check to some folders.
pub fn changed_dirs(index: &Index, only: &dyn Fn(u32) -> bool, stop: &AtomicBool) -> Vec<u32> {
    let mut out = Vec::new();
    let mut path = PathBuf::new();
    for (i, r) in index.records().iter().enumerate() {
        if !r.is_dir() || !only(i as u32) {
            continue;
        }
        if i % 256 == 0 && stop.load(Ordering::Relaxed) {
            break;
        }
        path.clear();
        path.push(OsStr::from_bytes(&index.path_of(i as u32)));
        match fs::symlink_metadata(&path) {
            Ok(md)
                if md.is_dir()
                    && r.flags & FLAG_RECENT == 0
                    && md.mtime() == r.mtime
                    && md.mtime_nsec() as u64 == r.size => {}
            _ => out.push(i as u32),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdir::Scratch;
    use std::os::unix::fs::symlink;

    fn touch(p: &Path) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"x").unwrap();
    }

    fn names(ix: &Index) -> Vec<String> {
        let mut v: Vec<String> = (0..ix.len() as u32)
            .filter(|&i| ix.record(i).parent != NONE)
            .map(|i| String::from_utf8_lossy(&ix.path_of(i)).to_string())
            .collect();
        v.sort();
        v
    }

    fn scan(root: &Path, user: &[&str]) -> ScanResult {
        let user: Vec<_> = user.iter().map(std::ffi::OsString::from).collect();
        scan_full(
            &[root.to_path_buf()],
            &Excludes::new(&user),
            &AtomicBool::new(false),
            &HashMap::new(),
        )
    }

    #[test]
    fn scans_and_applies_exclusions() {
        let t = Scratch::new("scan");
        let r = fs::canonicalize(&t.0).unwrap();
        touch(&r.join("Documents/a.txt"));
        touch(&r.join("Documents/.dot.txt"));
        touch(&r.join(".git/config"));
        touch(&r.join(".cache/x"));
        touch(&r.join("proj/node_modules/m/index.js"));
        touch(&r.join("proj/__pycache__/x.pyc"));
        touch(&r.join("proj/src/main.rs"));
        touch(&r.join("cargo/target/CACHEDIR.TAG"));
        fs::write(
            r.join("cargo/target/CACHEDIR.TAG"),
            "Signature: 8a477f597d28d172789f06886806bc55\n",
        )
        .unwrap();
        touch(&r.join("cargo/target/debug/bin"));
        touch(&r.join("fake/CACHEDIR.TAG")); // no signature: kept
        touch(&r.join("venv/pyvenv.cfg"));
        touch(&r.join("venv/lib/x.py"));
        touch(&r.join("shown/.hidden"));
        fs::write(r.join("shown/.hidden"), "secret.txt\nsecretdir\n").unwrap();
        touch(&r.join("shown/secret.txt"));
        touch(&r.join("shown/secretdir/y"));
        touch(&r.join("shown/ok.txt"));
        touch(&r.join("skipme/z"));
        symlink(r.join("Documents"), r.join("link")).unwrap();
        symlink("/nonexistent", r.join("dangling")).unwrap();
        let res = scan(&r, &["skipme"]);
        assert!(!res.stopped && res.failed_roots == 0);
        let n: Vec<String> = names(&res.index)
            .iter()
            .map(|p| p.strip_prefix(r.to_str().unwrap()).unwrap().to_string())
            .collect();
        let want = [
            "/Documents",
            "/Documents/.dot.txt",
            "/Documents/a.txt",
            "/cargo",
            "/dangling",
            "/fake",
            "/fake/CACHEDIR.TAG",
            "/link",
            "/proj",
            "/proj/src",
            "/proj/src/main.rs",
            "/shown",
            "/shown/.hidden",
            "/shown/ok.txt",
        ];
        assert_eq!(n, want);
        // symlinks are entries, never followed; the dot file is flagged
        let dot = res
            .index
            .find_path(&[r.as_os_str().as_bytes(), b"/Documents/.dot.txt"].concat())
            .unwrap();
        assert!(res.index.record(dot).is_hidden());
        let link = res
            .index
            .find_path(&[r.as_os_str().as_bytes(), b"/link"].concat())
            .unwrap();
        assert!(!res.index.record(link).is_dir());
    }

    #[test]
    fn root_with_marker_is_still_indexed() {
        let t = Scratch::new("rootmarker");
        let r = fs::canonicalize(&t.0).unwrap();
        fs::write(
            r.join("CACHEDIR.TAG"),
            "Signature: 8a477f597d28d172789f06886806bc55\n",
        )
        .unwrap();
        touch(&r.join("a"));
        assert_eq!(scan(&r, &[]).index.len(), 3);
    }

    #[test]
    fn missing_root_is_reported() {
        let t = Scratch::new("noroot");
        let res = scan(&t.0.join("nope"), &[]);
        assert_eq!(res.failed_roots, 1);
        assert!(!res.errors.is_empty());
        assert_eq!(res.index.len(), 0);
    }

    #[test]
    fn stop_flag_ends_the_scan() {
        let t = Scratch::new("stop");
        touch(&t.0.join("a/b"));
        let stop = AtomicBool::new(true);
        let res = scan_full(
            std::slice::from_ref(&t.0),
            &Excludes::new(&[]),
            &stop,
            &HashMap::new(),
        );
        assert!(res.stopped);
    }

    #[test]
    fn rebuild_applies_changes_and_keeps_the_rest() {
        let t = Scratch::new("rebuild");
        let r = fs::canonicalize(&t.0).unwrap();
        touch(&r.join("a/one.txt"));
        touch(&r.join("a/deep/two.txt"));
        touch(&r.join("b/three.txt"));
        touch(&r.join("c/four.txt"));
        let first = scan(&r, &[]);
        let ix = &first.index;
        let rp = |s: &str| [r.as_os_str().as_bytes(), s.as_bytes()].concat();
        // changes: new file in a, delete deep/two.txt (deep dirty), new dir in b, c removed
        touch(&r.join("a/new.txt"));
        fs::remove_file(r.join("a/deep/two.txt")).unwrap();
        touch(&r.join("b/sub/five.txt"));
        fs::remove_dir_all(r.join("c")).unwrap();
        let mut dirty = vec![
            ix.find_path(&rp("/a")).unwrap(),
            ix.find_path(&rp("/a/deep")).unwrap(),
            ix.find_path(&rp("/b")).unwrap(),
            ix.find_path(&rp("/c")).unwrap(),
            ix.find_path(&rp("/c")).unwrap(), // dupes are harmless
        ];
        dirty.sort_unstable();
        dirty.dedup();
        let res = rebuild(
            ix,
            &dirty,
            &Excludes::new(&[]),
            &AtomicBool::new(false),
            &HashMap::new(),
        );
        let fresh = scan(&r, &[]);
        assert_eq!(names(&res.index), names(&fresh.index));
        // the map sends old ids to the same paths, and gone records to NONE
        for old in (0..ix.len() as u32).filter(|&i| ix.record(i).is_dir()) {
            let new = res.map[old as usize];
            if new == NONE {
                assert!(
                    res.index.find_path(&ix.path_of(old)).is_none(),
                    "{:?}",
                    String::from_utf8_lossy(&ix.path_of(old))
                );
            } else {
                assert_eq!(res.index.path_of(new), ix.path_of(old));
            }
        }
        // untouched subtrees were copied, with the same structure
        assert!(res.index.find_path(&rp("/a/new.txt")).is_some());
        assert!(res.index.find_path(&rp("/b/sub/five.txt")).is_some());
        assert!(res.index.find_path(&rp("/c")).is_none());
        assert!(res.index.find_path(&rp("/a/deep/two.txt")).is_none());
    }

    #[test]
    fn rebuild_drops_a_folder_that_became_excluded() {
        let t = Scratch::new("rebuild-excl");
        let r = fs::canonicalize(&t.0).unwrap();
        touch(&r.join("v/lib/x.py"));
        let first = scan(&r, &[]);
        let v = first
            .index
            .find_path(&[r.as_os_str().as_bytes(), b"/v"].concat())
            .unwrap();
        touch(&r.join("v/pyvenv.cfg"));
        let res = rebuild(
            &first.index,
            &[v],
            &Excludes::new(&[]),
            &AtomicBool::new(false),
            &HashMap::new(),
        );
        assert_eq!(res.index.len(), 1);
        assert!(res.map.iter().skip(1).all(|&m| m == NONE));
    }

    #[test]
    fn changed_dirs_sees_additions_and_removals() {
        let t = Scratch::new("changed");
        let r = fs::canonicalize(&t.0).unwrap();
        touch(&r.join("a/x"));
        touch(&r.join("b/y"));
        let stop = AtomicBool::new(false);
        // folders changed in the last seconds are looked at again: the clock
        // tick may hide a later change
        let fresh = scan(&r, &[]);
        assert!(!changed_dirs(&fresh.index, &|_| true, &stop).is_empty());
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        for d in [r.clone(), r.join("a"), r.join("b")] {
            fs::File::open(&d).unwrap().set_modified(old).unwrap();
        }
        let res = scan(&r, &[]);
        assert!(changed_dirs(&res.index, &|_| true, &stop).is_empty());
        std::thread::sleep(std::time::Duration::from_millis(20));
        touch(&r.join("a/new"));
        fs::remove_dir_all(r.join("b")).unwrap();
        let ch = changed_dirs(&res.index, &|_| true, &stop);
        let paths: Vec<_> = ch
            .iter()
            .map(|&i| String::from_utf8_lossy(&res.index.path_of(i)).to_string())
            .collect();
        assert!(paths.iter().any(|p| p.ends_with("/a")), "{paths:?}");
        assert!(paths.iter().any(|p| p.ends_with("/b")), "{paths:?}");
        // the filter limits the check
        assert!(changed_dirs(&res.index, &|_| false, &stop).is_empty());
    }
}
