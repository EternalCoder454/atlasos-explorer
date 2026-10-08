//! Search: matching, filters, ranking and top-K selection over an [`Index`].
//!
//! A query is split into words on whitespace; each is folded like names are
//! (see `text`), and every word must match. An entry's class is the worst of
//! its words' classes, best first: exact basename, basename prefix, word prefix
//! (words split at space, `-`, `_`, `.`, camelCase and letter-digit steps),
//! acronym (the words' initials start with the query word) and substring.
//!
//! Ranking, as one integer key: class, then the recency bucket of the newer of
//! the modification time and the last use, then a boost for shallow paths and
//! for Desktop, Documents and Downloads, then the exact recency; ties by name.
//! Only the best `limit` hits are kept (a bounded heap), never a full sort.

use crate::category::{Category, icon_of, mime_of};
use crate::index::{Index, NONE};
use crate::tags;
use crate::text::{fold, is_word_start_at_ascii, word_starts_folded};
use atlas_explorer_core::display_name;
use memchr::memmem;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// Most hits one call returns.
pub const MAX_LIMIT: usize = 500;
/// Bytes of the query that are used.
pub const MAX_QUERY_BYTES: usize = 1024;
/// Words of the query that are used.
pub const MAX_WORDS: usize = 8;
/// Entries one search thread takes at least.
const CHUNK: usize = 32 * 1024;

const SUBSTRING: u8 = 1;
const ACRONYM: u8 = 2;
const WORD_PREFIX: u8 = 3;
const PREFIX: u8 = 4;
const EXACT: u8 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KindFilter {
    Folder,
    File,
}

/// The `options` of `Search`, already validated.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub kind: Option<KindFilter>,
    /// Bit mask of [`Category::bit`].
    pub kinds: Option<u32>,
    /// Absolute path bytes.
    pub root: Option<Vec<u8>>,
    pub include_hidden: bool,
    pub modified_after: Option<i64>,
    pub modified_before: Option<i64>,
    pub size_min: Option<u64>,
    pub size_max: Option<u64>,
    /// Match the whole path, not the name.
    pub path_match: bool,
    /// Only entries that carry this tag (`user.xdg.tags`), ignoring case and
    /// accents.
    pub tag: Option<String>,
}

impl Options {
    fn has_filter(&self) -> bool {
        self.kind.is_some()
            || self.kinds.is_some()
            || self.root.is_some()
            || self.modified_after.is_some()
            || self.modified_before.is_some()
            || self.size_min.is_some()
            || self.size_max.is_some()
            || self.tag.is_some()
    }
}

#[derive(Clone, Debug)]
pub struct Hit {
    /// Absolute path bytes.
    pub path: Vec<u8>,
    /// Display name (controls and bidi made visible).
    pub name: String,
    pub is_dir: bool,
    pub category: Category,
    pub mtime: i64,
    /// 0 for folders.
    pub size: u64,
    pub score: f64,
}

impl Hit {
    pub fn mime(&self) -> &'static str {
        mime_of(raw_name(&self.path), self.is_dir)
    }

    pub fn icon(&self) -> String {
        icon_of(self.mime(), self.is_dir)
    }
}

fn raw_name(path: &[u8]) -> &[u8] {
    match memchr::memrchr(b'/', path) {
        Some(p) if p + 1 < path.len() => &path[p + 1..],
        _ => path,
    }
}

struct Word {
    bytes: Vec<u8>,
    finder: memmem::Finder<'static>,
}

struct Matcher {
    words: Vec<Word>,
    path_mode: bool,
}

impl Matcher {
    fn new(query: &str, path_mode: bool) -> Matcher {
        let mut q = query;
        if q.len() > MAX_QUERY_BYTES {
            let mut n = MAX_QUERY_BYTES;
            while !q.is_char_boundary(n) {
                n -= 1;
            }
            q = &q[..n];
        }
        let words = q
            .split_whitespace()
            .take(MAX_WORDS)
            .map(|w| fold(w).into_bytes())
            .filter(|w| !w.is_empty())
            .map(|bytes| Word {
                finder: memmem::Finder::new(&bytes).into_owned(),
                bytes,
            })
            .collect();
        Matcher { words, path_mode }
    }

    /// The class of an entry (the worst over the words), or `None`.
    #[inline]
    fn classify(&self, raw: &[u8], folded: &[u8]) -> Option<u8> {
        let mut class = EXACT;
        for w in &self.words {
            class = class.min(self.word_class(w, raw, folded)?);
        }
        Some(class)
    }

    #[inline]
    fn word_class(&self, w: &Word, raw: &[u8], folded: &[u8]) -> Option<u8> {
        if w.bytes.len() > folded.len() {
            return None;
        }
        if folded == w.bytes {
            return Some(EXACT);
        }
        if folded.starts_with(&w.bytes) {
            return Some(PREFIX);
        }
        let found = w.finder.find(folded).is_some();
        if found {
            if raw.is_ascii() {
                for p in w.finder.find_iter(folded) {
                    if is_word_start_at_ascii(raw, p, self.path_mode) {
                        return Some(WORD_PREFIX);
                    }
                }
            } else {
                let starts = word_starts_folded(raw, self.path_mode);
                for p in w.finder.find_iter(folded) {
                    if starts.binary_search_by_key(&p, |s| s.0).is_ok() {
                        return Some(WORD_PREFIX);
                    }
                }
            }
        }
        if w.bytes.len() >= 2 && folded.first() == w.bytes.first() && self.acronym(w, raw) {
            return Some(ACRONYM);
        }
        found.then_some(SUBSTRING)
    }

    fn acronym(&self, w: &Word, raw: &[u8]) -> bool {
        if raw.is_ascii() {
            let mut k = 0;
            for i in 0..raw.len() {
                if is_word_start_at_ascii(raw, i, self.path_mode) {
                    if raw[i].to_ascii_lowercase() != w.bytes[k] {
                        return false;
                    }
                    k += 1;
                    if k == w.bytes.len() {
                        return true;
                    }
                }
            }
            false
        } else {
            let initials: String = word_starts_folded(raw, self.path_mode)
                .iter()
                .map(|s| s.1)
                .collect();
            initials.as_bytes().starts_with(&w.bytes)
        }
    }
}

/// The index's matcher for one name at a time, for a walk of folders that
/// are not indexed (see `walk`): the same words, folding and classes, so a
/// live result is judged like an indexed one.
pub struct NameMatcher(Matcher);

impl NameMatcher {
    pub fn new(query: &str) -> NameMatcher {
        NameMatcher(Matcher::new(query, false))
    }

    /// No word to look for: everything matches (only filters narrow it).
    pub fn is_empty(&self) -> bool {
        self.0.words.is_empty()
    }

    /// How well a name matches, 1 (substring) to 5 (the whole name), or `None`.
    pub fn class(&self, name: &[u8]) -> Option<u8> {
        if self.0.words.is_empty() {
            return Some(SUBSTRING);
        }
        let folded = crate::text::fold_bytes(name);
        self.0.classify(name, &folded)
    }
}

/// Ranking key; higher is better. See the module docs.
#[inline]
fn rank_key(class: u8, recency: i64, depth: u8, boost: u8, now: i64) -> u64 {
    let age = now.saturating_sub(recency);
    let bucket: u64 = match age {
        i64::MIN..=3_600 => 6,
        3_601..=86_400 => 5,
        86_401..=604_800 => 4,
        604_801..=2_592_000 => 3,
        2_592_001..=15_552_000 => 2,
        15_552_001..=63_072_000 => 1,
        _ => 0,
    };
    let shallow = 32 - u64::from(depth.min(32));
    let b = (shallow.min(31)) + 32 * u64::from(boost.min(1));
    let rec = recency.clamp(0, i64::from(u32::MAX)) as u64;
    (u64::from(class) << 56) | (bucket << 48) | (b << 40) | (rec << 8)
}

/// A candidate; the greatest (by `Ord`) is the worst, so a max-heap holds the
/// best K and its top is the one to replace.
struct Cand<'a> {
    key: u64,
    id: u32,
    name: &'a [u8],
}

impl PartialEq for Cand<'_> {
    fn eq(&self, o: &Self) -> bool {
        self.cmp(o) == Ordering::Equal
    }
}
impl Eq for Cand<'_> {}
impl PartialOrd for Cand<'_> {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Cand<'_> {
    fn cmp(&self, o: &Self) -> Ordering {
        o.key
            .cmp(&self.key)
            .then_with(|| self.name.cmp(o.name))
            .then_with(|| self.id.cmp(&o.id))
    }
}

struct TopK<'a> {
    k: usize,
    heap: BinaryHeap<Cand<'a>>,
}

impl<'a> TopK<'a> {
    fn new(k: usize) -> Self {
        TopK {
            k,
            heap: BinaryHeap::with_capacity(k + 1),
        }
    }

    #[inline]
    fn worst_key(&self) -> Option<u64> {
        if self.heap.len() >= self.k {
            self.heap.peek().map(|c| c.key)
        } else {
            None
        }
    }

    fn push(&mut self, c: Cand<'a>) {
        if self.heap.len() < self.k {
            self.heap.push(c);
        } else if let Some(worst) = self.heap.peek()
            && c < *worst
        {
            self.heap.pop();
            self.heap.push(c);
        }
    }
}

/// Id ranges to look at: the whole index, or the part under `root`.
fn ranges(index: &Index, root: Option<&[u8]>) -> Vec<(u32, u32)> {
    let Some(root) = root else {
        return vec![(0, index.len() as u32)];
    };
    let mut out = Vec::new();
    if let Some(id) = index.find_path(root) {
        out.push((id + 1, index.end(id)));
    }
    // a root above the index's roots holds them all
    for r in index.roots() {
        let rn = index.name(r);
        if rn.len() > root.len() && rn.starts_with(root) && (root == b"/" || rn[root.len()] == b'/')
        {
            out.push((r, index.end(r)));
        }
    }
    out
}

/// The part of `ranges` made of the ids in `ids` (ascending), one range each.
fn only_ids(ranges: &[(u32, u32)], ids: &[u32]) -> Vec<(u32, u32)> {
    ids.iter()
        .filter(|&&id| ranges.iter().any(|&(a, b)| a <= id && id < b))
        .map(|&id| (id, id + 1))
        .collect()
}

/// Run a search. `now` is the current time in seconds since the epoch.
pub fn search(index: &Index, query: &str, limit: usize, opts: &Options, now: i64) -> Vec<Hit> {
    let limit = limit.min(MAX_LIMIT);
    if limit == 0 || index.is_empty() {
        return Vec::new();
    }
    let matcher = Matcher::new(query, opts.path_match);
    if matcher.words.is_empty() && !opts.has_filter() {
        return Vec::new();
    }
    let mut ranges = ranges(index, opts.root.as_deref());
    if let Some(tag) = &opts.tag {
        // only the entries that carry the tag are looked at
        ranges = only_ids(&ranges, &index.ids_with_tag(&tags::folded(tag)));
    }
    let total: usize = ranges.iter().map(|&(a, b)| (b - a) as usize).sum();
    if total == 0 {
        return Vec::new();
    }
    let threads = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .clamp(1, 4)
        .min(total / CHUNK + 1);
    let parts = split(&ranges, total, threads);

    let mut top = TopK::new(limit);
    if parts.len() <= 1 {
        scan(index, &matcher, opts, limit, now, &parts[0], &mut top);
    } else {
        let work = |part: &Vec<(u32, u32)>| {
            let mut t = TopK::new(limit);
            scan(index, &matcher, opts, limit, now, part, &mut t);
            t.heap.into_vec()
        };
        let work = &work;
        let results: Vec<Vec<Cand>> = std::thread::scope(|s| {
            let mut handles = Vec::new();
            let mut inline: Vec<Vec<Cand>> = Vec::new();
            for part in &parts {
                // a thread that cannot be started is not an error: do its part here
                match std::thread::Builder::new().spawn_scoped(s, move || work(part)) {
                    Ok(h) => handles.push(h),
                    Err(e) => {
                        log::warn!("search: no extra thread ({e}), working inline");
                        inline.push(work(part));
                    }
                }
            }
            for h in handles {
                inline.push(h.join().unwrap_or_default());
            }
            inline
        });
        for c in results.into_iter().flatten() {
            top.push(c);
        }
    }
    top.heap
        .into_sorted_vec()
        .into_iter()
        .map(|c| make_hit(index, &c))
        .collect()
}

/// Cut the ranges into `n` parts of about equal size.
fn split(ranges: &[(u32, u32)], total: usize, n: usize) -> Vec<Vec<(u32, u32)>> {
    let n = n.max(1);
    let per = total.div_ceil(n);
    let mut parts: Vec<Vec<(u32, u32)>> = vec![Vec::new()];
    let mut room = per;
    for &(mut a, b) in ranges {
        while a < b {
            if room == 0 {
                parts.push(Vec::new());
                room = per;
            }
            let take = ((b - a) as usize).min(room) as u32;
            if let Some(last) = parts.last_mut() {
                last.push((a, a + take));
            }
            a += take;
            room -= take as usize;
        }
    }
    parts
}

fn make_hit(index: &Index, c: &Cand) -> Hit {
    let r = index.record(c.id);
    Hit {
        path: index.path_of(c.id),
        name: display_name(index.name(c.id)),
        is_dir: r.is_dir(),
        category: Category::from_u8(r.cat).unwrap_or(Category::Other),
        mtime: r.mtime,
        size: if r.is_dir() { 0 } else { r.size },
        score: c.key as f64,
    }
}

fn scan<'a>(
    index: &'a Index,
    m: &Matcher,
    o: &Options,
    _limit: usize,
    now: i64,
    part: &[(u32, u32)],
    top: &mut TopK<'a>,
) {
    let kinds = o.kinds;
    let mut pathbuf: Vec<u8> = Vec::new();
    for &(a, b) in part {
        for id in a..b {
            let r = index.record(id);
            if r.is_hidden() && !o.include_hidden {
                continue;
            }
            match o.kind {
                Some(KindFilter::Folder) if !r.is_dir() => continue,
                Some(KindFilter::File) if r.is_dir() => continue,
                _ => {}
            }
            if let Some(mask) = kinds
                && mask & (1u32 << (r.cat & 31)) == 0
            {
                continue;
            }
            if o.modified_after.is_some_and(|t| r.mtime < t)
                || o.modified_before.is_some_and(|t| r.mtime > t)
            {
                continue;
            }
            if r.parent == NONE {
                continue; // a root is a place, not a hit
            }
            if o.size_min.is_some() || o.size_max.is_some() {
                let size = if r.is_dir() { 0 } else { r.size };
                if o.size_min.is_some_and(|s| size < s) || o.size_max.is_some_and(|s| size > s) {
                    continue;
                }
            }
            let name = index.fold(id);
            let class = if m.words.is_empty() {
                SUBSTRING
            } else if m.path_mode {
                pathbuf.clear();
                index.fold_path_into(id, &mut pathbuf);
                match m.classify(&pathbuf, &pathbuf) {
                    Some(c) => c,
                    None => continue,
                }
            } else {
                match m.classify(index.name(id), name) {
                    Some(c) => c,
                    None => continue,
                }
            };
            let key = rank_key(
                class,
                index.recency(id),
                index.depth(id),
                index.boost(id),
                now,
            );
            if top.worst_key().is_some_and(|w| key < w) {
                continue;
            }
            top.push(Cand { key, id, name });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::category::category_of;
    use crate::index::{FLAG_DIR, FLAG_HIDDEN, IndexBuilder, NONE};
    use std::collections::HashMap;

    const NOW: i64 = 1_800_000_000;

    /// One root "/r" with the given (relative path, mtime, size, hidden) entries,
    /// folders created as needed (in depth-first order: give sorted paths).
    fn build(entries: &[(&str, i64, u64)]) -> Index {
        let mut b = IndexBuilder::new();
        let root = b
            .push(NONE, b"/r", FLAG_DIR, Category::Folder, NOW, 0)
            .unwrap();
        let mut dirs: HashMap<String, u32> = HashMap::new();
        dirs.insert(String::new(), root);
        let mut sorted: Vec<_> = entries.to_vec();
        sorted.sort_by(|a, b| a.0.cmp(b.0));
        for (path, mtime, size) in sorted {
            let (path, is_dir) = match path.strip_suffix('/') {
                Some(p) => (p, true),
                None => (path, false),
            };
            let (parent_path, name) = path.rsplit_once('/').unwrap_or(("", path));
            // make folders on the way
            let mut acc = String::new();
            let mut parent = root;
            for comp in parent_path.split('/').filter(|c| !c.is_empty()) {
                if !acc.is_empty() {
                    acc.push('/');
                }
                acc.push_str(comp);
                parent = *dirs.entry(acc.clone()).or_insert_with(|| {
                    b.push(
                        parent,
                        comp.as_bytes(),
                        FLAG_DIR,
                        Category::Folder,
                        NOW - 10_000_000,
                        0,
                    )
                    .unwrap()
                });
            }
            if is_dir {
                b.push(
                    parent,
                    name.as_bytes(),
                    FLAG_DIR,
                    Category::Folder,
                    mtime,
                    0,
                )
                .unwrap();
            } else {
                let flags = if name.starts_with('.') {
                    FLAG_HIDDEN
                } else {
                    0
                };
                b.push(
                    parent,
                    name.as_bytes(),
                    flags,
                    category_of(name.as_bytes(), false, false),
                    mtime,
                    size,
                )
                .unwrap();
            }
        }
        b.finish(&HashMap::new())
    }

    fn q(ix: &Index, query: &str, o: &Options) -> Vec<String> {
        search(ix, query, 100, o, NOW)
            .iter()
            .map(|h| {
                String::from_utf8_lossy(&h.path)
                    .strip_prefix("/r/")
                    .unwrap()
                    .to_string()
            })
            .collect()
    }

    fn old() -> i64 {
        NOW - 5 * 365 * 86_400
    }

    #[test]
    fn classes_order_results() {
        let t = old();
        let ix = build(&[
            ("rep", t, 1),
            ("rep-final.txt", t, 1),
            ("my rep.txt", t, 1),
            ("Red Egg Plant.txt", t, 1),
            ("storep.txt", t, 1),
            ("nomatch.txt", t, 1),
        ]);
        assert_eq!(
            q(&ix, "rep", &Options::default()),
            [
                "rep",
                "rep-final.txt",
                "my rep.txt",
                "Red Egg Plant.txt",
                "storep.txt"
            ]
        );
    }

    #[test]
    fn diacritics_and_case() {
        let t = old();
        let ix = build(&[
            ("Résumé.pdf", t, 1),
            ("resume.txt", t, 1),
            ("ÅNGSTRÖM.txt", t, 1),
        ]);
        let both = q(&ix, "resume", &Options::default());
        assert_eq!(both.len(), 2);
        assert_eq!(q(&ix, "RÉSUMÉ", &Options::default()).len(), 2);
        assert_eq!(q(&ix, "angstrom", &Options::default()), ["ÅNGSTRÖM.txt"]);
    }

    #[test]
    fn camel_case_is_a_word_prefix() {
        let t = old();
        let ix = build(&[
            ("fooBar.txt", t, 1),
            ("foobar.txt", t, 1),
            ("HTMLParser.txt", t, 1),
        ]);
        assert_eq!(
            q(&ix, "bar", &Options::default()),
            ["fooBar.txt", "foobar.txt"]
        );
        assert_eq!(q(&ix, "parser", &Options::default()), ["HTMLParser.txt"]);
    }

    #[test]
    fn acronym() {
        let t = old();
        let ix = build(&[
            ("My Big Report.docx", t, 1),
            ("unrelated.txt", t, 1),
            ("monthly-budget-report.xls", t, 1),
        ]);
        let r = q(&ix, "mbr", &Options::default());
        assert_eq!(r, ["monthly-budget-report.xls", "My Big Report.docx"]);
    }

    #[test]
    fn every_word_must_match() {
        let t = old();
        let ix = build(&[
            ("big report.txt", t, 1),
            ("big.txt", t, 1),
            ("report.txt", t, 1),
        ]);
        assert_eq!(
            q(&ix, "big report", &Options::default()),
            ["big report.txt"]
        );
        assert_eq!(
            q(&ix, "  report   big ", &Options::default()),
            ["big report.txt"]
        );
        assert!(q(&ix, "big nothing", &Options::default()).is_empty());
    }

    #[test]
    fn empty_query_needs_a_filter() {
        let ix = build(&[("a.png", old(), 1)]);
        assert!(q(&ix, "", &Options::default()).is_empty());
        assert!(q(&ix, "   ", &Options::default()).is_empty());
        let o = Options {
            kinds: Some(Category::Image.bit()),
            ..Default::default()
        };
        assert_eq!(q(&ix, "", &o), ["a.png"]);
    }

    #[test]
    fn filters() {
        let ix = build(&[
            ("d/", NOW - 100, 0),
            ("doc.pdf", NOW - 100, 5000),
            ("doc.png", NOW - 200_000, 50),
            ("docs.txt", old(), 10_000_000),
            (".docrc", NOW - 100, 1),
            ("sub/doc.md", NOW - 100, 1),
        ]);
        let all = Options::default();
        let mut r = q(&ix, "doc", &all);
        r.sort();
        assert_eq!(r, ["doc.pdf", "doc.png", "docs.txt", "sub/doc.md"]);
        assert_eq!(
            q(
                &ix,
                "doc",
                &Options {
                    include_hidden: true,
                    ..Default::default()
                }
            )
            .len(),
            5
        );
        let folders = Options {
            kind: Some(KindFilter::Folder),
            ..Default::default()
        };
        assert_eq!(q(&ix, "d", &folders), ["d"]);
        let files = Options {
            kind: Some(KindFilter::File),
            ..Default::default()
        };
        assert!(!q(&ix, "d", &files).contains(&"d".to_string()));
        let pdf = Options {
            kinds: Some(Category::Pdf.bit() | Category::Image.bit()),
            ..Default::default()
        };
        let mut r = q(&ix, "doc", &pdf);
        r.sort();
        assert_eq!(r, ["doc.pdf", "doc.png"]);
        let after = Options {
            modified_after: Some(NOW - 1000),
            ..Default::default()
        };
        assert_eq!(q(&ix, "doc", &after).len(), 2);
        let before = Options {
            modified_before: Some(NOW - 1000),
            ..Default::default()
        };
        assert_eq!(q(&ix, "doc", &before).len(), 2);
        let big = Options {
            size_min: Some(1000),
            size_max: Some(9_999_999),
            ..Default::default()
        };
        assert_eq!(q(&ix, "doc", &big), ["doc.pdf"]);
        let root = Options {
            root: Some(b"/r/sub".to_vec()),
            ..Default::default()
        };
        assert_eq!(q(&ix, "doc", &root), ["sub/doc.md"]);
        let missing = Options {
            root: Some(b"/r/nowhere".to_vec()),
            ..Default::default()
        };
        assert!(q(&ix, "doc", &missing).is_empty());
        let above = Options {
            root: Some(b"/".to_vec()),
            ..Default::default()
        };
        assert_eq!(q(&ix, "doc", &above).len(), 4);
        let sibling = Options {
            root: Some(b"/rr".to_vec()),
            ..Default::default()
        };
        assert!(q(&ix, "doc", &sibling).is_empty());
    }

    #[test]
    fn path_match() {
        let t = old();
        let ix = build(&[("Projects/Atlas/notes.txt", t, 1), ("notes.txt", t, 1)]);
        let o = Options {
            path_match: true,
            ..Default::default()
        };
        assert_eq!(q(&ix, "atlas notes", &o), ["Projects/Atlas/notes.txt"]);
        assert_eq!(q(&ix, "atlas notes", &Options::default()).len(), 0);
        assert_eq!(
            q(&ix, "projects/atlas", &o),
            ["Projects/Atlas", "Projects/Atlas/notes.txt"]
        );
    }

    #[test]
    fn ranking_recency_depth_boost_then_name() {
        // same class (prefix): newer first
        let ix = build(&[
            ("a1.txt", NOW - 100, 1),
            ("a2.txt", NOW - 3 * 86_400, 1),
            ("a3.txt", NOW - 100 * 86_400, 1),
        ]);
        assert_eq!(
            q(&ix, "a", &Options::default()),
            ["a1.txt", "a2.txt", "a3.txt"]
        );
        // class beats recency
        let ix = build(&[("zz report.txt", NOW - 10, 1), ("report.txt", old(), 1)]);
        assert_eq!(
            q(&ix, "report", &Options::default()),
            ["report.txt", "zz report.txt"]
        );
        // same bucket: shallower first, then Documents boost
        let t = NOW - 100 * 86_400;
        let ix = build(&[
            ("x/y/z/f1.txt", t, 1),
            ("x/f2.txt", t, 1),
            ("f3.txt", t, 1),
            ("Documents/deep/er/f4.txt", t, 1),
        ]);
        let r = q(&ix, "f", &Options::default());
        assert_eq!(
            r,
            [
                "Documents/deep/er/f4.txt",
                "f3.txt",
                "x/f2.txt",
                "x/y/z/f1.txt"
            ]
        );
        // ties by name
        let ix = build(&[("b.txt", t, 1), ("a.txt", t, 1), ("c.txt", t, 1)]);
        assert_eq!(
            q(&ix, "txt", &Options::default()),
            ["a.txt", "b.txt", "c.txt"]
        );
    }

    #[test]
    fn documents_boost_applies() {
        let t = NOW - 100 * 86_400;
        let ix = build(&[("Documents/g.txt", t, 1), ("other/g.txt", t, 1)]);
        assert_eq!(
            q(&ix, "g", &Options::default()),
            ["Documents/g.txt", "other/g.txt"]
        );
    }

    #[test]
    fn limit_is_capped_and_top_k_equals_full_sort() {
        // a pseudo-random index, big enough to use several threads
        let mut x: u64 = 0x2545_F491_4F6C_DD1D;
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        let mut b = IndexBuilder::new();
        let root = b
            .push(NONE, b"/r", FLAG_DIR, Category::Folder, NOW, 0)
            .unwrap();
        let mut dirs = vec![root];
        for i in 0..90_000u32 {
            let parent = dirs[(next() % dirs.len() as u64) as usize];
            let name = format!(
                "{}{}",
                ["alpha", "Beta", "gamma", "delta-x", "My Report"][(next() % 5) as usize],
                next() % 997
            );
            let mtime = NOW - (next() % 400_000_000) as i64;
            if i % 50 == 0 {
                let id = b
                    .push(
                        parent,
                        name.as_bytes(),
                        FLAG_DIR,
                        Category::Folder,
                        mtime,
                        0,
                    )
                    .unwrap();
                dirs.push(id);
            } else {
                b.push(parent, name.as_bytes(), 0, Category::Other, mtime, 1)
                    .unwrap();
            }
        }
        // not depth-first: fine for matching, which does not use subtree ranges
        let ix = b.finish(&HashMap::new());
        let o = Options::default();
        let full = search(&ix, "rep", 500, &o, NOW);
        assert_eq!(full.len(), 500);
        let ten = search(&ix, "rep", 10, &o, NOW);
        let a: Vec<_> = ten.iter().map(|h| &h.path).collect();
        let c: Vec<_> = full.iter().take(10).map(|h| &h.path).collect();
        assert_eq!(a, c);
        // sorted: scores never rise
        assert!(full.windows(2).all(|w| w[0].score >= w[1].score));
        assert_eq!(search(&ix, "rep", 100_000, &o, NOW).len(), 500);
        assert!(search(&ix, "rep", 0, &o, NOW).is_empty());
    }

    #[test]
    fn hits_carry_display_names_and_types() {
        let ix = build(&[("a\nb.png", old(), 7), ("dir/", old(), 0)]);
        let h = search(&ix, "a", 10, &Options::default(), NOW);
        let png = h.iter().find(|h| !h.is_dir).unwrap();
        assert_eq!(png.name, "a\u{240A}b.png");
        assert_eq!(png.mime(), "image/png");
        assert_eq!(png.icon(), "image-png");
        assert_eq!(png.size, 7);
        let d = search(&ix, "dir", 10, &Options::default(), NOW);
        assert!(d[0].is_dir && d[0].size == 0 && d[0].mime() == "inode/directory");
    }

    /// "/r" with these (name, tags, mtime, hidden) files, all at the top.
    fn build_tagged(files: &[(&str, &str, i64)]) -> Index {
        let mut b = IndexBuilder::new();
        let root = b
            .push(NONE, b"/r", FLAG_DIR, Category::Folder, NOW, 0)
            .unwrap();
        for &(name, tags, mtime) in files {
            let flags = if name.starts_with('.') {
                FLAG_HIDDEN
            } else {
                0
            };
            let id = b
                .push(
                    root,
                    name.as_bytes(),
                    flags,
                    category_of(name.as_bytes(), false, false),
                    mtime,
                    1,
                )
                .unwrap();
            b.set_tags(id, tags.as_bytes());
        }
        b.finish(&HashMap::new())
    }

    fn tag(t: &str) -> Options {
        Options {
            tag: Some(t.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn tag_alone_lists_the_tagged_entries_newest_first() {
        let ix = build_tagged(&[
            ("a-old.txt", "Red", NOW - 90 * 86_400),
            ("b-new.txt", "Work,red", NOW - 100),
            ("c-none.txt", "", NOW - 50),
            ("d-other.txt", "Blue", NOW - 60),
            ("e-mid.png", "RED", NOW - 3 * 86_400),
        ]);
        assert_eq!(
            q(&ix, "", &tag("Red")),
            ["b-new.txt", "e-mid.png", "a-old.txt"]
        );
        // no tag, no query: nothing, as before
        assert!(q(&ix, "", &Options::default()).is_empty());
        assert_eq!(q(&ix, "", &tag("blue")), ["d-other.txt"]);
        assert!(q(&ix, "", &tag("green")).is_empty());
        assert!(q(&ix, "", &tag("Re")).is_empty(), "whole tags only");
    }

    #[test]
    fn tag_matching_ignores_case_accents_and_spaces_around() {
        let ix = build_tagged(&[("a.txt", "Taxes 2025,Zo\u{eb}", old())]);
        for t in ["taxes 2025", "TAXES 2025", "  Taxes 2025 ", "zoe", "ZOË"] {
            assert_eq!(q(&ix, "", &tag(t)), ["a.txt"], "{t:?}");
        }
        assert!(q(&ix, "", &tag("taxes")).is_empty());
        assert!(q(&ix, "", &tag("")).is_empty());
        assert!(q(&ix, "", &tag("   ")).is_empty());
    }

    #[test]
    fn tag_combines_with_every_other_option() {
        let t = old();
        let ix = build_tagged(&[
            ("report.pdf", "Work", NOW - 100),
            ("report.txt", "Work", NOW - 100),
            ("notes.txt", "Work", NOW - 100),
            ("report-untagged.txt", "", NOW - 100),
            (".hidden-report.txt", "Work", NOW - 100),
            ("ancient.txt", "Work", t),
        ]);
        let with = |f: &dyn Fn(&mut Options)| {
            let mut o = tag("work");
            f(&mut o);
            o
        };
        assert_eq!(q(&ix, "report", &tag("work")).len(), 2);
        assert_eq!(
            q(&ix, "", &with(&|o| o.kinds = Some(Category::Pdf.bit()))),
            ["report.pdf"]
        );
        assert_eq!(q(&ix, "", &with(&|o| o.include_hidden = true)).len(), 5);
        assert_eq!(q(&ix, "", &tag("work")).len(), 4, "hidden left out");
        assert_eq!(
            q(&ix, "", &with(&|o| o.modified_after = Some(NOW - 1000))).len(),
            3
        );
        assert_eq!(
            q(&ix, "", &with(&|o| o.modified_before = Some(NOW - 1000))),
            ["ancient.txt"]
        );
        assert_eq!(q(&ix, "", &with(&|o| o.size_min = Some(2))).len(), 0);
        assert_eq!(q(&ix, "", &with(&|o| o.size_max = Some(1))).len(), 4);
        assert_eq!(
            q(&ix, "", &with(&|o| o.kind = Some(KindFilter::Folder))).len(),
            0
        );
        assert_eq!(
            q(&ix, "notes", &with(&|o| o.path_match = true)),
            ["notes.txt"]
        );
        assert_eq!(
            q(&ix, "", &with(&|o| o.root = Some(b"/r".to_vec()))).len(),
            4
        );
        assert!(q(&ix, "", &with(&|o| o.root = Some(b"/elsewhere".to_vec()))).is_empty());
    }

    #[test]
    fn tag_search_under_a_root_only_finds_entries_inside_it() {
        let mut b = IndexBuilder::new();
        let r = b
            .push(NONE, b"/r", FLAG_DIR, Category::Folder, NOW, 0)
            .unwrap();
        let d1 = b
            .push(r, b"d1", FLAG_DIR, Category::Folder, NOW - 5, 0)
            .unwrap();
        let x = b.push(d1, b"x.txt", 0, Category::Text, NOW - 5, 1).unwrap();
        b.set_tags(x, b"T");
        let d2 = b
            .push(r, b"d2", FLAG_DIR, Category::Folder, NOW - 5, 0)
            .unwrap();
        let y = b.push(d2, b"y.txt", 0, Category::Text, NOW - 5, 1).unwrap();
        b.set_tags(y, b"T");
        let ix = b.finish(&HashMap::new());
        let o = Options {
            tag: Some("t".into()),
            root: Some(b"/r/d2".to_vec()),
            ..Default::default()
        };
        assert_eq!(q(&ix, "", &o), ["d2/y.txt"]);
        assert_eq!(q(&ix, "", &tag("t")).len(), 2);
    }

    #[test]
    fn tagged_folders_are_hits_but_a_root_is_not() {
        let mut b = IndexBuilder::new();
        let r = b
            .push(NONE, b"/r", FLAG_DIR, Category::Folder, NOW, 0)
            .unwrap();
        b.set_tags(r, b"T");
        let d = b
            .push(r, b"d", FLAG_DIR, Category::Folder, NOW - 5, 0)
            .unwrap();
        b.set_tags(d, b"T");
        let ix = b.finish(&HashMap::new());
        assert_eq!(q(&ix, "", &tag("t")), ["d"]);
    }

    #[test]
    fn hostile_query_is_survived() {
        let ix = build(&[("a.txt", old(), 1)]);
        let long = "q ".repeat(5000);
        assert!(search(&ix, &long, 10, &Options::default(), NOW).is_empty());
        assert!(search(&ix, "\u{0}\u{FFFD}\u{202E}", 10, &Options::default(), NOW).is_empty());
    }
}
