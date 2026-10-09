//! The snapshot file `$XDG_CACHE_HOME/telamon-explorer/index/v2.idx`: a cache, so
//! deleting it only costs a rescan. Flat and little-endian:
//!
//! ```text
//! header, 64 bytes
//!   0  magic "ATLASIDX"      8  version u32 (2)        12 header length u32 (64)
//!   16 record count u32      20 exclusion hash u32          24 arena length u64
//!   32 saved at i64 (secs)   40 crc32 of the body
//!   44 crc32 of bytes 0..44 and 48..56
//!   48 tag entry count u32   52 tag arena length u32        56 reserved, 8 bytes
//! body
//!   record count x 40-byte records
//!     0 parent u32 (0xFFFFFFFF for a root)   4 name offset u32   8 folded name offset u32
//!     12 name length u16   14 folded length u16   16 flags u8   17 category u8
//!     18 reserved u16   20 reserved u32   24 mtime i64   32 size u64
//!   string arena (names; a root's name is its absolute path)
//!   tag entry count x 12-byte tag entries, sorted by record id
//!     0 record id u32   4 offset into the tag arena u32   8 length u16
//!     10 reserved u16 (zero)
//!   tag arena (per entry the file's tags as UTF-8 names joined by ",")
//! ```
//!
//! Version 2 added the tag sections; a version 1 file (`v1.idx`) is not read
//! and is deleted when a snapshot is written. The version is checked before the
//! header checksum so an older file is named as such.
//!
//! The reader treats the file as untrusted: the checksum catches damage, not
//! tampering, so every length and offset is checked against the file and the
//! arena, and the records must form a valid depth-first tree (`Index::from_parts`);
//! the tag entries must be in order, inside the tag arena and hold valid text.
//! Anything wrong means "no snapshot": the caller scans again.
//!
//! The folder follows the Store's cache rules: `telamon-explorer/index` under the
//! cache home must be a real folder (not a symlink), owned by the user, with no
//! group or other write bit (set back to 0700 on a write, refused on a read);
//! the file is opened with `O_NOFOLLOW` and must be a regular file of the user's
//! own; it is written to a temp file, synced, then renamed.

use crate::index::{Index, MAX_ARENA, MAX_RECORDS, MAX_TAG_ARENA, Record, TagRef, TagTable};
use std::collections::HashMap;
use std::ffi::CString;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::path::Path;

const MAGIC: &[u8; 8] = b"ATLASIDX";
pub const VERSION: u32 = 2;
const HEADER: usize = 64;
const RECORD: usize = 40;
const TAG_ENTRY: usize = 12;
const FILE_NAME: &str = "v2.idx";
const TMP_NAME: &str = "v2.idx.tmp";
/// The names of the version before: removed when a snapshot is written.
const OLD_NAMES: [&str; 2] = ["v1.idx", "v1.idx.tmp"];
/// Largest snapshot file read or written: the records, the arena and the tags
/// at their limits (see `index::MAX_ARENA`), and well below the service's
/// memory ceiling when the file is read.
pub const MAX_FILE: u64 = 384 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum SnapshotError {
    /// Written by another version of the format.
    WrongVersion(u32),
    /// Damaged or hostile; the reason is for the log.
    Corrupt(&'static str),
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SnapshotError::WrongVersion(v) => write!(f, "written by another version ({v})"),
            SnapshotError::Corrupt(why) => write!(f, "damaged ({why})"),
        }
    }
}

fn le32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn le16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
/// The checksum of the header: bytes 0..44 and 48..56 (the checksums
/// themselves are left out).
fn header_crc(b: &[u8]) -> u32 {
    let mut h = crc32fast::Hasher::new();
    h.update(&b[0..44]);
    h.update(&b[48..56]);
    h.finalize()
}
fn le64(b: &[u8], o: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[o..o + 8]);
    u64::from_le_bytes(a)
}

/// Serialise an index.
pub fn encode(index: &Index, saved_at: i64) -> Vec<u8> {
    encode_with(index, saved_at, 0)
}

/// Serialise an index scanned under the exclusion set `excl_hash`
/// (`Excludes::fingerprint`): a snapshot made under other rules is not used.
pub fn encode_with(index: &Index, saved_at: i64, excl_hash: u32) -> Vec<u8> {
    let recs = index.records();
    let arena = index.arena();
    let tags = index.tag_table();
    let mut out = Vec::with_capacity(
        HEADER + recs.len() * RECORD + arena.len() + tags.refs.len() * TAG_ENTRY + tags.arena.len(),
    );
    out.resize(HEADER, 0);
    for r in recs {
        out.extend_from_slice(&r.parent.to_le_bytes());
        out.extend_from_slice(&r.name_off.to_le_bytes());
        out.extend_from_slice(&r.fold_off.to_le_bytes());
        out.extend_from_slice(&r.name_len.to_le_bytes());
        out.extend_from_slice(&r.fold_len.to_le_bytes());
        out.push(r.flags);
        out.push(r.cat);
        out.extend_from_slice(&[0u8; 2]);
        out.extend_from_slice(&[0u8; 4]);
        out.extend_from_slice(&r.mtime.to_le_bytes());
        out.extend_from_slice(&r.size.to_le_bytes());
    }
    out.extend_from_slice(arena);
    for t in &tags.refs {
        out.extend_from_slice(&t.id.to_le_bytes());
        out.extend_from_slice(&t.off.to_le_bytes());
        out.extend_from_slice(&t.len.to_le_bytes());
        out.extend_from_slice(&[0u8; 2]);
    }
    out.extend_from_slice(&tags.arena);
    let body_crc = crc32fast::hash(&out[HEADER..]);
    out[0..8].copy_from_slice(MAGIC);
    out[8..12].copy_from_slice(&VERSION.to_le_bytes());
    out[12..16].copy_from_slice(&(HEADER as u32).to_le_bytes());
    out[16..20].copy_from_slice(&(recs.len() as u32).to_le_bytes());
    out[20..24].copy_from_slice(&excl_hash.to_le_bytes());
    out[24..32].copy_from_slice(&(arena.len() as u64).to_le_bytes());
    out[32..40].copy_from_slice(&saved_at.to_le_bytes());
    out[40..44].copy_from_slice(&body_crc.to_le_bytes());
    out[48..52].copy_from_slice(&(tags.refs.len() as u32).to_le_bytes());
    out[52..56].copy_from_slice(&(tags.arena.len() as u32).to_le_bytes());
    let head_crc = header_crc(&out);
    out[44..48].copy_from_slice(&head_crc.to_le_bytes());
    out
}

/// Check and decode a snapshot. `used` is the last-use table for the index.
pub fn decode(bytes: &[u8], used: &HashMap<u64, i64>) -> Result<(Index, i64), SnapshotError> {
    decode_with(bytes, used).map(|(i, t, _)| (i, t))
}

/// Like [`decode`], also returning the exclusion hash the snapshot was made under.
pub fn decode_with(
    bytes: &[u8],
    used: &HashMap<u64, i64>,
) -> Result<(Index, i64, u32), SnapshotError> {
    use SnapshotError::*;
    if bytes.len() < HEADER {
        return Err(Corrupt("shorter than the header"));
    }
    if &bytes[0..8] != MAGIC {
        return Err(Corrupt("not an index file"));
    }
    let version = le32(bytes, 8);
    if version != VERSION {
        return Err(WrongVersion(version));
    }
    if header_crc(bytes) != le32(bytes, 44) {
        return Err(Corrupt("header checksum"));
    }
    if le32(bytes, 12) as usize != HEADER {
        return Err(Corrupt("header length"));
    }
    let n = le32(bytes, 16) as usize;
    let arena_len = le64(bytes, 24);
    let tag_n = le32(bytes, 48) as usize;
    let tag_arena_len = le32(bytes, 52) as usize;
    if n > MAX_RECORDS || arena_len > MAX_ARENA as u64 || tag_n > n || tag_arena_len > MAX_TAG_ARENA
    {
        return Err(Corrupt("counts out of range"));
    }
    let arena_len = arena_len as usize;
    let expect = HEADER
        .checked_add(n.checked_mul(RECORD).ok_or(Corrupt("size overflow"))?)
        .and_then(|v| v.checked_add(arena_len))
        .and_then(|v| v.checked_add(tag_n.checked_mul(TAG_ENTRY)?))
        .and_then(|v| v.checked_add(tag_arena_len))
        .ok_or(Corrupt("size overflow"))?;
    if expect != bytes.len() {
        return Err(Corrupt("length does not match the header"));
    }
    if crc32fast::hash(&bytes[HEADER..]) != le32(bytes, 40) {
        return Err(Corrupt("checksum"));
    }
    let mut recs = Vec::with_capacity(n);
    for i in 0..n {
        let o = HEADER + i * RECORD;
        recs.push(Record {
            parent: le32(bytes, o),
            name_off: le32(bytes, o + 4),
            fold_off: le32(bytes, o + 8),
            name_len: u16::from_le_bytes([bytes[o + 12], bytes[o + 13]]),
            fold_len: u16::from_le_bytes([bytes[o + 14], bytes[o + 15]]),
            flags: bytes[o + 16],
            cat: bytes[o + 17],
            mtime: le64(bytes, o + 24) as i64,
            size: le64(bytes, o + 32),
        });
    }
    let arena_at = HEADER + n * RECORD;
    let tags_at = arena_at + arena_len;
    let tag_text_at = tags_at + tag_n * TAG_ENTRY;
    let arena = bytes[arena_at..tags_at].to_vec();
    let mut refs = Vec::with_capacity(tag_n);
    for i in 0..tag_n {
        let o = tags_at + i * TAG_ENTRY;
        if le16(bytes, o + 10) != 0 {
            return Err(Corrupt("reserved tag field is not zero"));
        }
        refs.push(TagRef {
            id: le32(bytes, o),
            off: le32(bytes, o + 4),
            len: le16(bytes, o + 8),
        });
    }
    let table = TagTable {
        refs,
        arena: bytes[tag_text_at..].to_vec(),
    };
    let index = Index::from_parts(recs, arena, table, used).map_err(|e| Corrupt(e.0))?;
    Ok((index, le64(bytes, 32) as i64, le32(bytes, 20)))
}

/// The cache folder, held open by file descriptor so the file operations cannot
/// be redirected by swapping a path component for a symlink.
pub struct CacheDir {
    fd: OwnedFd,
}

fn cstr(s: &str) -> io::Result<CString> {
    CString::new(s).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
}

fn check_dir(fd: RawFd, what: &str, fix: bool) -> io::Result<()> {
    // SAFETY: st is a plain-old-data struct filled in by fstat.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: fd is open; st is valid for writes.
    if unsafe { libc::fstat(fd, &mut st) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: geteuid has no arguments and cannot fail.
    if st.st_uid != unsafe { libc::geteuid() } {
        return Err(io::Error::other(format!("{what} is not owned by you")));
    }
    if st.st_mode & 0o022 != 0 {
        if !fix {
            return Err(io::Error::other(format!("{what} can be written by others")));
        }
        log::warn!("{what} was writable by others, set to 0700");
        // SAFETY: fd is open.
        if unsafe { libc::fchmod(fd, 0o700) } != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

impl CacheDir {
    /// Open `<cache_home>/telamon-explorer/index`; with `create`, make it (0700).
    /// `Ok(None)` when it does not exist and `create` is false. Folders above
    /// `telamon-explorer` may be symlinks (a moved `~/.cache`); the two below may not.
    pub fn open(cache_home: &Path, create: bool) -> io::Result<Option<CacheDir>> {
        use std::os::unix::fs::DirBuilderExt;
        if create {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(cache_home)?;
        }
        let base = match File::open(cache_home) {
            Ok(f) => OwnedFd::from(f),
            Err(e) if e.kind() == io::ErrorKind::NotFound && !create => return Ok(None),
            Err(e) => return Err(e),
        };
        let mut cur = base;
        for name in ["telamon-explorer", "index"] {
            let c = cstr(name)?;
            if create {
                // SAFETY: cur is an open directory; c is NUL-terminated.
                let r = unsafe { libc::mkdirat(cur.as_raw_fd(), c.as_ptr(), 0o700) };
                if r != 0 {
                    let e = io::Error::last_os_error();
                    if e.raw_os_error() != Some(libc::EEXIST) {
                        return Err(e);
                    }
                }
            }
            // SAFETY: as above; O_NOFOLLOW makes a symlink fail with ELOOP.
            let fd = unsafe {
                libc::openat(
                    cur.as_raw_fd(),
                    c.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            if fd < 0 {
                let e = io::Error::last_os_error();
                return match e.raw_os_error() {
                    Some(libc::ENOENT) if !create => Ok(None),
                    Some(libc::ELOOP) | Some(libc::ENOTDIR) => Err(io::Error::other(format!(
                        "cache folder {name} is not a real folder (a symlink?)"
                    ))),
                    _ => Err(e),
                };
            }
            // SAFETY: fd is new and owned by nobody else.
            let next = unsafe { OwnedFd::from_raw_fd(fd) };
            check_dir(next.as_raw_fd(), &format!("cache folder {name}"), create)?;
            cur = next;
        }
        Ok(Some(CacheDir { fd: cur }))
    }

    /// The snapshot's bytes; `Ok(None)` when there is none.
    pub fn read(&self) -> io::Result<Option<Vec<u8>>> {
        let c = cstr(FILE_NAME)?;
        // SAFETY: the folder fd is open; c is NUL-terminated.
        let fd = unsafe {
            libc::openat(
                self.fd.as_raw_fd(),
                c.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            )
        };
        if fd < 0 {
            let e = io::Error::last_os_error();
            return match e.raw_os_error() {
                Some(libc::ENOENT) => Ok(None),
                Some(libc::ELOOP) => Err(io::Error::other("the snapshot is a symlink, not used")),
                _ => Err(e),
            };
        }
        // SAFETY: fd is new and owned by nobody else.
        let mut f = File::from(unsafe { OwnedFd::from_raw_fd(fd) });
        let md = f.metadata()?;
        use std::os::unix::fs::MetadataExt;
        // SAFETY: geteuid cannot fail.
        if !md.is_file() || md.uid() != unsafe { libc::geteuid() } || md.mode() & 0o022 != 0 {
            return Err(io::Error::other(
                "the snapshot is not a regular file of yours, not used",
            ));
        }
        if md.len() > MAX_FILE {
            return Err(io::Error::other("the snapshot is too large, not used"));
        }
        let mut buf = Vec::with_capacity(md.len() as usize);
        Read::by_ref(&mut f)
            .take(MAX_FILE + 1)
            .read_to_end(&mut buf)?;
        Ok(Some(buf))
    }

    /// Replace the snapshot: write a temp file (0600), sync it, rename it over
    /// the old one, sync the folder. A failure leaves the old snapshot and no
    /// temp file.
    pub fn write(&self, bytes: &[u8]) -> io::Result<()> {
        if bytes.len() as u64 > MAX_FILE {
            return Err(io::Error::other("the index is too large to save"));
        }
        let tmp = cstr(TMP_NAME)?;
        let fin = cstr(FILE_NAME)?;
        let dir = self.fd.as_raw_fd();
        // SAFETY: dir is open; tmp is NUL-terminated. A leftover temp file from
        // a crash is removed; there is one writer per user session.
        unsafe {
            libc::unlinkat(dir, tmp.as_ptr(), 0);
        }
        // SAFETY: as above; O_EXCL|O_NOFOLLOW so nothing else is ever opened.
        let fd = unsafe {
            libc::openat(
                dir,
                tmp.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fd is new and owned by nobody else.
        let mut f = File::from(unsafe { OwnedFd::from_raw_fd(fd) });
        let res = f.write_all(bytes).and_then(|()| f.sync_all());
        drop(f);
        let res = res.and_then(|()| {
            // SAFETY: both names are NUL-terminated and dir is open.
            if unsafe { libc::renameat(dir, tmp.as_ptr(), dir, fin.as_ptr()) } != 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
        if let Err(e) = res {
            // SAFETY: as above.
            unsafe {
                libc::unlinkat(dir, tmp.as_ptr(), 0);
            }
            return Err(e);
        }
        // the files of the version before are of no use any more; a failure to
        // remove them is not a failure to save
        for old in OLD_NAMES {
            if let Ok(c) = cstr(old) {
                // SAFETY: dir is open; c is NUL-terminated.
                unsafe {
                    libc::unlinkat(dir, c.as_ptr(), 0);
                }
            }
        }
        // SAFETY: dir is open. A failed directory sync is not worth failing for:
        // the rename is done, only its durability is in question.
        unsafe {
            libc::fsync(dir);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::testutil::{sample, sample_tagged};
    use crate::testdir::Scratch;
    use std::os::unix::fs::{PermissionsExt, symlink};

    fn none() -> HashMap<u64, i64> {
        HashMap::new()
    }

    fn same(a: &Index, b: &Index) {
        assert_eq!(a.records(), b.records());
        for i in 0..a.len() as u32 {
            assert_eq!(a.name(i), b.name(i));
            assert_eq!(a.fold(i), b.fold(i));
            assert_eq!(a.end(i), b.end(i));
            assert_eq!(a.tags_of(i), b.tags_of(i));
        }
        assert_eq!(a.tag_table(), b.tag_table());
    }

    #[test]
    fn round_trip() {
        let ix = sample();
        let bytes = encode(&ix, 1234);
        let (back, saved) = decode(&bytes, &none()).unwrap();
        assert_eq!(saved, 1234);
        same(&ix, &back);
        // an index with tags too
        let tagged = sample_tagged();
        let (back, _) = decode(&encode(&tagged, 7), &none()).unwrap();
        same(&tagged, &back);
        assert_eq!(back.tags_of(5), "Taxes 2025,RED");
        assert_eq!(back.tag_counts(10), tagged.tag_counts(10));
        // an empty index too
        let e = Index::empty();
        let (b, _) = decode(&encode(&e, 0), &none()).unwrap();
        assert!(b.is_empty());
    }

    #[test]
    fn every_truncation_is_refused() {
        for ix in [sample(), sample_tagged()] {
            let bytes = encode(&ix, 1);
            for cut in 0..bytes.len() {
                assert!(decode(&bytes[..cut], &none()).is_err(), "cut at {cut}");
            }
            let mut longer = bytes.clone();
            longer.push(0);
            assert!(decode(&longer, &none()).is_err());
        }
    }

    #[test]
    fn every_flipped_byte_is_refused() {
        for ix in [sample(), sample_tagged()] {
            let bytes = encode(&ix, 1);
            for i in 0..bytes.len() {
                if (56..64).contains(&i) {
                    continue; // reserved, not covered
                }
                let mut b = bytes.clone();
                b[i] ^= 0x41;
                assert!(decode(&b, &none()).is_err(), "flip at {i}");
            }
        }
    }

    fn reseal(b: &mut [u8]) {
        let body = crc32fast::hash(&b[HEADER..]);
        b[40..44].copy_from_slice(&body.to_le_bytes());
        let head = header_crc(b);
        b[44..48].copy_from_slice(&head.to_le_bytes());
    }

    #[test]
    fn wrong_version_is_named() {
        for v in [1u32, 3, u32::MAX] {
            let mut b = encode(&sample(), 1);
            b[8..12].copy_from_slice(&v.to_le_bytes());
            reseal(&mut b);
            assert_eq!(
                decode(&b, &none()).err(),
                Some(SnapshotError::WrongVersion(v))
            );
            // named even when the checksum does not fit (version before CRC)
            let mut b = encode(&sample(), 1);
            b[8..12].copy_from_slice(&v.to_le_bytes());
            assert_eq!(
                decode(&b, &none()).err(),
                Some(SnapshotError::WrongVersion(v))
            );
        }
    }

    /// A file as version 1 wrote it: 64-byte header, records, arena, no tags.
    fn v1_file(ix: &Index) -> Vec<u8> {
        let mut b = encode(ix, 9);
        b.truncate(HEADER + ix.len() * RECORD + ix.arena().len());
        b[8..12].copy_from_slice(&1u32.to_le_bytes());
        b[48..64].fill(0);
        let body = crc32fast::hash(&b[HEADER..]);
        b[40..44].copy_from_slice(&body.to_le_bytes());
        let head = crc32fast::hash(&b[0..44]);
        b[44..48].copy_from_slice(&head.to_le_bytes());
        b
    }

    #[test]
    fn a_version_1_file_is_named_not_read() {
        let b = v1_file(&sample());
        assert_eq!(
            decode(&b, &none()).err(),
            Some(SnapshotError::WrongVersion(1))
        );
    }

    #[test]
    fn a_snapshot_without_tags_is_as_small_as_before_plus_the_header_fields() {
        let ix = sample();
        assert_eq!(
            encode(&ix, 1).len(),
            HEADER + ix.len() * RECORD + ix.arena().len()
        );
    }

    /// Where the tag entries start in the encoding of `ix`.
    fn tags_at(ix: &Index) -> usize {
        HEADER + ix.len() * RECORD + ix.arena().len()
    }

    #[test]
    fn hostile_tag_sections_with_valid_checksums_are_refused() {
        let ix = sample_tagged();
        let base = encode(&ix, 1);
        assert!(decode(&base, &none()).is_ok());
        let at = tags_at(&ix);
        let text_at = at + ix.tag_table().refs.len() * TAG_ENTRY;
        let bad = |what: &str, edit: &dyn Fn(&mut Vec<u8>)| {
            let mut b = base.clone();
            edit(&mut b);
            reseal(&mut b);
            assert!(decode(&b, &none()).is_err(), "{what}");
        };
        // entry fields (entry 1 is the second tagged record)
        let e1 = at + TAG_ENTRY;
        bad("id past the records", &|b| {
            b[e1..e1 + 4].copy_from_slice(&99u32.to_le_bytes())
        });
        bad("id not ascending", &|b| {
            b[e1..e1 + 4].copy_from_slice(&0u32.to_le_bytes())
        });
        bad("offset outside", &|b| {
            b[e1 + 4..e1 + 8].copy_from_slice(&u32::MAX.to_le_bytes())
        });
        bad("length outside", &|b| {
            b[e1 + 8..e1 + 10].copy_from_slice(&60_000u16.to_le_bytes())
        });
        bad("zero length", &|b| {
            b[e1 + 8..e1 + 10].copy_from_slice(&0u16.to_le_bytes())
        });
        bad("reserved field set", &|b| b[e1 + 10] = 1);
        bad("overlap", &|b| {
            let first = b[at + 4..at + 8].to_vec();
            b[e1 + 4..e1 + 8].copy_from_slice(&first);
        });
        // the tag text
        bad("not UTF-8", &|b| b[text_at] = 0xFF);
        bad("control character in a name", &|b| b[text_at + 1] = 0x01);
        bad("an empty name", &|b| b[text_at + 2] = b',');
        bad("a repeated name", &|b| {
            b[text_at..text_at + 8].copy_from_slice(b"ab,AB,ab");
        });
        // counts that lie
        bad("more entries than records", &|b| {
            b[48..52].copy_from_slice(&99u32.to_le_bytes())
        });
        bad("entry count huge", &|b| {
            b[48..52].copy_from_slice(&u32::MAX.to_le_bytes())
        });
        bad("tag arena length huge", &|b| {
            b[52..56].copy_from_slice(&u32::MAX.to_le_bytes())
        });
        // Under u32, but over what the service's memory allows.
        bad("arena past the memory limit", &|b| {
            b[24..32].copy_from_slice(&(MAX_ARENA as u64 + 1).to_le_bytes())
        });
        bad("tag arena past the memory limit", &|b| {
            b[52..56].copy_from_slice(&(MAX_TAG_ARENA as u32 + 1).to_le_bytes())
        });
        bad("tag arena length short", &|b| {
            let n = le32(b, 52) - 1;
            b[52..56].copy_from_slice(&n.to_le_bytes())
        });
        bad("tag arena length long", &|b| {
            let n = le32(b, 52) + 1;
            b[52..56].copy_from_slice(&n.to_le_bytes())
        });
        bad("entries without text", &|b| {
            b[52..56].copy_from_slice(&0u32.to_le_bytes())
        });
    }

    #[test]
    fn hostile_content_with_valid_checksums_is_refused() {
        let ix = sample();
        let base = encode(&ix, 1);
        // each edit patches a record field, then fixes the checksums
        let edits: &[(&str, usize, &[u8])] = &[
            ("name offset", 4, &u32::MAX.to_le_bytes()),
            ("name length", 12, &60_000u16.to_le_bytes()),
            ("fold offset", 8, &(0x7FFF_0000u32).to_le_bytes()),
            ("flags", 16, &[0xF0]),
            ("category", 17, &[99]),
        ];
        for (what, off, val) in edits {
            for rec in [0usize, 1, 3, 7] {
                let mut b = base.clone();
                let o = HEADER + rec * RECORD + off;
                b[o..o + val.len()].copy_from_slice(val);
                reseal(&mut b);
                assert!(decode(&b, &none()).is_err(), "{what} in record {rec}");
            }
        }
        // counts that lie
        let mut b = base.clone();
        b[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
        reseal(&mut b);
        assert!(decode(&b, &none()).is_err());
        let mut b = base.clone();
        b[24..32].copy_from_slice(&u64::MAX.to_le_bytes());
        reseal(&mut b);
        assert!(decode(&b, &none()).is_err());
        let mut b = base.clone();
        b[16..20].copy_from_slice(&1u32.to_le_bytes());
        reseal(&mut b);
        assert!(decode(&b, &none()).is_err());
        // not an index at all
        assert!(decode(b"hello", &none()).is_err());
        assert!(decode(&[0u8; 4096], &none()).is_err());
    }

    #[test]
    fn write_and_read_back_with_the_right_modes() {
        let t = Scratch::new("snap");
        let cd = CacheDir::open(&t.0, true).unwrap().unwrap();
        assert!(
            CacheDir::open(&t.0.join("nothere"), false)
                .unwrap()
                .is_none()
        );
        assert!(cd.read().unwrap().is_none());
        let bytes = encode(&sample(), 5);
        cd.write(&bytes).unwrap();
        assert_eq!(cd.read().unwrap().unwrap(), bytes);
        let dir = t.0.join("telamon-explorer/index");
        assert_eq!(
            std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(dir.join(FILE_NAME))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert!(!dir.join(TMP_NAME).exists());
        // a stale temp file from a crash does not block a write
        std::fs::write(dir.join(TMP_NAME), b"junk").unwrap();
        cd.write(&bytes).unwrap();
        assert!(!dir.join(TMP_NAME).exists());
    }

    #[test]
    fn a_write_removes_the_files_of_version_1() {
        let t = Scratch::new("snap-v1");
        let cd = CacheDir::open(&t.0, true).unwrap().unwrap();
        let dir = t.0.join("telamon-explorer/index");
        std::fs::write(dir.join("v1.idx"), v1_file(&sample())).unwrap();
        std::fs::write(dir.join("v1.idx.tmp"), b"junk").unwrap();
        // the old file is not read as a snapshot
        assert!(cd.read().unwrap().is_none());
        cd.write(&encode(&sample(), 5)).unwrap();
        assert!(!dir.join("v1.idx").exists());
        assert!(!dir.join("v1.idx.tmp").exists());
        assert!(dir.join(FILE_NAME).exists());
        assert_eq!(FILE_NAME, "v2.idx");
    }

    #[test]
    fn symlinks_are_refused() {
        let t = Scratch::new("snap-link");
        let cd = CacheDir::open(&t.0, true).unwrap().unwrap();
        cd.write(&encode(&sample(), 5)).unwrap();
        let dir = t.0.join("telamon-explorer/index");
        // the file as a symlink
        std::fs::rename(dir.join(FILE_NAME), t.0.join("real")).unwrap();
        symlink(t.0.join("real"), dir.join(FILE_NAME)).unwrap();
        assert!(cd.read().is_err());
        // the folder as a symlink
        let t2 = Scratch::new("snap-link2");
        std::fs::create_dir_all(t2.0.join("telamon-explorer")).unwrap();
        std::fs::create_dir_all(t2.0.join("elsewhere")).unwrap();
        symlink(t2.0.join("elsewhere"), t2.0.join("telamon-explorer/index")).unwrap();
        assert!(CacheDir::open(&t2.0, true).is_err());
        assert!(CacheDir::open(&t2.0, false).is_err());
        // a cache home that is itself a symlink is allowed (a moved ~/.cache)
        let t3 = Scratch::new("snap-link3");
        symlink(&t.0, t3.0.join("cache")).unwrap();
        assert!(
            CacheDir::open(&t3.0.join("cache"), false)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn open_folder_modes() {
        let t = Scratch::new("snap-mode");
        let cd = CacheDir::open(&t.0, true).unwrap().unwrap();
        cd.write(&encode(&sample(), 5)).unwrap();
        let dir = t.0.join("telamon-explorer/index");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o775)).unwrap();
        assert!(
            CacheDir::open(&t.0, false).is_err(),
            "a read refuses a group-writable folder"
        );
        let cd = CacheDir::open(&t.0, true).unwrap().unwrap();
        assert_eq!(
            std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700,
            "a write sets it back"
        );
        cd.write(&encode(&sample(), 5)).unwrap();
        // a file others can write is refused
        std::fs::set_permissions(dir.join(FILE_NAME), std::fs::Permissions::from_mode(0o666))
            .unwrap();
        assert!(cd.read().is_err());
    }
}
