//! C ABI for tags, ratings, permissions, checksums and folder sizes
//! (`atlas_explorer_core::{tags, attrs, perms, checksum, foldersize}`): what
//! the Properties window and the tag menus ask of the core. The rules are in
//! the core; these functions move bytes. All file functions block: C++ calls
//! them from a worker.

use crate::ffi::{bytes, put};
use atlas_explorer_core::attrs::{self, Change, Edit, Key};
use atlas_explorer_core::checksum::{self, Alg};
use atlas_explorer_core::foldersize::{self, Totals};
use atlas_explorer_core::perms::{self, Access, Who};
use atlas_explorer_core::tags;
use atlas_explorer_core::xattr;
use std::ffi::{OsStr, c_void};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

/// An absolute path from bytes; `None` for anything else (a relative path is
/// never opened).
fn abs_path(ptr: *const u8, len: usize) -> Option<PathBuf> {
    // SAFETY: the callers' contracts promise `len` readable bytes.
    let b = unsafe { bytes(ptr, len) };
    (b.first() == Some(&b'/') && !b.contains(&0)).then(|| PathBuf::from(OsStr::from_bytes(b)))
}

fn text_of(ptr: *const u8, len: usize) -> String {
    // SAFETY: the callers' contracts promise `len` readable bytes.
    String::from_utf8_lossy(unsafe { bytes(ptr, len) }).into_owned()
}

/// A flag C++ sets (a `std::atomic<bool>`), read here.
///
/// # Safety
/// `p` is null or points to a live one-byte atomic flag.
unsafe fn flag<'a>(p: *const u8) -> &'a AtomicBool {
    static NEVER: AtomicBool = AtomicBool::new(false);
    if p.is_null() {
        &NEVER
    } else {
        // SAFETY: promised by the caller; AtomicBool has the size of bool.
        unsafe { &*(p as *const AtomicBool) }
    }
}

// ---- Tags ----

/// The tags of a file, one name a line, written to `out`. `*status`: 0 read
/// (also when there are none), 1 the file system keeps no attributes, 2 a link,
/// 3 not readable, 4 the tags are not text (shown, never rewritten).
///
/// # Safety
/// `path` points to `len` readable bytes; `out` to `cap` writable bytes (or
/// null); `status` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_tags_read(
    path: *const u8,
    len: usize,
    out: *mut u8,
    cap: usize,
    status: *mut u32,
) -> usize {
    let (st, text) = match abs_path(path, len) {
        None => (3, String::new()),
        Some(p) => {
            if xattr::is_link(&p) {
                (2, String::new())
            } else {
                match xattr::get(&p, xattr::TAGS) {
                    Ok(None) => (0, String::new()),
                    Ok(Some(raw)) => (
                        if tags::is_clean(&raw) { 0 } else { 4 },
                        tags::parse(&raw).join("\n"),
                    ),
                    Err(xattr::Error::Unsupported) => (1, String::new()),
                    Err(_) => (3, String::new()),
                }
            }
        }
    };
    if !status.is_null() {
        // SAFETY: writable per the contract.
        unsafe { *status = st };
    }
    // SAFETY: forwarded.
    unsafe { put(text.as_bytes(), out, cap) }
}

/// The star rating (0 to 10) of a file; -1 when it can't be read.
///
/// # Safety
/// `path` points to `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_rating_read(path: *const u8, len: usize) -> i32 {
    let Some(p) = abs_path(path, len) else {
        return -1;
    };
    match xattr::get(&p, xattr::RATING) {
        Ok(None) => 0,
        Ok(Some(raw)) => i32::from(tags::rating_parse(&raw).unwrap_or(0)),
        Err(_) => -1,
    }
}

/// `#rrggbb` for a colour tag's name, nothing for other names.
///
/// # Safety
/// `name` points to `len` readable bytes; `out` to `cap` writable bytes (or null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_tags_colour(
    name: *const u8,
    len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    let n = text_of(name, len);
    let hex = tags::colour_of(&n).unwrap_or("");
    // SAFETY: forwarded.
    unsafe { put(hex.as_bytes(), out, cap) }
}

/// The seven colours: lines `Name`, tab, `#rrggbb`.
///
/// # Safety
/// `out` points to `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_tags_colours(out: *mut u8, cap: usize) -> usize {
    let text = tags::COLOURS
        .iter()
        .map(|(n, h)| format!("{n}\t{h}"))
        .collect::<Vec<_>>()
        .join("\n");
    // SAFETY: forwarded.
    unsafe { put(text.as_bytes(), out, cap) }
}

/// A tag name a person typed, made into a tag. Returns the name's length and
/// `*problem` 0; or 0 and `*problem` set (1 empty, 2 comma, 3 control, 4 too
/// long) with the sentence written to `out`.
///
/// # Safety
/// `name` points to `len` readable bytes; `out` to `cap` writable bytes (or
/// null); `problem` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_tags_new_name(
    name: *const u8,
    len: usize,
    out: *mut u8,
    cap: usize,
    problem: *mut u32,
) -> usize {
    let typed = text_of(name, len);
    let (code, text) = match tags::new_name(&typed) {
        Ok(n) => (0, n),
        Err(p) => (
            match p {
                tags::NameProblem::Empty => 1,
                tags::NameProblem::Comma => 2,
                tags::NameProblem::Control => 3,
                tags::NameProblem::TooLong => 4,
            },
            p.text().to_string(),
        ),
    };
    if !problem.is_null() {
        // SAFETY: writable per the contract.
        unsafe { *problem = code };
    }
    // SAFETY: forwarded.
    unsafe { put(text.as_bytes(), out, cap) }
}

/// The tags to list (menu, sidebar): `seen` are names a line each, `indexed`
/// lines `name`, tab, `count`. Result: lines `name`, tab, `count`.
///
/// # Safety
/// The pointers cover their lengths; `out` has `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_tags_in_use(
    seen: *const u8,
    seen_len: usize,
    indexed: *const u8,
    indexed_len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    let seen: Vec<String> = text_of(seen, seen_len)
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();
    let indexed: Vec<(String, u32)> = text_of(indexed, indexed_len)
        .lines()
        .filter_map(|l| {
            let (n, c) = l.split_once('\t')?;
            (!n.is_empty()).then(|| (n.to_string(), c.parse().unwrap_or(0)))
        })
        .collect();
    let text = tags::in_use(&seen, &indexed)
        .iter()
        .map(|(n, c)| format!("{n}\t{c}"))
        .collect::<Vec<_>>()
        .join("\n");
    // SAFETY: forwarded.
    unsafe { put(text.as_bytes(), out, cap) }
}

/// Whether none (0), some (1) or all (2) of the items have the tag. `items`
/// holds each item's tags a line each, items separated by byte 0x1e.
///
/// # Safety
/// The pointers cover their lengths.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_tags_have(
    items: *const u8,
    items_len: usize,
    name: *const u8,
    name_len: usize,
) -> u32 {
    let list = text_of(items, items_len);
    let items: Vec<Vec<String>> = if list.is_empty() {
        Vec::new()
    } else {
        list.split('\u{1e}')
            .map(|i| {
                i.lines()
                    .filter(|l| !l.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .collect()
    };
    match tags::have(&items, &text_of(name, name_len)) {
        tags::Have::None => 0,
        tags::Have::Some => 1,
        tags::Have::All => 2,
    }
}

/// The sentence for what `xattr` could not do, by the codes of
/// `telamon_tags_read`'s status.
///
/// # Safety
/// `out` points to `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_tags_status_text(status: u32, out: *mut u8, cap: usize) -> usize {
    let t = match status {
        1 => xattr::Error::Unsupported.text(),
        2 => xattr::Error::Link.text(),
        4 => "The tags of this item are in a form Files doesn't change.",
        _ => xattr::Error::Denied.text(),
    };
    // SAFETY: forwarded.
    unsafe { put(t.as_bytes(), out, cap) }
}

// ---- Changes ----

/// What a run over items did (`attrs::Outcome`), held for C++.
pub struct TelamonOutcome(attrs::Outcome);

fn changes_text(c: &[Change]) -> Vec<u8> {
    let mut out = Vec::new();
    for ch in c {
        out.extend_from_slice(ch.path.as_os_str().as_bytes());
        out.push(0);
        out.extend_from_slice(ch.key.name().as_bytes());
        out.push(0);
        out.extend_from_slice(ch.before.as_bytes());
        out.push(0);
        out.extend_from_slice(ch.after.as_bytes());
        out.push(0);
    }
    out
}

fn parse_changes(raw: &[u8]) -> Option<Vec<Change>> {
    let mut fields = raw.split(|b| *b == 0);
    let mut out = Vec::new();
    loop {
        let Some(path) = fields.next() else { break };
        if path.is_empty() {
            // The trailing empty piece after the last NUL.
            break;
        }
        let key = std::str::from_utf8(fields.next()?).ok()?;
        let before = std::str::from_utf8(fields.next()?).ok()?;
        let after = std::str::from_utf8(fields.next()?).ok()?;
        if path[0] != b'/' {
            return None;
        }
        out.push(Change {
            path: PathBuf::from(OsStr::from_bytes(path)),
            key: Key::from_name(key)?,
            before: before.to_string(),
            after: after.to_string(),
        });
    }
    Some(out)
}

/// Edits items: `kind` 0 adds and removes tags (`add` and `remove` are names
/// a line each; `clear` first takes all away), 1 sets the rating (`value`
/// 0 to 10), 2 turns permission bits on (`set`) and off (`clear_bits`),
/// 3 the same for a folder and everything in it. `paths` are absolute paths
/// separated by NUL bytes. Returns an outcome to free with
/// `telamon_attrs_free`, or null for arguments that make no sense. `cancel`
/// is null or a flag that stops the run once set.
///
/// # Safety
/// The pointers cover their lengths; `cancel` is null or a live atomic flag.
#[allow(clippy::too_many_arguments)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_attrs_edit(
    kind: u32,
    paths: *const u8,
    paths_len: usize,
    add: *const u8,
    add_len: usize,
    remove: *const u8,
    remove_len: usize,
    clear: bool,
    value: u32,
    set_bits: u32,
    clear_bits: u32,
    cancel: *const u8,
) -> *mut TelamonOutcome {
    // SAFETY: forwarded.
    let raw = unsafe { bytes(paths, paths_len) };
    let list: Vec<PathBuf> = raw
        .split(|b| *b == 0)
        .filter(|p| p.first() == Some(&b'/'))
        .map(|p| PathBuf::from(OsStr::from_bytes(p)))
        .collect();
    if list.is_empty() {
        return std::ptr::null_mut();
    }
    // SAFETY: forwarded.
    let cancel = unsafe { flag(cancel) };
    let names = |p: *const u8, n: usize| -> Vec<String> {
        text_of(p, n)
            .lines()
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect()
    };
    let out = match kind {
        0 => attrs::run_edit(
            &list,
            &Edit::Tags {
                add: names(add, add_len),
                remove: names(remove, remove_len),
                clear,
            },
            cancel,
        ),
        1 if value <= 10 => attrs::run_edit(&list, &Edit::Rating(value as u8), cancel),
        2 => attrs::run_edit(
            &list,
            &Edit::Mode {
                set: set_bits & perms::RWX,
                clear: clear_bits & perms::RWX,
            },
            cancel,
        ),
        3 => {
            let mut all = attrs::Outcome::default();
            for root in &list {
                let o = attrs::run_mode_tree(
                    root,
                    set_bits & perms::RWX,
                    clear_bits & perms::RWX,
                    cancel,
                );
                all.items += o.items;
                all.failed += o.failed;
                all.problem = all.problem.or(o.problem);
                all.overflow |= o.overflow;
                all.cancelled |= o.cancelled;
                if !all.overflow {
                    all.changes.extend(o.changes);
                    if all.changes.len() > attrs::MAX_RECORDED {
                        all.overflow = true;
                        all.changes.clear();
                    }
                }
            }
            all
        }
        _ => return std::ptr::null_mut(),
    };
    Box::into_raw(Box::new(TelamonOutcome(out)))
}

/// Sets values back (`rev`) or makes them again, for the changes of an
/// earlier run (`changes`: the text `telamon_attrs_changes` wrote).
///
/// # Safety
/// As `telamon_attrs_edit`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_attrs_revert(
    changes: *const u8,
    len: usize,
    rev: bool,
    cancel: *const u8,
) -> *mut TelamonOutcome {
    // SAFETY: forwarded.
    let Some(list) = parse_changes(unsafe { bytes(changes, len) }) else {
        return std::ptr::null_mut();
    };
    if list.is_empty() {
        return std::ptr::null_mut();
    }
    // SAFETY: forwarded.
    let out = attrs::run_revert(&list, rev, unsafe { flag(cancel) });
    Box::into_raw(Box::new(TelamonOutcome(out)))
}

/// Counts of a run: items looked at and items that failed; the return value
/// holds flags: 1 more changes than Undo can keep, 2 stopped, 4 the file
/// system can't keep tags.
///
/// # Safety
/// `h` is an outcome; `items` and `failed` are writable (or null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_attrs_summary(
    h: *const TelamonOutcome,
    items: *mut u64,
    failed: *mut u64,
) -> u32 {
    // SAFETY: an outcome from this module, or null.
    let Some(o) = (unsafe { h.as_ref() }) else {
        return 0;
    };
    // SAFETY: writable per the contract.
    unsafe {
        if !items.is_null() {
            *items = o.0.items;
        }
        if !failed.is_null() {
            *failed = o.0.failed;
        }
    }
    u32::from(o.0.overflow)
        | u32::from(o.0.cancelled) << 1
        | u32::from(o.0.problem.as_ref().is_some_and(|p| p.is_unsupported())) << 2
}

/// The first reason an item failed, in words.
///
/// # Safety
/// `h` is an outcome; `out` has `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_attrs_problem(
    h: *const TelamonOutcome,
    out: *mut u8,
    cap: usize,
) -> usize {
    // SAFETY: an outcome from this module, or null.
    let text = unsafe { h.as_ref() }
        .and_then(|o| o.0.problem.as_ref().map(|p| p.text()))
        .unwrap_or_default();
    // SAFETY: forwarded.
    unsafe { put(text.as_bytes(), out, cap) }
}

/// The changes made: for each, path, key, value before and value after, every
/// field ended by a NUL byte.
///
/// # Safety
/// `h` is an outcome; `out` has `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_attrs_changes(
    h: *const TelamonOutcome,
    out: *mut u8,
    cap: usize,
) -> usize {
    // SAFETY: an outcome from this module, or null.
    let text = unsafe { h.as_ref() }
        .map(|o| changes_text(&o.0.changes))
        .unwrap_or_default();
    // SAFETY: forwarded.
    unsafe { put(&text, out, cap) }
}

/// # Safety
/// `h` came from `telamon_attrs_edit` or `telamon_attrs_revert`, once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_attrs_free(h: *mut TelamonOutcome) {
    if !h.is_null() {
        // SAFETY: made by Box::into_raw above and not freed before.
        drop(unsafe { Box::from_raw(h) });
    }
}

/// One value of an item as text: `key` 0 tags, 1 rating, 2 mode (octal).
/// Returns the length; `*ok` is false when it can't be read.
///
/// # Safety
/// `path` points to `len` readable bytes; `out` to `cap` writable bytes (or
/// null); `ok` is writable (or null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_attr_read(
    path: *const u8,
    len: usize,
    key: u32,
    out: *mut u8,
    cap: usize,
    ok: *mut bool,
) -> usize {
    let key = match key {
        0 => Key::Tags,
        1 => Key::Rating,
        _ => Key::Mode,
    };
    let r = abs_path(path, len).map(|p| attrs::read(&p, key));
    if !ok.is_null() {
        // SAFETY: writable per the contract.
        unsafe { *ok = matches!(r, Some(Ok(_))) };
    }
    let text = r.and_then(Result::ok).unwrap_or_default();
    // SAFETY: forwarded.
    unsafe { put(text.as_bytes(), out, cap) }
}

// ---- Permissions ----

/// What one class (0 owner, 1 group, 2 others) may do with a mode: bit 0
/// read, bit 1 write, bit 2 run.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_perm_access(mode: u32, who: u32) -> u32 {
    let Some(w) = Who::from_index(who) else {
        return 0;
    };
    let a = perms::access(mode, w);
    u32::from(a.read) | u32::from(a.write) << 1 | u32::from(a.run) << 2
}

/// The mode with one class set to `bits` (as `telamon_perm_access` gives).
#[unsafe(no_mangle)]
pub extern "C" fn telamon_perm_with(mode: u32, who: u32, bits: u32) -> u32 {
    let Some(w) = Who::from_index(who) else {
        return mode;
    };
    perms::with_access(
        mode,
        w,
        Access {
            read: bits & 1 != 0,
            write: bits & 2 != 0,
            run: bits & 4 != 0,
        },
    )
}

/// The bits to turn on and off to go from one mode to another.
///
/// # Safety
/// `set` and `clear` are writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_perm_difference(
    before: u32,
    after: u32,
    set: *mut u32,
    clear: *mut u32,
) {
    let (s, c) = perms::difference(before, after);
    // SAFETY: writable per the contract.
    unsafe {
        if !set.is_null() {
            *set = s;
        }
        if !clear.is_null() {
            *clear = c;
        }
    }
}

/// Texts: `which` 0 what one class (`who`) may do, in words; 1 the sentence
/// for a whole mode (`who` 1: the owner is the person using Files); 2 the
/// mode in octal; 3 as `rwxr-xr--`.
///
/// # Safety
/// `out` points to `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_perm_text(
    which: u32,
    mode: u32,
    who: u32,
    is_dir: bool,
    out: *mut u8,
    cap: usize,
) -> usize {
    let text = match which {
        0 => Who::from_index(who)
            .map(|w| perms::words(perms::access(mode, w), is_dir))
            .unwrap_or_default(),
        1 => perms::sentence(mode, is_dir, who == 1),
        2 => perms::octal(mode),
        _ => perms::symbolic(mode),
    };
    // SAFETY: forwarded.
    unsafe { put(text.as_bytes(), out, cap) }
}

/// The user id of this process.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_current_uid() -> u32 {
    attrs::current_uid()
}

// ---- Checksums ----

/// Called with the bytes read so far, on the worker's thread.
pub type SumProgress = Option<extern "C" fn(*mut c_void, u64)>;

/// The checksum of a file (`alg`: 0 SHA-256, 1 SHA-1, 2 MD5, 3 SHA-512).
/// Returns 0 with the hex digits in `out`, 1 when `cancel` stopped it, 2 with
/// a sentence in `out`. `*len` is the length written.
///
/// # Safety
/// `path` points to `path_len` readable bytes; `cancel` is null or a live
/// atomic flag; `progress` (when set) may be called with `user`; `out` has
/// `cap` writable bytes; `len` is writable.
#[allow(clippy::too_many_arguments)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_sum_file(
    path: *const u8,
    path_len: usize,
    alg: u32,
    cancel: *const u8,
    progress: SumProgress,
    user: *mut c_void,
    out: *mut u8,
    cap: usize,
    len: *mut usize,
) -> i32 {
    let (rc, text) = match (abs_path(path, path_len), Alg::from_index(alg)) {
        (Some(p), Some(a)) => {
            let mut cb = |n: u64| {
                if let Some(f) = progress {
                    f(user, n);
                }
            };
            // SAFETY: forwarded.
            match checksum::hash_file(&p, a, unsafe { flag(cancel) }, &mut cb) {
                Ok(hex) => (0, hex),
                Err(checksum::FileError::Cancelled) => (1, String::new()),
                Err(e) => (2, e.text()),
            }
        }
        _ => (2, "The file couldn't be read.".to_string()),
    };
    if !len.is_null() {
        // SAFETY: writable per the contract; forwarded `put`.
        unsafe { *len = put(text.as_bytes(), out, cap) };
    }
    rc
}

/// What a person pasted, read as a checksum: the kind (as `alg` numbers) in
/// `*alg` and the lowercase hex in `out`; length 0 when it is none.
///
/// # Safety
/// `text` points to `len` readable bytes; `out` to `cap` writable bytes (or
/// null); `alg` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_sum_expected(
    text: *const u8,
    len: usize,
    alg: *mut u32,
    out: *mut u8,
    cap: usize,
) -> usize {
    let Some((a, hex)) = checksum::parse_expected(&text_of(text, len)) else {
        return 0;
    };
    if !alg.is_null() {
        // SAFETY: writable per the contract.
        unsafe { *alg = a.index() };
    }
    // SAFETY: forwarded.
    unsafe { put(hex.as_bytes(), out, cap) }
}

/// The name of a kind of checksum ("SHA-256").
///
/// # Safety
/// `out` points to `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_sum_name(alg: u32, out: *mut u8, cap: usize) -> usize {
    let n = Alg::from_index(alg).map(Alg::name).unwrap_or("");
    // SAFETY: forwarded.
    unsafe { put(n.as_bytes(), out, cap) }
}

// ---- Folder size ----

/// Totals as C++ reads them.
#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct TelamonTotals {
    pub files: u64,
    pub folders: u64,
    pub bytes: u64,
    pub on_disk: u64,
    pub unreadable: u64,
    pub skipped: u64,
}

impl From<&Totals> for TelamonTotals {
    fn from(t: &Totals) -> Self {
        TelamonTotals {
            files: t.files,
            folders: t.folders,
            bytes: t.bytes,
            on_disk: t.on_disk,
            unreadable: t.unreadable,
            skipped: t.skipped,
        }
    }
}

pub type SizeProgress = Option<extern "C" fn(*mut c_void, *const TelamonTotals)>;

/// Counts a file or folder on this computer. Returns 0 when done, 1 when
/// `cancel` stopped it (`totals` then holds what was counted), 2 for a path
/// that can't be used.
///
/// # Safety
/// `path` points to `len` readable bytes; `cancel` is null or a live atomic
/// flag; `progress` may be called with `user`; `totals` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_foldersize(
    path: *const u8,
    len: usize,
    cancel: *const u8,
    progress: SizeProgress,
    user: *mut c_void,
    totals: *mut TelamonTotals,
) -> i32 {
    let Some(p) = abs_path(path, len) else {
        return 2;
    };
    let mut cb = |t: &Totals| {
        if let Some(f) = progress {
            let c = TelamonTotals::from(t);
            f(user, &c);
        }
    };
    // SAFETY: forwarded.
    let r = foldersize::measure(Path::new(&p), unsafe { flag(cancel) }, &mut cb);
    let (rc, t) = match r {
        Ok(t) => (0, t),
        Err(t) => (1, t),
    };
    if !totals.is_null() {
        // SAFETY: writable per the contract.
        unsafe { *totals = TelamonTotals::from(&t) };
    }
    rc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(mut f: impl FnMut(*mut u8, usize) -> usize) -> String {
        let mut buf = vec![0u8; 4096];
        let n = f(buf.as_mut_ptr(), buf.len());
        String::from_utf8_lossy(&buf[..n.min(4096)]).into_owned()
    }

    #[test]
    fn colours_and_names() {
        let t = read(|o, c| unsafe { telamon_tags_colours(o, c) });
        assert_eq!(t.lines().count(), 7);
        assert!(t.starts_with("Red\t#"));
        let hex = read(|o, c| unsafe { telamon_tags_colour(b"green".as_ptr(), 5, o, c) });
        assert!(hex.starts_with('#'));
        assert_eq!(
            read(|o, c| unsafe { telamon_tags_colour(b"Work".as_ptr(), 4, o, c) }),
            ""
        );
        let mut problem = 9;
        let n =
            read(|o, c| unsafe { telamon_tags_new_name(b" red ".as_ptr(), 5, o, c, &mut problem) });
        assert_eq!((n.as_str(), problem), ("Red", 0));
        let t =
            read(|o, c| unsafe { telamon_tags_new_name(b"a,b".as_ptr(), 3, o, c, &mut problem) });
        assert_eq!(problem, 2);
        assert!(t.contains("comma"));
    }

    #[test]
    fn have_and_in_use() {
        let items = "Red\nWork\u{1e}Red";
        let name = b"red";
        assert_eq!(
            unsafe { telamon_tags_have(items.as_ptr(), items.len(), name.as_ptr(), 3) },
            2
        );
        let name = b"work";
        assert_eq!(
            unsafe { telamon_tags_have(items.as_ptr(), items.len(), name.as_ptr(), 4) },
            1
        );
        assert_eq!(
            unsafe { telamon_tags_have(std::ptr::null(), 0, name.as_ptr(), 4) },
            0
        );
        let seen = "zed";
        let indexed = "Work\t3\nred\t2";
        let t = read(|o, c| unsafe {
            telamon_tags_in_use(
                seen.as_ptr(),
                seen.len(),
                indexed.as_ptr(),
                indexed.len(),
                o,
                c,
            )
        });
        assert_eq!(t, "Red\t2\nWork\t3\nzed\t0");
    }

    #[test]
    fn edits_round_trip_through_the_abi() {
        let dir = std::env::temp_dir().join(format!("telamon-propsffi-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("a.txt");
        std::fs::write(&f, b"x").unwrap();
        let mut raw = f.as_os_str().as_bytes().to_vec();
        raw.push(0);
        let add = b"Red\nWork";
        let h = unsafe {
            telamon_attrs_edit(
                0,
                raw.as_ptr(),
                raw.len(),
                add.as_ptr(),
                add.len(),
                std::ptr::null(),
                0,
                false,
                0,
                0,
                0,
                std::ptr::null(),
            )
        };
        assert!(!h.is_null());
        let (mut items, mut failed) = (0, 0);
        let flags = unsafe { telamon_attrs_summary(h, &mut items, &mut failed) };
        if failed > 0 && flags & 4 != 0 {
            eprintln!("skipped: no user attributes here");
            unsafe { telamon_attrs_free(h) };
            return;
        }
        assert_eq!((items, failed, flags), (1, 0, 0));
        let changes = read(|o, c| unsafe { telamon_attrs_changes(h, o, c) });
        assert!(changes.contains("tags\0\0Red,Work\0"));
        let mut st = 9;
        let t =
            read(|o, c| unsafe { telamon_tags_read(raw.as_ptr(), raw.len() - 1, o, c, &mut st) });
        assert_eq!((t.as_str(), st), ("Red\nWork", 0));
        // Undo with the text the outcome gave.
        let bytes_ = unsafe {
            let n = telamon_attrs_changes(h, std::ptr::null_mut(), 0);
            let mut v = vec![0u8; n];
            telamon_attrs_changes(h, v.as_mut_ptr(), n);
            v
        };
        unsafe { telamon_attrs_free(h) };
        let h2 =
            unsafe { telamon_attrs_revert(bytes_.as_ptr(), bytes_.len(), true, std::ptr::null()) };
        assert!(!h2.is_null());
        assert_eq!(
            unsafe { telamon_attrs_summary(h2, &mut items, &mut failed) },
            0
        );
        assert_eq!(failed, 0);
        unsafe { telamon_attrs_free(h2) };
        let t =
            read(|o, c| unsafe { telamon_tags_read(raw.as_ptr(), raw.len() - 1, o, c, &mut st) });
        assert_eq!(t, "");
        // Bad arguments make no outcome.
        assert!(
            unsafe { telamon_attrs_revert(b"junk".as_ptr(), 4, true, std::ptr::null()) }.is_null()
        );
        assert!(
            unsafe {
                telamon_attrs_edit(
                    9,
                    raw.as_ptr(),
                    raw.len(),
                    std::ptr::null(),
                    0,
                    std::ptr::null(),
                    0,
                    false,
                    0,
                    0,
                    0,
                    std::ptr::null(),
                )
            }
            .is_null()
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn checksums_and_sizes_through_the_abi() {
        let dir = std::env::temp_dir().join(format!("telamon-sumffi-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("abc");
        std::fs::write(&f, b"abc").unwrap();
        let p = f.as_os_str().as_bytes();
        let mut len = 0usize;
        let mut out = vec![0u8; 256];
        let rc = unsafe {
            telamon_sum_file(
                p.as_ptr(),
                p.len(),
                0,
                std::ptr::null(),
                None,
                std::ptr::null_mut(),
                out.as_mut_ptr(),
                out.len(),
                &mut len,
            )
        };
        assert_eq!(rc, 0);
        assert!(String::from_utf8_lossy(&out[..len]).starts_with("ba7816bf"));
        let stop = AtomicBool::new(true);
        let rc = unsafe {
            telamon_sum_file(
                p.as_ptr(),
                p.len(),
                0,
                &stop as *const _ as *const u8,
                None,
                std::ptr::null_mut(),
                out.as_mut_ptr(),
                out.len(),
                &mut len,
            )
        };
        assert_eq!(rc, 1);
        let rc = unsafe {
            telamon_sum_file(
                b"rel".as_ptr(),
                3,
                0,
                std::ptr::null(),
                None,
                std::ptr::null_mut(),
                out.as_mut_ptr(),
                out.len(),
                &mut len,
            )
        };
        assert_eq!(rc, 2);
        let mut alg = 9;
        let t = read(|o, c| unsafe {
            telamon_sum_expected(
                b"SHA256:BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD".as_ptr(),
                71,
                &mut alg,
                o,
                c,
            )
        });
        assert_eq!((t.len(), alg), (64, 0));
        assert_eq!(read(|o, c| unsafe { telamon_sum_name(3, o, c) }), "SHA-512");
        let mut totals = TelamonTotals::default();
        let rc = unsafe {
            let d = dir.as_os_str().as_bytes();
            telamon_foldersize(
                d.as_ptr(),
                d.len(),
                std::ptr::null(),
                None,
                std::ptr::null_mut(),
                &mut totals,
            )
        };
        assert_eq!((rc, totals.files, totals.bytes), (0, 1, 3));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn permissions_through_the_abi() {
        assert_eq!(telamon_perm_access(0o640, 0), 3);
        assert_eq!(telamon_perm_access(0o640, 1), 1);
        assert_eq!(telamon_perm_with(0o640, 1, 3), 0o660);
        assert_eq!(telamon_perm_with(0o640, 9, 3), 0o640);
        let (mut s, mut c) = (0, 0);
        unsafe { telamon_perm_difference(0o755, 0o750, &mut s, &mut c) };
        assert_eq!((s, c), (0, 0o005));
        assert_eq!(
            read(|o, n| unsafe { telamon_perm_text(2, 0o644, 0, false, o, n) }),
            "644"
        );
        assert_eq!(
            read(|o, n| unsafe { telamon_perm_text(0, 0o644, 1, false, o, n) }),
            "Read only"
        );
        assert!(telamon_current_uid() < u32::MAX);
    }
}
