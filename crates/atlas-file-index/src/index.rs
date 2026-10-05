//! The in-memory index: fixed-size records in depth-first order over one string
//! arena. A folder's subtree is the id range `id..end[id]`, so "under this
//! folder" is a range test and a path is rebuilt from parent links.
//!
//! An [`Index`] is immutable once built; changes make a new one (see
//! `scan::rebuild`) that is published with an `Arc` swap, so a query never
//! waits on a scan.

use crate::category::Category;
use crate::text::fold_bytes;
use std::collections::HashMap;

/// `parent` of a root.
pub const NONE: u32 = u32::MAX;
/// Most records an index holds (the scan stops there and says so).
pub const MAX_RECORDS: usize = 4_000_000;
/// Most levels `path_of` and `fold_path_into` follow.
const PATH_LEVELS: usize = 4096;
/// Arena size limit: offsets are `u32`.
pub const MAX_ARENA: usize = u32::MAX as usize - (1 << 20);

pub const FLAG_DIR: u8 = 1;
pub const FLAG_HIDDEN: u8 = 2;
pub const FLAG_EXEC: u8 = 4;
/// A folder whose modification time was within [`RECENT_SECS`] of the moment it
/// was listed: a change in the same clock tick could have been missed, so the
/// next reconcile walk lists it again.
pub const FLAG_RECENT: u8 = 8;
const FLAGS_VALID: u8 = FLAG_DIR | FLAG_HIDDEN | FLAG_EXEC | FLAG_RECENT;
/// See [`FLAG_RECENT`].
pub const RECENT_SECS: i64 = 2;
/// Deepest folder level the scanner enters; deeper ones are not indexed.
pub const MAX_DEPTH: usize = 200;

/// One file or folder. `name` is the raw name (a root's is its absolute path);
/// `fold` is the folded name, which shares the bytes when folding changes
/// nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    pub parent: u32,
    pub name_off: u32,
    pub fold_off: u32,
    pub name_len: u16,
    pub fold_len: u16,
    pub flags: u8,
    pub cat: u8,
    pub mtime: i64,
    pub size: u64,
}

impl Record {
    pub fn is_dir(&self) -> bool {
        self.flags & FLAG_DIR != 0
    }
    pub fn is_hidden(&self) -> bool {
        self.flags & FLAG_HIDDEN != 0
    }
}

/// What the matcher needs per entry, derived when an index is finished.
struct Derived {
    end: Vec<u32>,
    depth: Vec<u8>,
    boost: Vec<u8>,
    recency: Vec<i64>,
}

pub struct Index {
    recs: Vec<Record>,
    arena: Vec<u8>,
    d: Derived,
}

/// Builds an index by appending records in depth-first order.
#[derive(Default)]
pub struct IndexBuilder {
    pub(crate) recs: Vec<Record>,
    pub(crate) arena: Vec<u8>,
}

/// Where a builder was, to roll back a folder found to be excluded.
#[derive(Clone, Copy)]
pub struct Mark {
    recs: usize,
    arena: usize,
}

impl IndexBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.recs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.recs.is_empty()
    }

    pub fn is_full(&self) -> bool {
        self.recs.len() >= MAX_RECORDS || self.arena.len() >= MAX_ARENA
    }

    pub fn mark(&self) -> Mark {
        Mark {
            recs: self.recs.len(),
            arena: self.arena.len(),
        }
    }

    pub fn rollback(&mut self, m: Mark) {
        self.recs.truncate(m.recs);
        self.arena.truncate(m.arena);
    }

    /// Append a record; returns its id, or `None` when the index is full or the
    /// name is too long to store.
    pub fn push(
        &mut self,
        parent: u32,
        name: &[u8],
        flags: u8,
        cat: Category,
        mtime: i64,
        size: u64,
    ) -> Option<u32> {
        if self.is_full() || name.len() > usize::from(u16::MAX) || name.is_empty() {
            return None;
        }
        let name_off = self.arena.len() as u32;
        self.arena.extend_from_slice(name);
        let fold = fold_bytes(name);
        let (fold_off, fold_len) = if fold == name {
            (name_off, name.len())
        } else {
            let off = self.arena.len() as u32;
            self.arena.extend_from_slice(&fold);
            (off, fold.len())
        };
        let id = self.recs.len() as u32;
        self.recs.push(Record {
            parent,
            name_off,
            fold_off,
            name_len: name.len() as u16,
            fold_len: fold_len as u16,
            flags,
            cat: cat as u8,
            mtime,
            size,
        });
        Some(id)
    }

    /// Append a copy of record `id` of `old` under `parent`, reusing its folded
    /// name instead of folding again.
    pub fn push_copy(&mut self, old: &Index, id: u32, parent: u32) -> Option<u32> {
        if self.is_full() {
            return None;
        }
        let r = old.recs[id as usize];
        let name_off = self.arena.len() as u32;
        self.arena.extend_from_slice(old.name(id));
        let fold_off = if r.fold_off == r.name_off {
            name_off
        } else {
            let off = self.arena.len() as u32;
            self.arena.extend_from_slice(old.fold(id));
            off
        };
        let new_id = self.recs.len() as u32;
        self.recs.push(Record {
            parent,
            name_off,
            fold_off,
            ..r
        });
        Some(new_id)
    }

    /// Replace the mtime (and, for a folder, the stored nanoseconds) of an appended record.
    pub fn set_times(&mut self, id: u32, mtime: i64, size: u64, recent: bool) {
        if let Some(r) = self.recs.get_mut(id as usize) {
            r.mtime = mtime;
            r.size = size;
            r.flags = (r.flags & !FLAG_RECENT) | if recent { FLAG_RECENT } else { 0 };
        }
    }

    /// Finish into an index; `used` is the last-use time per path hash from
    /// `recently-used.xbel` (see [`path_hash_root`]).
    pub fn finish(mut self, used: &HashMap<u64, i64>) -> Index {
        // the index lives a long time: give back the growth slack
        self.recs.shrink_to_fit();
        self.arena.shrink_to_fit();
        Index::from_parts_unchecked(self.recs, self.arena, used)
    }
}

const FNV: u64 = 0x0000_0100_0000_01B3;

/// Hash of a root's absolute path, the start of every path hash below it.
pub fn path_hash_root(path: &[u8]) -> u64 {
    child_hash(0xCBF2_9CE4_8422_2325, path)
}

/// Hash of `name` inside the folder hashed `parent`.
pub fn child_hash(parent: u64, name: &[u8]) -> u64 {
    let mut h = parent ^ 0x9E37_79B9_7F4A_7C15;
    for &b in name {
        h ^= u64::from(b);
        h = h.wrapping_mul(FNV);
    }
    h ^= 0xFF;
    h.wrapping_mul(FNV)
}

/// Why a decoded set of records was refused.
#[derive(Debug, PartialEq, Eq)]
pub struct Invalid(pub &'static str);

impl Index {
    pub fn empty() -> Index {
        IndexBuilder::new().finish(&HashMap::new())
    }

    /// Check records read from an untrusted source and build an index: every
    /// offset and length inside the arena, flags and categories known, parents
    /// before children and on the path from the root (depth-first order), names
    /// non-empty and without `/` or NUL except roots (absolute paths).
    pub fn from_parts(
        recs: Vec<Record>,
        arena: Vec<u8>,
        used: &HashMap<u64, i64>,
    ) -> Result<Index, Invalid> {
        if recs.len() > MAX_RECORDS || arena.len() > MAX_ARENA {
            return Err(Invalid("too large"));
        }
        let alen = arena.len();
        let in_arena = |off: u32, len: u16| {
            (off as usize)
                .checked_add(usize::from(len))
                .is_some_and(|e| e <= alen)
        };
        let mut stack: Vec<u32> = Vec::new();
        for (i, r) in recs.iter().enumerate() {
            if !in_arena(r.name_off, r.name_len) || !in_arena(r.fold_off, r.fold_len) {
                return Err(Invalid("offset outside the string arena"));
            }
            if r.name_len == 0 || r.fold_len == 0 {
                return Err(Invalid("empty name"));
            }
            if r.flags & !FLAGS_VALID != 0 || Category::from_u8(r.cat).is_none() {
                return Err(Invalid("unknown flags or category"));
            }
            if (r.cat == Category::Folder as u8) != r.is_dir() {
                return Err(Invalid("category does not match the folder flag"));
            }
            let name = &arena[r.name_off as usize..r.name_off as usize + usize::from(r.name_len)];
            if r.parent == NONE {
                if !r.is_dir() || name[0] != b'/' || name.contains(&0) {
                    return Err(Invalid("bad root"));
                }
                stack.clear();
            } else {
                if r.parent as usize >= i {
                    return Err(Invalid("parent after child"));
                }
                if name.contains(&b'/') || name.contains(&0) || name == b"." || name == b".." {
                    return Err(Invalid("bad name"));
                }
                // the folded name is derived from the name, so it cannot be longer
                // than the longest folding of it (4 bytes per byte)
                let fold =
                    &arena[r.fold_off as usize..r.fold_off as usize + usize::from(r.fold_len)];
                if fold.contains(&b'/') || fold.contains(&0) {
                    return Err(Invalid("bad folded name"));
                }
                while let Some(&t) = stack.last() {
                    if t == r.parent {
                        break;
                    }
                    stack.pop();
                }
                if stack.is_empty() {
                    return Err(Invalid("records not in depth-first order"));
                }
                if stack.len() > MAX_DEPTH + 1 {
                    return Err(Invalid("folders nested too deeply"));
                }
            }
            if r.is_dir() {
                stack.push(i as u32);
            }
        }
        Ok(Index::from_parts_unchecked(recs, arena, used))
    }

    fn from_parts_unchecked(recs: Vec<Record>, arena: Vec<u8>, used: &HashMap<u64, i64>) -> Index {
        let n = recs.len();
        let mut end: Vec<u32> = (1..=n as u32).collect();
        for i in (0..n).rev() {
            let p = recs[i].parent;
            if p != NONE {
                let e = end[i];
                if e > end[p as usize] {
                    end[p as usize] = e;
                }
            }
        }
        let mut depth = vec![0u8; n];
        let mut boost = vec![0u8; n];
        let mut recency: Vec<i64> = recs.iter().map(|r| r.mtime).collect();
        let mut hash: Vec<u64> = Vec::new();
        if !used.is_empty() {
            hash = vec![0u64; n];
        }
        for (i, r) in recs.iter().enumerate() {
            let name = &arena[r.name_off as usize..r.name_off as usize + usize::from(r.name_len)];
            if r.parent == NONE {
                if !hash.is_empty() {
                    hash[i] = path_hash_root(name);
                }
            } else {
                let p = r.parent as usize;
                depth[i] = depth[p].saturating_add(1);
                boost[i] = u8::from(
                    boost[p] != 0 || (recs[p].parent == NONE && r.is_dir() && is_home_folder(name)),
                );
                if !hash.is_empty() {
                    hash[i] = child_hash(hash[p], name);
                }
            }
            if !hash.is_empty()
                && let Some(&t) = used.get(&hash[i])
                && t > recency[i]
            {
                recency[i] = t;
            }
        }
        Index {
            recs,
            arena,
            d: Derived {
                end,
                depth,
                boost,
                recency,
            },
        }
    }

    pub fn len(&self) -> usize {
        self.recs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.recs.is_empty()
    }

    pub fn records(&self) -> &[Record] {
        &self.recs
    }

    pub fn arena(&self) -> &[u8] {
        &self.arena
    }

    pub fn record(&self, id: u32) -> &Record {
        &self.recs[id as usize]
    }

    /// Raw name (a root's is its absolute path).
    pub fn name(&self, id: u32) -> &[u8] {
        let r = &self.recs[id as usize];
        &self.arena[r.name_off as usize..r.name_off as usize + usize::from(r.name_len)]
    }

    pub fn fold(&self, id: u32) -> &[u8] {
        let r = &self.recs[id as usize];
        &self.arena[r.fold_off as usize..r.fold_off as usize + usize::from(r.fold_len)]
    }

    /// One past the last id of the subtree of `id`.
    pub fn end(&self, id: u32) -> u32 {
        self.d.end[id as usize]
    }

    pub fn depth(&self, id: u32) -> u8 {
        self.d.depth[id as usize]
    }

    /// 1 below Desktop, Documents or Downloads of a root.
    pub fn boost(&self, id: u32) -> u8 {
        self.d.boost[id as usize]
    }

    /// Newer of the modification time and the last use.
    pub fn recency(&self, id: u32) -> i64 {
        self.d.recency[id as usize]
    }

    pub fn roots(&self) -> impl Iterator<Item = u32> + '_ {
        self.recs
            .iter()
            .enumerate()
            .filter(|(_, r)| r.parent == NONE)
            .map(|(i, _)| i as u32)
    }

    /// Absolute path of `id` as bytes.
    pub fn path_of(&self, id: u32) -> Vec<u8> {
        let mut chain = Vec::with_capacity(16);
        let mut cur = id;
        while cur != NONE && chain.len() < PATH_LEVELS {
            chain.push(cur);
            cur = self.recs[cur as usize].parent;
        }
        let mut out = Vec::new();
        for (k, &c) in chain.iter().rev().enumerate() {
            if k > 0 {
                out.push(b'/');
            }
            let n = self.name(c);
            if k == 0 && n == b"/" {
                continue;
            }
            out.extend_from_slice(n);
        }
        out
    }

    /// Append the folded path of `id` to `out` (for `match: path`).
    pub fn fold_path_into(&self, id: u32, out: &mut Vec<u8>) {
        let mut chain: [u32; 16] = [0; 16];
        let mut long: Vec<u32> = Vec::new();
        let mut n = 0;
        let mut cur = id;
        while cur != NONE && n < PATH_LEVELS {
            if n < chain.len() {
                chain[n] = cur;
            } else {
                if long.is_empty() {
                    long.extend_from_slice(&chain);
                }
                long.push(cur);
            }
            n += 1;
            cur = self.recs[cur as usize].parent;
        }
        let chain: &[u32] = if long.is_empty() { &chain[..n] } else { &long };
        for (k, &c) in chain.iter().rev().enumerate() {
            if k > 0 {
                out.push(b'/');
            }
            let f = self.fold(c);
            if k == 0 && f == b"/" {
                continue;
            }
            out.extend_from_slice(f);
        }
    }

    /// Path hash (see [`path_hash_root`]) to id of every folder: a quick way to
    /// find the folder of many paths at once.
    pub fn dir_hashes(&self) -> HashMap<u64, u32> {
        let n = self.recs.len();
        let mut hash = vec![0u64; n];
        let mut out = HashMap::new();
        for (i, r) in self.recs.iter().enumerate() {
            let name = self.name(i as u32);
            hash[i] = if r.parent == NONE {
                path_hash_root(name)
            } else {
                child_hash(hash[r.parent as usize], name)
            };
            if r.is_dir() {
                out.insert(hash[i], i as u32);
            }
        }
        out
    }

    /// The deepest indexed folder on `path` (a folder, or the folder of a file),
    /// using a table from [`Index::dir_hashes`]; one hash step per path part.
    pub fn deepest_dir(&self, table: &HashMap<u64, u32>, path: &[u8]) -> Option<u32> {
        for r in self.roots() {
            let rn = self.name(r);
            let rest = if path == rn {
                &path[path.len()..]
            } else if rn == b"/" && path.len() > 1 {
                &path[1..]
            } else if path.len() > rn.len() && path.starts_with(rn) && path[rn.len()] == b'/' {
                &path[rn.len() + 1..]
            } else {
                continue;
            };
            let mut h = path_hash_root(rn);
            let mut found = r;
            for comp in rest.split(|&b| b == b'/').filter(|c| !c.is_empty()) {
                h = child_hash(h, comp);
                match table.get(&h) {
                    Some(&id) => found = id,
                    None => break,
                }
            }
            return Some(found);
        }
        None
    }

    /// The child of `dir` called `name`.
    pub fn child(&self, dir: u32, name: &[u8]) -> Option<u32> {
        let end = self.end(dir);
        let mut j = dir + 1;
        while j < end {
            if self.name(j) == name {
                return Some(j);
            }
            j = self.end(j);
        }
        None
    }

    /// The record for an absolute path, if indexed.
    pub fn find_path(&self, path: &[u8]) -> Option<u32> {
        let (id, rest) = self.find_deepest(path)?;
        if rest.is_empty() { Some(id) } else { None }
    }

    /// The deepest indexed folder on `path`, and the rest of the path below it.
    pub fn find_deepest<'a>(&self, path: &'a [u8]) -> Option<(u32, &'a [u8])> {
        for r in self.roots() {
            let rn = self.name(r);
            let rest = if path == rn {
                &path[path.len()..]
            } else if rn == b"/" {
                &path[1..]
            } else if path.len() > rn.len() && path.starts_with(rn) && path[rn.len()] == b'/' {
                &path[rn.len() + 1..]
            } else {
                continue;
            };
            let mut cur = r;
            let mut rest = rest;
            while !rest.is_empty() {
                let (comp, tail) = match memchr::memchr(b'/', rest) {
                    Some(p) => (&rest[..p], &rest[p + 1..]),
                    None => (rest, &rest[rest.len()..]),
                };
                if comp.is_empty() {
                    rest = tail;
                    continue;
                }
                match self.child(cur, comp) {
                    Some(c) if self.recs[c as usize].is_dir() || tail.is_empty() => {
                        cur = c;
                        rest = tail;
                    }
                    _ => return Some((cur, rest)),
                }
            }
            return Some((cur, rest));
        }
        None
    }
}

fn is_home_folder(name: &[u8]) -> bool {
    matches!(name, b"Desktop" | b"Documents" | b"Downloads")
}

#[cfg(test)]
pub(crate) mod testutil {
    use super::*;

    /// A small index: /r with a.txt, sub/(b.md, c/(d.png)), Documents/e.pdf.
    pub fn sample() -> Index {
        let mut b = IndexBuilder::new();
        let r = b
            .push(NONE, b"/r", FLAG_DIR, Category::Folder, 100, 0)
            .unwrap();
        b.push(r, b"a.txt", 0, Category::Text, 200, 10).unwrap();
        let sub = b
            .push(r, b"sub", FLAG_DIR, Category::Folder, 300, 0)
            .unwrap();
        b.push(sub, b"b.md", 0, Category::Text, 400, 20).unwrap();
        let c = b
            .push(sub, b"c", FLAG_DIR, Category::Folder, 500, 0)
            .unwrap();
        b.push(c, b"d.png", 0, Category::Image, 600, 30).unwrap();
        let docs = b
            .push(r, b"Documents", FLAG_DIR, Category::Folder, 700, 0)
            .unwrap();
        b.push(docs, b"e.pdf", 0, Category::Pdf, 800, 40).unwrap();
        b.finish(&HashMap::new())
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::sample;
    use super::*;

    #[test]
    fn structure() {
        let ix = sample();
        assert_eq!(ix.len(), 8);
        assert_eq!(ix.end(0), 8);
        assert_eq!(ix.end(2), 6);
        assert_eq!(ix.path_of(5), b"/r/sub/c/d.png");
        assert_eq!(ix.find_path(b"/r/sub/c"), Some(4));
        assert_eq!(ix.find_path(b"/r/sub/zz"), None);
        assert_eq!(ix.find_path(b"/other"), None);
        assert_eq!(
            ix.find_deepest(b"/r/sub/new/x")
                .map(|(i, r)| (i, r.to_vec())),
            Some((2, b"new/x".to_vec()))
        );
        assert_eq!(ix.child(0, b"Documents"), Some(6));
        assert_eq!(ix.depth(5), 3);
        assert_eq!(ix.boost(7), 1);
        assert_eq!(ix.boost(1), 0);
        let mut f = Vec::new();
        ix.fold_path_into(7, &mut f);
        assert_eq!(f, b"/r/documents/e.pdf");
    }

    #[test]
    fn recency_uses_last_use() {
        let mut b = IndexBuilder::new();
        let r = b
            .push(NONE, b"/r", FLAG_DIR, Category::Folder, 1, 0)
            .unwrap();
        b.push(r, b"a", 0, Category::Other, 10, 0).unwrap();
        let mut used = HashMap::new();
        used.insert(child_hash(path_hash_root(b"/r"), b"a"), 999);
        let ix = b.finish(&used);
        assert_eq!(ix.recency(1), 999);
        assert_eq!(ix.record(1).mtime, 10);
    }

    fn parts(ix: &Index) -> (Vec<Record>, Vec<u8>) {
        (ix.records().to_vec(), ix.arena().to_vec())
    }

    #[test]
    fn hostile_records_are_refused() {
        let ix = sample();
        let none = HashMap::new();
        let (r, a) = parts(&ix);
        assert!(Index::from_parts(r.clone(), a.clone(), &none).is_ok());
        let mut bad = r.clone();
        bad[1].name_off = u32::MAX;
        assert!(Index::from_parts(bad, a.clone(), &none).is_err());
        let mut bad = r.clone();
        bad[1].name_len = 60000;
        assert!(Index::from_parts(bad, a.clone(), &none).is_err());
        let mut bad = r.clone();
        bad[1].fold_off = a.len() as u32;
        bad[1].fold_len = 1;
        assert!(Index::from_parts(bad, a.clone(), &none).is_err());
        let mut bad = r.clone();
        bad[2].parent = 7; // parent after child
        assert!(Index::from_parts(bad, a.clone(), &none).is_err());
        let mut bad = r.clone();
        bad[1].parent = 1; // itself
        assert!(Index::from_parts(bad, a.clone(), &none).is_err());
        let mut bad = r.clone();
        bad[1].parent = 5; // a file, and later
        assert!(Index::from_parts(bad, a.clone(), &none).is_err());
        let mut bad = r.clone();
        bad[1].flags = 0x80;
        assert!(Index::from_parts(bad, a.clone(), &none).is_err());
        let mut bad = r.clone();
        bad[1].cat = 200;
        assert!(Index::from_parts(bad, a.clone(), &none).is_err());
        let mut bad = r.clone();
        bad[0].parent = 0; // root pointing at itself
        assert!(Index::from_parts(bad, a.clone(), &none).is_err());
        // out of depth-first order: d.png (5) claims the root as parent after sub's subtree closed? valid;
        // but a child of a closed folder is not
        let mut bad = r.clone();
        bad[7].parent = 4; // e.pdf under c, which is closed by Documents
        assert!(Index::from_parts(bad, a.clone(), &none).is_err());
        let mut bad = r.clone();
        bad[0].name_len = 1; // root name "/" + nothing is fine; make a relative root
        bad[1].parent = NONE; // a.txt as a root: not a folder
        assert!(Index::from_parts(bad, a, &none).is_err());
    }

    fn parts_of(names: &[(&str, u32, bool)]) -> (Vec<Record>, Vec<u8>) {
        let mut b = IndexBuilder::new();
        for &(n, parent, dir) in names {
            let (flags, cat) = if dir {
                (FLAG_DIR, Category::Folder)
            } else {
                (0, Category::Text)
            };
            b.push(parent, n.as_bytes(), flags, cat, 1, 1).unwrap();
        }
        (b.recs, b.arena)
    }

    #[test]
    fn dot_and_dotdot_names_are_refused() {
        for bad in [".", ".."] {
            let (r, a) = parts_of(&[("/r", NONE, true), (bad, 0, false)]);
            assert!(Index::from_parts(r, a, &HashMap::new()).is_err(), "{bad}");
        }
        let (mut r, a) = parts_of(&[("/r", NONE, true), ("x", 0, false)]);
        r[1].fold_len = 0;
        assert!(Index::from_parts(r, a, &HashMap::new()).is_err());
    }

    #[test]
    fn over_deep_trees_are_refused() {
        let mut v: Vec<(String, u32, bool)> = vec![("/r".into(), NONE, true)];
        for i in 0..(MAX_DEPTH as u32 + 3) {
            v.push(("d".into(), i, true));
        }
        let refs: Vec<(&str, u32, bool)> = v.iter().map(|(n, p, d)| (n.as_str(), *p, *d)).collect();
        let (r, a) = parts_of(&refs);
        assert!(Index::from_parts(r, a, &HashMap::new()).is_err());
    }

    #[test]
    fn deepest_dir_finds_folders_and_files_folders() {
        let ix = sample();
        let t = ix.dir_hashes();
        assert_eq!(ix.deepest_dir(&t, b"/r/sub/c"), Some(4));
        assert_eq!(ix.deepest_dir(&t, b"/r/sub/c/d.png"), Some(4));
        assert_eq!(ix.deepest_dir(&t, b"/r/sub/new/x"), Some(2));
        assert_eq!(ix.deepest_dir(&t, b"/elsewhere"), None);
    }
}
