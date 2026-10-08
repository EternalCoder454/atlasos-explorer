//! The live search: a breadth-first walk of a folder the index does not hold
//! (an external drive, a folder the index leaves out, or everything when the
//! index is turned off). Names are judged by the index's own matcher and the
//! same filters, so a live hit and an indexed hit mean the same thing. Hits
//! are handed out in batches while the walk goes on, and the walk checks a
//! stop flag between entries. Symlinks are not followed and other filesystems
//! are not entered, as the scanner does not either.
//!
//! [`covers`] says whether the index holds a folder, so a search of it can ask
//! the index instead.

use crate::category::{Category, category_of};
use crate::config::{BUILTIN_EXCLUDES, CACHEDIR_SIGNATURE, is_cachedir_tag, marker_excludes};
use crate::index::MAX_DEPTH;
use crate::query::{KindFilter, NameMatcher, Options};
use crate::tags;
use std::collections::VecDeque;
use std::fs;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Hits in one batch at most.
const BATCH: usize = 200;
/// A batch is handed out at least this often while hits are coming.
const FLUSH: Duration = Duration::from_millis(30);
/// The stop flag is looked at every this many entries.
const STOP_EVERY: u32 = 64;

/// A name and the filters, ready to judge entries.
pub struct LiveMatcher {
    names: NameMatcher,
    opts: Options,
    /// The wanted tag in its [`tags::folded`] form.
    tag: Option<String>,
}

impl LiveMatcher {
    pub fn new(query: &str, opts: Options) -> LiveMatcher {
        LiveMatcher {
            names: NameMatcher::new(query),
            tag: opts.tag.as_deref().map(tags::folded),
            opts,
        }
    }

    /// Nothing to look for: no word and no filter. Such a search has no
    /// results (the window does not start one).
    pub fn is_empty(&self) -> bool {
        let o = &self.opts;
        self.names.is_empty()
            && o.kind.is_none()
            && o.kinds.is_none()
            && o.modified_after.is_none()
            && o.modified_before.is_none()
            && o.size_min.is_none()
            && o.size_max.is_none()
            && o.tag.is_none()
    }

    /// Is the entry hidden and so left out?
    pub fn hides(&self, name: &[u8]) -> bool {
        !self.opts.include_hidden && name.first() == Some(&b'.')
    }

    /// The first look, from the name alone: the match class, or `None`.
    pub fn name_class(&self, name: &[u8]) -> Option<u8> {
        self.names.class(name)
    }

    /// The second look, once the entry has been stat'ed: do the filters accept it?
    pub fn accepts(&self, name: &[u8], is_dir: bool, exec: bool, size: u64, mtime: i64) -> bool {
        let o = &self.opts;
        match o.kind {
            Some(KindFilter::Folder) if !is_dir => return false,
            Some(KindFilter::File) if is_dir => return false,
            _ => {}
        }
        if let Some(mask) = o.kinds {
            let cat: Category = category_of(name, is_dir, exec);
            if mask & cat.bit() == 0 {
                return false;
            }
        }
        if o.modified_after.is_some_and(|t| mtime < t)
            || o.modified_before.is_some_and(|t| mtime > t)
        {
            return false;
        }
        let size = if is_dir { 0 } else { size };
        !(o.size_min.is_some_and(|s| size < s) || o.size_max.is_some_and(|s| size > s))
    }

    /// Does the search ask for a tag? Then every candidate needs
    /// [`LiveMatcher::tag_accepts`] too, which `accepts` and `test` (they have
    /// only a name and a stat) do not do.
    pub fn wants_tag(&self) -> bool {
        self.tag.is_some()
    }

    /// The third look: does the entry at `path` carry the tag asked for (its
    /// `user.xdg.tags`, read now, symlinks not followed)? Always true when no
    /// tag is asked for.
    pub fn tag_accepts(&self, path: &Path) -> bool {
        match &self.tag {
            None => true,
            Some(want) => tags::contains(&tags::clean(&tags::read_raw(path)), want),
        }
    }

    /// Judge an entry fully (the name, then the filters) given its stat.
    pub fn test(&self, name: &[u8], is_dir: bool, exec: bool, size: u64, mtime: i64) -> Option<u8> {
        let class = self.name_class(name)?;
        self.accepts(name, is_dir, exec, size, mtime)
            .then_some(class)
    }
}

/// One result of a walk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WalkHit {
    /// Absolute path bytes.
    pub path: Vec<u8>,
    pub is_dir: bool,
    /// 0 for folders.
    pub size: u64,
    pub mtime: i64,
    /// The match class, 1 (substring) to 5 (whole name).
    pub class: u8,
}

/// Why a walk ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WalkEnd {
    /// Every folder was looked through.
    Done,
    /// The stop flag was set.
    Stopped,
    /// `max_hits` was reached.
    Capped,
    /// The folder to search could not be read.
    Unreadable,
}

/// Walk `root`, handing out batches of hits to `flush` as they are found
/// (the first hit at once, then at least every 30 ms while more come), at
/// most `max_hits` in all.
pub fn walk(
    root: &Path,
    m: &LiveMatcher,
    stop: &AtomicBool,
    max_hits: usize,
    flush: &mut dyn FnMut(Vec<WalkHit>),
) -> WalkEnd {
    let Ok(root_meta) = fs::metadata(root) else {
        return WalkEnd::Unreadable;
    };
    if !root_meta.is_dir() {
        return WalkEnd::Unreadable;
    }
    let root_dev = root_meta.dev();
    let mut queue: VecDeque<(PathBuf, usize)> = VecDeque::new();
    queue.push_back((root.to_path_buf(), 0));
    let mut batch: Vec<WalkHit> = Vec::new();
    let mut total = 0usize;
    let mut last = Instant::now();
    let mut first_sent = false;
    let mut tick = 0u32;
    let mut first_dir = true;

    while let Some((dir, depth)) = queue.pop_front() {
        let rd = match fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(_) if first_dir => return WalkEnd::Unreadable,
            Err(_) => continue, // a folder we may not read is skipped
        };
        first_dir = false;
        for entry in rd {
            tick += 1;
            if tick.is_multiple_of(STOP_EVERY) && stop.load(Ordering::Relaxed) {
                send(&mut batch, flush);
                return WalkEnd::Stopped;
            }
            let Ok(entry) = entry else { continue };
            let name_os = entry.file_name();
            let name = name_os.as_bytes();
            if m.hides(name) {
                continue;
            }
            let Ok(ft) = entry.file_type() else { continue };
            let is_dir = ft.is_dir();
            // Sockets, pipes and devices are not files a person looks for.
            if ft.is_socket() || ft.is_fifo() || ft.is_block_device() || ft.is_char_device() {
                continue;
            }
            let mut meta: Option<fs::Metadata> = None;
            if is_dir && depth < MAX_DEPTH {
                // lstat, so a symlink to a folder is not followed; a folder on
                // another filesystem is not entered.
                if let Ok(md) = entry.metadata() {
                    if md.dev() == root_dev {
                        queue.push_back((entry.path(), depth + 1));
                    }
                    meta = Some(md);
                }
            }
            let Some(class) = m.name_class(name) else {
                continue;
            };
            if meta.is_none() {
                meta = entry.metadata().ok();
            }
            let (size, mtime, exec) = meta.as_ref().map_or((0, 0, false), |md| {
                (md.len(), md.mtime(), md.mode() & 0o111 != 0)
            });
            if !m.accepts(name, is_dir, exec, size, mtime) {
                continue;
            }
            let full = entry.path();
            if m.wants_tag() && !m.tag_accepts(&full) {
                continue;
            }
            batch.push(WalkHit {
                path: full.as_os_str().as_bytes().to_vec(),
                is_dir,
                size: if is_dir { 0 } else { size },
                mtime,
                class,
            });
            total += 1;
            if total >= max_hits {
                send(&mut batch, flush);
                return WalkEnd::Capped;
            }
            if batch.len() >= BATCH || !first_sent || last.elapsed() >= FLUSH {
                first_sent = true;
                last = Instant::now();
                send(&mut batch, flush);
            }
        }
        if !batch.is_empty() && last.elapsed() >= FLUSH {
            last = Instant::now();
            send(&mut batch, flush);
        }
    }
    send(&mut batch, flush);
    WalkEnd::Done
}

fn send(batch: &mut Vec<WalkHit>, flush: &mut dyn FnMut(Vec<WalkHit>)) {
    if !batch.is_empty() {
        flush(std::mem::take(batch));
    }
}

/// Does the index hold `folder`? Yes when it is below one of its `roots`, on
/// the same filesystem as that root, and no folder on the way is one the
/// scanner leaves out: a dot-folder, `node_modules`, `__pycache__`, or one
/// with a `CACHEDIR.TAG` or a `pyvenv.cfg`. (Names a person added to
/// `Exclude=` are not known here; a folder they excluded is searched through
/// the index and finds nothing.) Reads a few folders: not for the GUI thread.
pub fn covers(folder: &Path, roots: &[PathBuf]) -> bool {
    let Some(root) = roots
        .iter()
        .filter(|r| folder.starts_with(r))
        .max_by_key(|r| r.as_os_str().len())
    else {
        return false;
    };
    let Ok(root_dev) = fs::metadata(root).map(|m| m.dev()) else {
        return false;
    };
    let Ok(rel) = folder.strip_prefix(root) else {
        return false;
    };
    let mut at = root.clone();
    for part in rel.components() {
        at.push(part);
        let name = part.as_os_str().as_bytes();
        if name.first() == Some(&b'.') || BUILTIN_EXCLUDES.iter().any(|e| e.as_bytes() == name) {
            return false;
        }
        match fs::symlink_metadata(&at) {
            Ok(md) if md.is_dir() && md.dev() == root_dev => {}
            _ => return false,
        }
        let tag = read_tag(&at.join("CACHEDIR.TAG"));
        if marker_excludes(tag, at.join("pyvenv.cfg").exists()) {
            return false;
        }
    }
    // the folder itself, when it is a root, is searched whole
    true
}

fn read_tag(path: &Path) -> Option<bool> {
    let mut f = fs::File::open(path).ok()?;
    let mut head = vec![0u8; CACHEDIR_SIGNATURE.len()];
    let n = f.read(&mut head).ok()?;
    head.truncate(n);
    Some(is_cachedir_tag(&head))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdir::Scratch;
    use std::os::unix::fs::symlink;

    fn touch(p: &Path, bytes: usize) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, vec![b'x'; bytes]).unwrap();
    }

    fn run(root: &Path, query: &str, opts: Options) -> (Vec<WalkHit>, WalkEnd, usize) {
        run_with(root, query, opts, 1000, &AtomicBool::new(false))
    }

    fn run_with(
        root: &Path,
        query: &str,
        opts: Options,
        max: usize,
        stop: &AtomicBool,
    ) -> (Vec<WalkHit>, WalkEnd, usize) {
        let m = LiveMatcher::new(query, opts);
        let mut out = Vec::new();
        let mut batches = 0;
        let end = walk(root, &m, stop, max, &mut |b| {
            batches += 1;
            out.extend(b);
        });
        (out, end, batches)
    }

    fn rel(root: &Path, hits: &[WalkHit]) -> Vec<String> {
        let mut v: Vec<String> = hits
            .iter()
            .map(|h| {
                String::from_utf8_lossy(&h.path)
                    .strip_prefix(&format!("{}/", root.display()))
                    .unwrap()
                    .to_string()
            })
            .collect();
        v.sort();
        v
    }

    fn tree() -> Scratch {
        let s = Scratch::new("walk");
        let r = &s.0;
        touch(&r.join("Report Final.txt"), 10);
        touch(&r.join("a/report-2024.pdf"), 2_000_000);
        touch(&r.join("a/b/deep/Report.md"), 5);
        touch(&r.join("a/notes.txt"), 1);
        touch(&r.join(".hiddendir/report-secret.txt"), 1);
        touch(&r.join(".report-dot"), 1);
        fs::create_dir_all(r.join("reports-folder")).unwrap();
        s
    }

    #[test]
    fn finds_names_like_the_index_does() {
        let s = tree();
        let (hits, end, _) = run(&s.0, "report", Options::default());
        assert_eq!(end, WalkEnd::Done);
        assert_eq!(
            rel(&s.0, &hits),
            [
                "Report Final.txt",
                "a/b/deep/Report.md",
                "a/report-2024.pdf",
                "reports-folder"
            ]
        );
        // Case, diacritics and several words, as the index matches.
        let r = &s.0;
        touch(&r.join("Résumé Final.docx"), 3);
        let (hits, _, _) = run(r, "resume final", Options::default());
        assert_eq!(rel(r, &hits), ["Résumé Final.docx"]);
        let (hits, _, _) = run(r, "RPT", Options::default());
        assert!(rel(r, &hits).is_empty());
    }

    #[test]
    fn hidden_entries_only_when_asked() {
        let s = tree();
        let (hits, _, _) = run(&s.0, "report", Options::default());
        assert!(
            !rel(&s.0, &hits)
                .iter()
                .any(|p| p.contains(".hidden") || p.contains("dot"))
        );
        let o = Options {
            include_hidden: true,
            ..Options::default()
        };
        let (hits, _, _) = run(&s.0, "report", o);
        let names = rel(&s.0, &hits);
        assert!(
            names.contains(&".hiddendir/report-secret.txt".to_string()),
            "{names:?}"
        );
        assert!(names.contains(&".report-dot".to_string()));
    }

    #[test]
    fn filters_combine() {
        let s = tree();
        let r = &s.0;
        let o = Options {
            kinds: Some(Category::Pdf.bit()),
            ..Options::default()
        };
        let (hits, _, _) = run(r, "report", o);
        assert_eq!(rel(r, &hits), ["a/report-2024.pdf"]);
        let o = Options {
            kind: Some(KindFilter::Folder),
            ..Options::default()
        };
        let (hits, _, _) = run(r, "report", o);
        assert_eq!(rel(r, &hits), ["reports-folder"]);
        assert!(hits[0].is_dir && hits[0].size == 0);
        // Size: more than 1 MB.
        let o = Options {
            size_min: Some(1_000_000),
            kind: Some(KindFilter::File),
            ..Options::default()
        };
        let (hits, _, _) = run(r, "", o);
        assert_eq!(rel(r, &hits), ["a/report-2024.pdf"]);
        assert_eq!(hits[0].size, 2_000_000);
        // Modified: nothing is older than the future, everything is newer than the past.
        let o = Options {
            modified_after: Some(i64::MAX),
            ..Options::default()
        };
        assert!(run(r, "report", o).0.is_empty());
        let o = Options {
            modified_after: Some(0),
            ..Options::default()
        };
        assert_eq!(run(r, "report", o).0.len(), 4);
    }

    #[test]
    fn filters_alone_find_everything_that_passes() {
        let s = tree();
        let o = Options {
            kinds: Some(Category::Text.bit()),
            ..Options::default()
        };
        let (hits, _, _) = run(&s.0, "", o);
        assert_eq!(
            rel(&s.0, &hits),
            ["Report Final.txt", "a/b/deep/Report.md", "a/notes.txt"]
        );
    }

    #[test]
    fn hits_carry_what_the_rows_show() {
        let s = tree();
        let (hits, _, _) = run(&s.0, "notes", Options::default());
        assert_eq!(hits.len(), 1);
        let h = &hits[0];
        assert_eq!((h.is_dir, h.size), (false, 1));
        assert!(h.mtime > 1_500_000_000);
        // A whole-name match outranks a substring one.
        assert!(
            run(&s.0, "notes.txt", Options::default()).0[0].class
                > run(&s.0, "otes", Options::default()).0[0].class
        );
    }

    #[test]
    fn symlinks_are_not_followed_and_are_found_by_name() {
        let s = tree();
        let r = &s.0;
        symlink(r.join("a"), r.join("link-to-a")).unwrap();
        symlink("/nonexistent", r.join("dangling-report")).unwrap();
        let (hits, end, _) = run(r, "report", Options::default());
        assert_eq!(end, WalkEnd::Done);
        let names = rel(r, &hits);
        // Found once through the real folder only.
        assert_eq!(
            names
                .iter()
                .filter(|n| n.ends_with("report-2024.pdf"))
                .count(),
            1
        );
        assert!(names.contains(&"dangling-report".to_string()));
    }

    #[test]
    fn stops_on_request_and_at_the_cap() {
        let s = Scratch::new("walkcap");
        for i in 0..300 {
            touch(&s.0.join(format!("d{}/file-{i}.txt", i % 10)), 1);
        }
        let (hits, end, batches) = run_with(
            &s.0,
            "file",
            Options::default(),
            50,
            &AtomicBool::new(false),
        );
        assert_eq!((hits.len(), end), (50, WalkEnd::Capped));
        assert!(batches >= 1);
        // The stop flag ends it with what was found so far (and never panics).
        let stop = AtomicBool::new(true);
        let (hits, end, _) = run_with(&s.0, "file", Options::default(), 1000, &stop);
        assert_eq!(end, WalkEnd::Stopped);
        assert!(hits.len() < 300);
    }

    #[test]
    fn the_first_hit_comes_at_once() {
        let s = tree();
        let m = LiveMatcher::new("report", Options::default());
        let mut sizes = Vec::new();
        walk(&s.0, &m, &AtomicBool::new(false), 100, &mut |b| {
            sizes.push(b.len())
        });
        assert_eq!(sizes[0], 1, "{sizes:?}");
        assert_eq!(sizes.iter().sum::<usize>(), 4);
    }

    #[test]
    fn an_unreadable_or_missing_folder_is_reported() {
        let s = tree();
        let (hits, end, _) = run(&s.0.join("nope"), "x", Options::default());
        assert!(hits.is_empty());
        assert_eq!(end, WalkEnd::Unreadable);
        let (_, end, _) = run(&s.0.join("Report Final.txt"), "x", Options::default());
        assert_eq!(end, WalkEnd::Unreadable);
    }

    #[test]
    fn empty_search_is_recognised() {
        assert!(LiveMatcher::new("  ", Options::default()).is_empty());
        assert!(!LiveMatcher::new("a", Options::default()).is_empty());
        let o = Options {
            size_max: Some(5),
            ..Options::default()
        };
        assert!(!LiveMatcher::new("", o).is_empty());
    }

    fn tagged_tree() -> Option<Scratch> {
        let s = Scratch::new("walk-tags");
        if crate::testdir::skip_without_xattrs(&s.0) {
            return None;
        }
        let r = &s.0;
        touch(&r.join("plain.txt"), 1);
        touch(&r.join("red.txt"), 1);
        touch(&r.join("sub/deep-red.png"), 1);
        touch(&r.join("sub/blue.txt"), 1);
        touch(&r.join("sub/.hidden-red"), 1);
        fs::create_dir_all(r.join("red-folder")).unwrap();
        crate::testdir::set_tags(&r.join("red.txt"), "Work, Red").unwrap();
        crate::testdir::set_tags(&r.join("sub/deep-red.png"), "RED").unwrap();
        crate::testdir::set_tags(&r.join("sub/blue.txt"), "Blue").unwrap();
        crate::testdir::set_tags(&r.join("sub/.hidden-red"), "Red").unwrap();
        crate::testdir::set_tags(&r.join("red-folder"), "red").unwrap();
        Some(s)
    }

    fn tag(t: &str) -> Options {
        Options {
            tag: Some(t.to_string()),
            ..Options::default()
        }
    }

    #[test]
    fn the_tag_is_read_from_each_candidate() {
        let Some(s) = tagged_tree() else { return };
        let r = &s.0;
        let (hits, end, _) = run(r, "", tag("red"));
        assert_eq!(end, WalkEnd::Done);
        assert_eq!(rel(r, &hits), ["red-folder", "red.txt", "sub/deep-red.png"]);
        let (hits, _, _) = run(r, "", tag("  BLUE "));
        assert_eq!(rel(r, &hits), ["sub/blue.txt"]);
        assert!(run(r, "", tag("green")).0.is_empty());
        assert!(run(r, "", tag("re")).0.is_empty());
        // with a name, a kind, hidden entries
        let (hits, _, _) = run(r, "deep", tag("red"));
        assert_eq!(rel(r, &hits), ["sub/deep-red.png"]);
        let o = Options {
            tag: Some("red".into()),
            kinds: Some(Category::Image.bit()),
            ..Options::default()
        };
        assert_eq!(rel(r, &run(r, "", o).0), ["sub/deep-red.png"]);
        let o = Options {
            tag: Some("red".into()),
            include_hidden: true,
            ..Options::default()
        };
        assert_eq!(rel(r, &run(r, "", o).0).len(), 4);
    }

    #[test]
    fn a_symlink_has_no_tags_of_its_own() {
        let Some(s) = tagged_tree() else { return };
        let r = &s.0;
        symlink(r.join("red.txt"), r.join("link-to-red")).unwrap();
        let (hits, _, _) = run(r, "", tag("red"));
        assert!(!rel(r, &hits).contains(&"link-to-red".to_string()));
    }

    #[test]
    fn a_tag_makes_a_search_non_empty() {
        let o = Options {
            tag: Some("red".into()),
            ..Options::default()
        };
        let m = LiveMatcher::new("", o);
        assert!(!m.is_empty());
        assert!(m.wants_tag());
        assert!(!LiveMatcher::new("", Options::default()).wants_tag());
        // with no tag asked for, any path is accepted
        assert!(LiveMatcher::new("a", Options::default()).tag_accepts(Path::new("/nonexistent")));
        assert!(!m.tag_accepts(Path::new("/nonexistent")));
    }

    #[test]
    fn covers_follows_the_scanners_rules() {
        let s = Scratch::new("covers");
        let r = &s.0;
        fs::create_dir_all(r.join("docs/sub")).unwrap();
        fs::create_dir_all(r.join(".config/x")).unwrap();
        fs::create_dir_all(r.join("proj/node_modules/m")).unwrap();
        fs::create_dir_all(r.join("cargo/target/debug")).unwrap();
        fs::write(
            r.join("cargo/target/CACHEDIR.TAG"),
            format!(
                "{}\n# made by cargo",
                String::from_utf8_lossy(CACHEDIR_SIGNATURE)
            ),
        )
        .unwrap();
        fs::create_dir_all(r.join("fake/tag")).unwrap();
        fs::write(r.join("fake/CACHEDIR.TAG"), "nothing").unwrap();
        fs::create_dir_all(r.join("venv/lib")).unwrap();
        fs::write(r.join("venv/pyvenv.cfg"), "home = /usr").unwrap();
        let roots = vec![r.clone()];
        assert!(covers(r, &roots), "the root itself");
        assert!(covers(&r.join("docs"), &roots));
        assert!(covers(&r.join("docs/sub"), &roots));
        assert!(
            covers(&r.join("fake/tag"), &roots),
            "a tag without the signature"
        );
        assert!(!covers(&r.join(".config/x"), &roots));
        assert!(!covers(&r.join("proj/node_modules/m"), &roots));
        assert!(!covers(&r.join("cargo/target"), &roots));
        assert!(!covers(&r.join("cargo/target/debug"), &roots));
        assert!(!covers(&r.join("venv/lib"), &roots));
        // Outside every root, or a folder that is not there.
        assert!(!covers(Path::new("/usr"), &roots));
        assert!(!covers(&r.join("missing"), &roots));
        assert!(!covers(&r.join("docs"), &[]));
        // A symlink to a folder is not the folder.
        symlink(r.join("docs"), r.join("alias")).unwrap();
        assert!(!covers(&r.join("alias"), &roots));
        // The deepest root wins; a sibling with the same start is not inside.
        assert!(!covers(
            Path::new(&format!("{}-other", r.display())),
            &roots
        ));
    }
}
