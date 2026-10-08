//! The in-memory index: fixed-size records in depth-first order over one string
//! arena. A folder's subtree is the id range `id..end[id]`, so "under this
//! folder" is a range test and a path is rebuilt from parent links.
//!
//! An [`Index`] is immutable once built; changes make a new one (see
//! `scan::rebuild`) that is published with an `Arc` swap, so a query never
//! waits on a scan.

use crate::category::Category;
use crate::tags;
use crate::text::{fold, fold_bytes};
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

/// Where the tags of one entry are in the tag arena (cleaned, see [`tags`]).
/// Entries without tags have no reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TagRef {
    pub id: u32,
    pub off: u32,
    pub len: u16,
}

/// The tags side table: references sorted by `id`, with offsets that grow, and
/// the text they point into. It is separate from the records so a record stays
/// the same size and an index with no tags costs nothing extra.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TagTable {
    pub refs: Vec<TagRef>,
    pub arena: Vec<u8>,
}

pub struct Index {
    recs: Vec<Record>,
    arena: Vec<u8>,
    tags: TagTable,
    d: Derived,
}

/// Builds an index by appending records in depth-first order.
#[derive(Default)]
pub struct IndexBuilder {
    pub(crate) recs: Vec<Record>,
    pub(crate) arena: Vec<u8>,
    pub(crate) tags: TagTable,
}

/// Where a builder was, to roll back a folder found to be excluded.
#[derive(Clone, Copy)]
pub struct Mark {
    recs: usize,
    arena: usize,
    tag_refs: usize,
    tag_arena: usize,
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
            tag_refs: self.tags.refs.len(),
            tag_arena: self.tags.arena.len(),
        }
    }

    pub fn rollback(&mut self, m: Mark) {
        self.recs.truncate(m.recs);
        self.arena.truncate(m.arena);
        self.tags.refs.truncate(m.tag_refs);
        self.tags.arena.truncate(m.tag_arena);
    }

    /// Set the tags of the record just appended with `push` (or any record
    /// whose id is above that of the last one with tags): `raw` is the value of
    /// `user.xdg.tags`, cleaned by [`tags::clean`]; a value with no usable tag
    /// stores nothing.
    pub fn set_tags(&mut self, id: u32, raw: &[u8]) {
        if raw.is_empty() {
            return;
        }
        self.set_clean_tags(id, &tags::clean(raw));
    }

    fn set_clean_tags(&mut self, id: u32, text: &str) {
        if text.is_empty()
            || id as usize >= self.recs.len()
            || self.tags.refs.last().is_some_and(|r| r.id >= id)
            || self.tags.arena.len() + text.len() > MAX_ARENA
            || text.len() > usize::from(u16::MAX)
        {
            return;
        }
        self.tags.refs.push(TagRef {
            id,
            off: self.tags.arena.len() as u32,
            len: text.len() as u16,
        });
        self.tags.arena.extend_from_slice(text.as_bytes());
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
    /// name instead of folding again. Its tags come along.
    pub fn push_copy(&mut self, old: &Index, id: u32, parent: u32) -> Option<u32> {
        self.push_copy_with(old, id, parent, None)
    }

    /// Like [`IndexBuilder::push_copy`], but with `fresh` (the value of
    /// `user.xdg.tags` read just now) the tags are those, not the old ones.
    pub fn push_copy_with(
        &mut self,
        old: &Index,
        id: u32,
        parent: u32,
        fresh: Option<&[u8]>,
    ) -> Option<u32> {
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
        match fresh {
            Some(raw) => self.set_tags(new_id, raw),
            None => {
                let t = old.tags_of(id);
                if !t.is_empty() {
                    self.set_clean_tags(new_id, t);
                }
            }
        }
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
        self.tags.refs.shrink_to_fit();
        self.tags.arena.shrink_to_fit();
        Index::from_parts_unchecked(self.recs, self.arena, self.tags, used)
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
    /// non-empty and without `/` or NUL except roots (absolute paths). The tag
    /// table is checked too: ids ascending and below the record count, ranges
    /// inside the tag arena, in order and not overlapping, and every text valid
    /// UTF-8 in the form `tags::clean` stores.
    pub fn from_parts(
        recs: Vec<Record>,
        arena: Vec<u8>,
        tag_table: TagTable,
        used: &HashMap<u64, i64>,
    ) -> Result<Index, Invalid> {
        if recs.len() > MAX_RECORDS
            || arena.len() > MAX_ARENA
            || tag_table.refs.len() > recs.len()
            || tag_table.arena.len() > MAX_ARENA
        {
            return Err(Invalid("too large"));
        }
        check_tags(&tag_table, recs.len())?;
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
        Ok(Index::from_parts_unchecked(recs, arena, tag_table, used))
    }

    fn from_parts_unchecked(
        recs: Vec<Record>,
        arena: Vec<u8>,
        tag_table: TagTable,
        used: &HashMap<u64, i64>,
    ) -> Index {
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
            tags: tag_table,
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

    /// The tag table, for the snapshot.
    pub fn tag_table(&self) -> &TagTable {
        &self.tags
    }

    /// The tags of `id` as the cleaned comma-separated text ("" when none).
    pub fn tags_of(&self, id: u32) -> &str {
        match self.tags.refs.binary_search_by_key(&id, |r| r.id) {
            Ok(i) => {
                let r = self.tags.refs[i];
                let b = &self.tags.arena[r.off as usize..r.off as usize + usize::from(r.len)];
                // checked when the table was built or loaded
                std::str::from_utf8(b).unwrap_or("")
            }
            Err(_) => "",
        }
    }

    /// Ids (ascending) of the entries that carry the tag whose
    /// [`tags::folded`] form is `wanted`.
    pub fn ids_with_tag(&self, wanted: &str) -> Vec<u32> {
        self.tags
            .refs
            .iter()
            .filter(|r| tags::contains(self.tags_of(r.id), wanted))
            .map(|r| r.id)
            .collect()
    }

    /// The tags in use as `(name, entries)`, most used first, at most `max`.
    /// Names that differ only in case or accents are one tag; its name is the
    /// spelling used most (ties: the one that sorts first). Folders and files
    /// count alike; roots (places, not search results) and hidden entries (left
    /// out of a search unless asked for) do not.
    pub fn tag_counts(&self, max: usize) -> Vec<(String, u32)> {
        struct Group<'a> {
            count: u32,
            spellings: HashMap<&'a str, u32>,
        }
        let mut groups: HashMap<String, Group<'_>> = HashMap::new();
        for r in &self.tags.refs {
            let rec = &self.recs[r.id as usize];
            if rec.parent == NONE || rec.is_hidden() {
                continue;
            }
            for name in tags::names(self.tags_of(r.id)) {
                let g = groups.entry(fold(name)).or_insert_with(|| Group {
                    count: 0,
                    spellings: HashMap::new(),
                });
                g.count += 1;
                *g.spellings.entry(name).or_insert(0) += 1;
            }
        }
        let mut out: Vec<(String, u32, String)> = groups
            .into_iter()
            .filter_map(|(folded, g)| {
                let best = g
                    .spellings
                    .into_iter()
                    .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(a.0)))?;
                Some((best.0.to_string(), g.count, folded))
            })
            .collect();
        out.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| a.2.cmp(&b.2))
                .then_with(|| a.0.cmp(&b.0))
        });
        out.truncate(max);
        out.into_iter().map(|(n, c, _)| (n, c)).collect()
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

/// Check an untrusted tag table against `n` records (see `Index::from_parts`).
fn check_tags(t: &TagTable, n: usize) -> Result<(), Invalid> {
    let mut prev_id: Option<u32> = None;
    let mut next_off = 0usize;
    for r in &t.refs {
        if prev_id.is_some_and(|p| r.id <= p) {
            return Err(Invalid("tag ids not ascending"));
        }
        if r.id as usize >= n {
            return Err(Invalid("tags for a record that does not exist"));
        }
        let start = r.off as usize;
        let Some(end) = start.checked_add(usize::from(r.len)) else {
            return Err(Invalid("tag text outside the tag arena"));
        };
        if r.len == 0 || end > t.arena.len() {
            return Err(Invalid("tag text outside the tag arena"));
        }
        if start < next_off {
            return Err(Invalid("tag texts overlap"));
        }
        let Ok(text) = std::str::from_utf8(&t.arena[start..end]) else {
            return Err(Invalid("tag text is not UTF-8"));
        };
        if !tags::is_clean(text) {
            return Err(Invalid("tag text is not valid"));
        }
        prev_id = Some(r.id);
        next_off = end;
    }
    Ok(())
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

    /// [`sample`] with tags: a.txt (1) "Red,Work", b.md (3) "red", d.png (5)
    /// "Taxes 2025,RED", Documents (6) "Work", e.pdf (7) "work, Zo\u{eb}".
    pub fn sample_tagged() -> Index {
        let mut b = IndexBuilder::new();
        let r = b
            .push(NONE, b"/r", FLAG_DIR, Category::Folder, 100, 0)
            .unwrap();
        let a = b.push(r, b"a.txt", 0, Category::Text, 200, 10).unwrap();
        b.set_tags(a, b"Red,Work");
        let sub = b
            .push(r, b"sub", FLAG_DIR, Category::Folder, 300, 0)
            .unwrap();
        let m = b.push(sub, b"b.md", 0, Category::Text, 400, 20).unwrap();
        b.set_tags(m, b"red");
        let c = b
            .push(sub, b"c", FLAG_DIR, Category::Folder, 500, 0)
            .unwrap();
        let d = b.push(c, b"d.png", 0, Category::Image, 600, 30).unwrap();
        b.set_tags(d, b"Taxes 2025,RED");
        let docs = b
            .push(r, b"Documents", FLAG_DIR, Category::Folder, 700, 0)
            .unwrap();
        b.set_tags(docs, b"Work");
        let e = b.push(docs, b"e.pdf", 0, Category::Pdf, 800, 40).unwrap();
        b.set_tags(e, "work, Zo\u{eb}".as_bytes());
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
        assert!(Index::from_parts(r.clone(), a.clone(), TagTable::default(), &none).is_ok());
        let mut bad = r.clone();
        bad[1].name_off = u32::MAX;
        assert!(Index::from_parts(bad, a.clone(), TagTable::default(), &none).is_err());
        let mut bad = r.clone();
        bad[1].name_len = 60000;
        assert!(Index::from_parts(bad, a.clone(), TagTable::default(), &none).is_err());
        let mut bad = r.clone();
        bad[1].fold_off = a.len() as u32;
        bad[1].fold_len = 1;
        assert!(Index::from_parts(bad, a.clone(), TagTable::default(), &none).is_err());
        let mut bad = r.clone();
        bad[2].parent = 7; // parent after child
        assert!(Index::from_parts(bad, a.clone(), TagTable::default(), &none).is_err());
        let mut bad = r.clone();
        bad[1].parent = 1; // itself
        assert!(Index::from_parts(bad, a.clone(), TagTable::default(), &none).is_err());
        let mut bad = r.clone();
        bad[1].parent = 5; // a file, and later
        assert!(Index::from_parts(bad, a.clone(), TagTable::default(), &none).is_err());
        let mut bad = r.clone();
        bad[1].flags = 0x80;
        assert!(Index::from_parts(bad, a.clone(), TagTable::default(), &none).is_err());
        let mut bad = r.clone();
        bad[1].cat = 200;
        assert!(Index::from_parts(bad, a.clone(), TagTable::default(), &none).is_err());
        let mut bad = r.clone();
        bad[0].parent = 0; // root pointing at itself
        assert!(Index::from_parts(bad, a.clone(), TagTable::default(), &none).is_err());
        // out of depth-first order: d.png (5) claims the root as parent after sub's subtree closed? valid;
        // but a child of a closed folder is not
        let mut bad = r.clone();
        bad[7].parent = 4; // e.pdf under c, which is closed by Documents
        assert!(Index::from_parts(bad, a.clone(), TagTable::default(), &none).is_err());
        let mut bad = r.clone();
        bad[0].name_len = 1; // root name "/" + nothing is fine; make a relative root
        bad[1].parent = NONE; // a.txt as a root: not a folder
        assert!(Index::from_parts(bad, a, TagTable::default(), &none).is_err());
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
            assert!(
                Index::from_parts(r, a, TagTable::default(), &HashMap::new()).is_err(),
                "{bad}"
            );
        }
        let (mut r, a) = parts_of(&[("/r", NONE, true), ("x", 0, false)]);
        r[1].fold_len = 0;
        assert!(Index::from_parts(r, a, TagTable::default(), &HashMap::new()).is_err());
    }

    #[test]
    fn over_deep_trees_are_refused() {
        let mut v: Vec<(String, u32, bool)> = vec![("/r".into(), NONE, true)];
        for i in 0..(MAX_DEPTH as u32 + 3) {
            v.push(("d".into(), i, true));
        }
        let refs: Vec<(&str, u32, bool)> = v.iter().map(|(n, p, d)| (n.as_str(), *p, *d)).collect();
        let (r, a) = parts_of(&refs);
        assert!(Index::from_parts(r, a, TagTable::default(), &HashMap::new()).is_err());
    }

    #[test]
    fn tags_are_stored_cleaned_and_looked_up_by_id() {
        let ix = testutil::sample_tagged();
        assert_eq!(ix.tags_of(1), "Red,Work");
        assert_eq!(ix.tags_of(3), "red");
        assert_eq!(ix.tags_of(5), "Taxes 2025,RED");
        assert_eq!(ix.tags_of(6), "Work");
        assert_eq!(ix.tags_of(7), "work,Zo\u{eb}");
        for none in [0, 2, 4] {
            assert_eq!(ix.tags_of(none), "");
        }
        assert_eq!(ix.tag_table().refs.len(), 5);
        // an index without tags has an empty table
        assert!(sample().tag_table().refs.is_empty());
        assert_eq!(sample().tags_of(1), "");
        assert_eq!(ix.ids_with_tag("red"), [1, 3, 5]);
        assert_eq!(ix.ids_with_tag("zoe"), [7]);
        assert!(ix.ids_with_tag("blue").is_empty());
        assert!(ix.ids_with_tag("").is_empty());
    }

    #[test]
    fn set_tags_ignores_nothing_to_store_and_out_of_order_ids() {
        let mut b = IndexBuilder::new();
        let r = b
            .push(NONE, b"/r", FLAG_DIR, Category::Folder, 1, 0)
            .unwrap();
        let a = b.push(r, b"a", 0, Category::Other, 1, 0).unwrap();
        let c = b.push(r, b"c", 0, Category::Other, 1, 0).unwrap();
        b.set_tags(c, b"two");
        b.set_tags(a, b"one"); // below the last id with tags: ignored
        b.set_tags(99, b"far"); // no such record
        b.set_tags(r, b""); // nothing
        b.set_tags(r, b" , ,\x01bad"); // nothing usable
        let ix = b.finish(&HashMap::new());
        assert_eq!(ix.tag_table().refs.len(), 1);
        assert_eq!(ix.tags_of(c), "two");
        assert_eq!(ix.tags_of(a), "");
    }

    #[test]
    fn rollback_drops_the_tags_of_the_rolled_back_records() {
        let mut b = IndexBuilder::new();
        let r = b
            .push(NONE, b"/r", FLAG_DIR, Category::Folder, 1, 0)
            .unwrap();
        let keep = b.push(r, b"keep", 0, Category::Other, 1, 0).unwrap();
        b.set_tags(keep, b"kept");
        let mark = b.mark();
        let gone = b.push(r, b"gone", 0, Category::Other, 1, 0).unwrap();
        b.set_tags(gone, b"dropped,too");
        b.rollback(mark);
        let again = b.push(r, b"again", 0, Category::Other, 1, 0).unwrap();
        b.set_tags(again, b"new");
        let ix = b.finish(&HashMap::new());
        assert_eq!(ix.len(), 3);
        assert_eq!(ix.tags_of(1), "kept");
        assert_eq!(ix.tags_of(2), "new");
        assert_eq!(ix.tag_table().arena, b"keptnew");
        assert_eq!(ix.tag_table().refs.len(), 2);
    }

    #[test]
    fn copies_carry_tags_and_fresh_tags_replace_them() {
        let old = testutil::sample_tagged();
        let mut b = IndexBuilder::new();
        for id in 0..old.len() as u32 {
            let parent = old.record(id).parent;
            // fresh tags for a.txt (1): replace; for b.md (3): none now
            let fresh: Option<&[u8]> = match id {
                1 => Some(b"Blue"),
                3 => Some(b""),
                _ => None,
            };
            b.push_copy_with(&old, id, parent, fresh).unwrap();
        }
        let ix = b.finish(&HashMap::new());
        assert_eq!(ix.tags_of(1), "Blue");
        assert_eq!(ix.tags_of(3), "");
        assert_eq!(ix.tags_of(5), "Taxes 2025,RED");
        assert_eq!(ix.tags_of(6), "Work");
        assert_eq!(ix.tags_of(7), "work,Zo\u{eb}");
        // a plain copy keeps all of them
        let mut b = IndexBuilder::new();
        for id in 0..old.len() as u32 {
            b.push_copy(&old, id, old.record(id).parent).unwrap();
        }
        let copy = b.finish(&HashMap::new());
        for id in 0..old.len() as u32 {
            assert_eq!(copy.tags_of(id), old.tags_of(id));
        }
    }

    #[test]
    fn tag_counts_group_ignoring_case_and_pick_the_common_spelling() {
        let ix = testutil::sample_tagged();
        // red: Red, red, RED (3 entries; spellings tie 1-1-1: "RED" sorts first);
        // work: Work (a.txt, Documents), work (e.pdf): "Work" twice wins
        assert_eq!(
            ix.tag_counts(500),
            [
                ("RED".to_string(), 3),
                ("Work".to_string(), 3),
                ("Taxes 2025".to_string(), 1),
                ("Zo\u{eb}".to_string(), 1),
            ]
        );
        assert_eq!(ix.tag_counts(2).len(), 2);
        assert!(ix.tag_counts(0).is_empty());
        assert!(sample().tag_counts(500).is_empty());
    }

    #[test]
    fn tag_counts_leave_out_roots_and_hidden_entries() {
        let mut b = IndexBuilder::new();
        let r = b
            .push(NONE, b"/r", FLAG_DIR, Category::Folder, 1, 0)
            .unwrap();
        // (the scanner never reads a root's tags, but a snapshot may hold them)
        b.set_tags(r, b"root-tag");
        let h = b
            .push(r, b".h", FLAG_HIDDEN, Category::Other, 1, 0)
            .unwrap();
        b.set_tags(h, b"hidden-tag,shared");
        let v = b.push(r, b"v", 0, Category::Other, 1, 0).unwrap();
        b.set_tags(v, b"shared");
        let ix = b.finish(&HashMap::new());
        assert_eq!(ix.tag_counts(10), [("shared".to_string(), 1)]);
    }

    #[test]
    fn tag_counts_order_by_count_then_name() {
        let mut b = IndexBuilder::new();
        let r = b
            .push(NONE, b"/r", FLAG_DIR, Category::Folder, 1, 0)
            .unwrap();
        for (i, t) in ["b,c", "a,c", "C", "d,B"].iter().enumerate() {
            let id = b
                .push(r, format!("f{i}").as_bytes(), 0, Category::Other, 1, 0)
                .unwrap();
            b.set_tags(id, t.as_bytes());
        }
        let ix = b.finish(&HashMap::new());
        let got: Vec<(String, u32)> = ix.tag_counts(10);
        // c x3, b x2 (spelled "B": a tie goes to the first in order), then a and d x1
        assert_eq!(
            got,
            [
                ("c".to_string(), 3),
                ("B".to_string(), 2),
                ("a".to_string(), 1),
                ("d".to_string(), 1)
            ]
        );
    }

    fn tagged_parts() -> (Vec<Record>, Vec<u8>, TagTable) {
        let ix = testutil::sample_tagged();
        (
            ix.records().to_vec(),
            ix.arena().to_vec(),
            ix.tag_table().clone(),
        )
    }

    #[test]
    fn hostile_tag_tables_are_refused() {
        let none = HashMap::new();
        let (r, a, t) = tagged_parts();
        assert!(Index::from_parts(r.clone(), a.clone(), t.clone(), &none).is_ok());
        let refuse = |name: &str, f: &dyn Fn(&mut TagTable)| {
            let mut bad = t.clone();
            f(&mut bad);
            assert!(
                Index::from_parts(r.clone(), a.clone(), bad, &none).is_err(),
                "{name}"
            );
        };
        refuse("id not ascending", &|t| t.refs.swap(0, 1));
        refuse("duplicate id", &|t| t.refs[1].id = t.refs[0].id);
        refuse("id past the records", &|t| t.refs[4].id = 8);
        refuse("id far past the records", &|t| t.refs[4].id = u32::MAX);
        refuse("offset outside", &|t| t.refs[0].off = u32::MAX);
        refuse("offset at the end", &|t| {
            t.refs[0].off = t.arena.len() as u32
        });
        refuse("length outside", &|t| t.refs[4].len = 60_000);
        refuse("length one too long", &|t| t.refs[4].len += 1);
        refuse("empty text", &|t| t.refs[2].len = 0);
        refuse("overlap", &|t| t.refs[1].off = t.refs[0].off);
        refuse("offsets going back", &|t| t.refs[1].off = 0);
        refuse("more entries than records", &|t| {
            let last = t.refs[4];
            t.refs = vec![last; 9];
        });
        refuse("not UTF-8", &|t| t.arena[0] = 0xFF);
        refuse("control character", &|t| t.arena[1] = 0x07);
        refuse("empty name inside", &|t| t.arena[2] = b',');
        refuse("text not in the stored form", &|t| t.arena[0] = b' ');
        refuse("a name over the limit", &|t| {
            t.arena = "x".repeat(tags::MAX_NAME_CHARS + 1).into_bytes();
            t.refs = vec![TagRef {
                id: 1,
                off: 0,
                len: t.arena.len() as u16,
            }];
        });
        refuse("too many names", &|t| {
            let many: Vec<String> = (0..=tags::MAX_TAGS).map(|i| format!("t{i}")).collect();
            t.arena = many.join(",").into_bytes();
            t.refs = vec![TagRef {
                id: 1,
                off: 0,
                len: t.arena.len() as u16,
            }];
        });
        refuse("repeated names", &|t| {
            t.arena = b"Red,red".to_vec();
            t.refs = vec![TagRef {
                id: 1,
                off: 0,
                len: 7,
            }];
        });
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
