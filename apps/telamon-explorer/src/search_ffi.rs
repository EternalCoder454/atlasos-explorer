//! C ABI for the search field (`cpp/kio/SearchController.*`): what the filter
//! chips mean, where a search runs, the texts, and the live walk of folders
//! the index does not hold. The decisions are in `atlas_explorer_core::search`
//! and `atlas_file_index::walk`; these functions only move bytes.

use crate::ffi::{bytes, put};
use atlas_explorer_core::content::{self, ContentQuery, Ending, Stats};
use atlas_explorer_core::pattern::{self, Pattern};
use atlas_explorer_core::search::{
    self, IndexState, Kind, MAX_HITS, MAX_LIVE_HITS, Modified, Scope, SizeClass,
};
use atlas_file_index::category::Category;
use atlas_file_index::namefilter::NameFilter;
use atlas_file_index::query::{KindFilter, Options};
use atlas_file_index::uri::path_to_uri;
use atlas_file_index::walk::{
    ContentHit, LiveMatcher, WalkEnd, WalkHit, covers, walk, walk_content,
};
use std::ffi::c_void;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// The chips as numbers the walker and the D-Bus options share. The C++ twin
/// is `TelamonSearchFilter` in RustBridge.h.
#[repr(C)]
pub struct TelamonSearchFilter {
    /// Bit mask of the index's categories; 0 for any.
    pub kinds_mask: u32,
    /// Only files (a size says nothing of a folder).
    pub files_only: bool,
    pub has_after: bool,
    pub after: i64,
    pub has_min: bool,
    pub min: u64,
    pub has_max: bool,
    pub max: u64,
}

/// Fills `out` for the chips: `kind` (0 any, 1 document, 2 image, 3 audio,
/// 4 video, 5 archive, 6 code, 7 folder), `modified` (0 any, 1 today, 2 past 7
/// days, 3 past month, 4 past year) and `size` (0 any, 1 small, 2 medium,
/// 3 large). `now` and `start_of_today` are seconds since the epoch.
///
/// # Safety
/// `out` points to a writable `TelamonSearchFilter`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_search_filter(
    kind: u32,
    modified: u32,
    size: u32,
    now: i64,
    start_of_today: i64,
    out: *mut TelamonSearchFilter,
) {
    if out.is_null() {
        return;
    }
    let f = search::filter(
        Kind::from_u32(kind),
        Modified::from_u32(modified),
        SizeClass::from_u32(size),
        now,
        start_of_today,
    );
    let mask = f
        .kinds
        .iter()
        .filter_map(|n| Category::from_name(n))
        .fold(0u32, |m, c| m | c.bit());
    let v = TelamonSearchFilter {
        kinds_mask: mask,
        files_only: f.files_only,
        has_after: f.modified_after.is_some(),
        after: f.modified_after.unwrap_or(0),
        has_min: f.size_min.is_some(),
        min: f.size_min.unwrap_or(0),
        has_max: f.size_max.is_some(),
        max: f.size_max.unwrap_or(0),
    };
    // SAFETY: `out` is writable (contract).
    unsafe { out.write(v) };
}

/// The `kinds` option for a kind chip: the index's category names, one per line.
///
/// # Safety
/// `out` points to `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_search_kinds(kind: u32, out: *mut u8, cap: usize) -> usize {
    let names = Kind::from_u32(kind).categories().join("\n");
    // SAFETY: `out` as promised.
    unsafe { put(names.as_bytes(), out, cap) }
}

/// Where a search runs: 0 the index, everywhere; 1 the index, below the
/// folder; 2 a walk of the folder; 3 a walk of the home folder; 4 a walk by
/// KIO. See `search::route`.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_search_route(
    scope: u32,
    local: bool,
    indexed: bool,
    index_on: bool,
) -> u32 {
    search::route(Scope::from_u32(scope), local, indexed, index_on) as u32
}

/// Where a search runs when it must walk (a name pattern, or words inside
/// files): 2 a walk of the folder, 3 a walk of the home folder, 4 by KIO.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_search_route_live(scope: u32, local: bool) -> u32 {
    search::route_live(Scope::from_u32(scope), local) as u32
}

/// The index state for the service's `state` text (or the number 5 for "the
/// service did not answer" is `telamon_search_chip`'s `unavailable`):
/// 0 unknown, 1 ready, 2 updating, 3 off, 4 problem.
///
/// # Safety
/// `state` points to `len` readable bytes (or is null with `len` 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_search_index_state(state: *const u8, len: usize) -> u32 {
    // SAFETY: forwarded from this function's contract.
    let s = String::from_utf8_lossy(unsafe { bytes(state, len) }).into_owned();
    match IndexState::from_status(&s) {
        IndexState::Unknown => 0,
        IndexState::Ready => 1,
        IndexState::Updating => 2,
        IndexState::Off => 3,
        IndexState::Problem => 4,
        IndexState::Unavailable => 5,
    }
}

/// Is there an index to ask in this state (0 to 5 as above)? Off is not.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_search_index_on(state: u32) -> bool {
    state_of(state).is_on()
}

fn state_of(n: u32) -> IndexState {
    match n {
        1 => IndexState::Ready,
        2 => IndexState::Updating,
        3 => IndexState::Off,
        4 => IndexState::Problem,
        5 => IndexState::Unavailable,
        _ => IndexState::Unknown,
    }
}

/// The status chip for a state: the text, and in `*level` how it is drawn
/// (0 not at all, 1 good, 2 warning, 3 error). `error` is the service's reason.
///
/// # Safety
/// `error` points to `error_len` readable bytes (or is null with 0); `out`
/// to `cap` writable bytes (or is null); `level` to a writable u32.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_search_chip(
    state: u32,
    error: *const u8,
    error_len: usize,
    out: *mut u8,
    cap: usize,
    level: *mut u32,
) -> usize {
    // SAFETY: forwarded from this function's contract.
    let err = String::from_utf8_lossy(unsafe { bytes(error, error_len) }).into_owned();
    let (lv, text) = state_of(state).chip(&err);
    if !level.is_null() {
        // SAFETY: `level` is writable (contract).
        unsafe { level.write(lv) };
    }
    // SAFETY: `out` as promised.
    unsafe { put(text.as_bytes(), out, cap) }
}

/// Texts of the search: `which` 0 the count line (`n` results; `flags` 1 when
/// the list was cut), 1 the live line (`flags` 1 running, 2 stopped, 4 cut),
/// 2 the title shown when search can't be reached, 3 its text, 4 the same as
/// a line.
///
/// # Safety
/// `out` points to `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_search_text(
    which: u32,
    n: u64,
    flags: u32,
    out: *mut u8,
    cap: usize,
) -> usize {
    let n = usize::try_from(n).unwrap_or(usize::MAX);
    let text = match which {
        0 => search::count_text(n, flags & 1 != 0),
        1 => search::live_text(n, flags & 1 != 0, flags & 2 != 0, flags & 4 != 0),
        2 => search::UNAVAILABLE_TITLE.to_string(),
        3 => search::UNAVAILABLE_TEXT.to_string(),
        4 => search::UNAVAILABLE_LINE.to_string(),
        _ => String::new(),
    };
    // SAFETY: `out` as promised.
    unsafe { put(text.as_bytes(), out, cap) }
}

/// The limits: 0 hits one index search returns, 1 hits a walk stops at, 2 the
/// smallest "Medium" file in bytes, 3 the smallest "Large" one, 4 files a
/// search inside files lists, 5 the biggest text file it reads, 6 the longest
/// pattern, in bytes.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_search_limit(which: u32) -> usize {
    match which {
        0 => MAX_HITS,
        2 => search::SMALL_LIMIT as usize,
        3 => search::LARGE_LIMIT as usize,
        4 => content::MAX_CONTENT_HITS,
        5 => content::MAX_TEXT_BYTES as usize,
        6 => pattern::MAX_PATTERN_BYTES,
        _ => MAX_LIVE_HITS,
    }
}

/// How a result's folder reads in the Path column.
///
/// # Safety
/// Each pointer pair covers its length (or is null with length 0); `out`
/// points to `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_search_path_text(
    parent: *const u8,
    parent_len: usize,
    home: *const u8,
    home_len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    // SAFETY: forwarded from this function's contract.
    let (p, h) = unsafe {
        (
            String::from_utf8_lossy(bytes(parent, parent_len)).into_owned(),
            String::from_utf8_lossy(bytes(home, home_len)).into_owned(),
        )
    };
    let t = search::path_text(&p, &h);
    // SAFETY: `out` as promised.
    unsafe { put(t.as_bytes(), out, cap) }
}

/// Does the index hold this folder? `roots` is the indexed folders' paths,
/// one per line. Reads a few folders: call it from a worker.
///
/// # Safety
/// Each pointer pair covers its length (or is null with length 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_search_covers(
    folder: *const u8,
    folder_len: usize,
    roots: *const u8,
    roots_len: usize,
) -> bool {
    // SAFETY: forwarded from this function's contract.
    let (folder, roots) = unsafe { (bytes(folder, folder_len), bytes(roots, roots_len)) };
    let roots: Vec<PathBuf> = roots
        .split(|&b| b == b'\n')
        .filter(|r| !r.is_empty())
        .map(|r| PathBuf::from(std::ffi::OsStr::from_bytes(r)))
        .collect();
    covers(Path::new(std::ffi::OsStr::from_bytes(folder)), &roots)
}

fn options_of(f: &TelamonSearchFilter, include_hidden: bool, tag: Option<String>) -> Options {
    Options {
        tag,
        kind: f.files_only.then_some(KindFilter::File),
        kinds: (f.kinds_mask != 0).then_some(f.kinds_mask),
        include_hidden,
        modified_after: f.has_after.then_some(f.after),
        size_min: f.has_min.then_some(f.min),
        size_max: f.has_max.then_some(f.max),
        ..Options::default()
    }
}

fn matcher_of(
    query: *const u8,
    query_len: usize,
    filter: *const TelamonSearchFilter,
    include_hidden: bool,
    tag: Option<String>,
    use_pattern: bool,
) -> Option<LiveMatcher> {
    if filter.is_null() {
        return None;
    }
    // SAFETY: the caller's contract: `query` covers `query_len`, `filter` is valid.
    let (q, f) = unsafe {
        (
            String::from_utf8_lossy(bytes(query, query_len)).into_owned(),
            &*filter,
        )
    };
    let opts = options_of(f, include_hidden, tag);
    if use_pattern && !q.trim().is_empty() {
        // An invalid pattern is no matcher (the window has said why already).
        Pattern::new(q.trim())
            .ok()
            .map(|p| LiveMatcher::with_pattern(p, opts))
    } else {
        Some(LiveMatcher::new(&q, opts))
    }
}

/// A matcher for entries a KIO listing delivers one at a time. Free it with
/// `telamon_matcher_free`. Null when `filter` is null, or when `use_pattern`
/// is set and the query is not a valid pattern.
///
/// # Safety
/// `query` covers `query_len` readable bytes (or is null with 0); `filter`
/// points to a valid filter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_matcher_new(
    query: *const u8,
    query_len: usize,
    filter: *const TelamonSearchFilter,
    include_hidden: bool,
    use_pattern: bool,
) -> *mut c_void {
    matcher_of(query, query_len, filter, include_hidden, None, use_pattern)
        .map_or(std::ptr::null_mut(), |m| Box::into_raw(Box::new(m)).cast())
}

/// How well an entry matches, 1 to 5, or 0 for not at all. `name` is the
/// entry's name (not its path).
///
/// # Safety
/// `matcher` comes from `telamon_matcher_new` and is not freed; `name`
/// covers `name_len` readable bytes (or is null with 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_matcher_test(
    matcher: *const c_void,
    name: *const u8,
    name_len: usize,
    is_dir: bool,
    size: u64,
    mtime: i64,
) -> u32 {
    if matcher.is_null() {
        return 0;
    }
    // SAFETY: `matcher` is a live LiveMatcher (contract); `name` as promised.
    let (m, name) = unsafe { (&*matcher.cast::<LiveMatcher>(), bytes(name, name_len)) };
    if m.hides(name) {
        return 0;
    }
    m.test(name, is_dir, false, size, mtime)
        .map_or(0, u32::from)
}

/// # Safety
/// `matcher` is from `telamon_matcher_new` (or null) and is not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_matcher_free(matcher: *mut c_void) {
    if !matcher.is_null() {
        // SAFETY: created by Box::into_raw in telamon_matcher_new.
        drop(unsafe { Box::from_raw(matcher.cast::<LiveMatcher>()) });
    }
}

/// Called from the walk's thread with a batch of hits (see [`encode`]) and
/// `end` 0, or once with no hits and `end` 1 done, 2 stopped, 3 cut at the
/// limit, 4 the folder could not be read. After the last call the walk never
/// touches `user` again.
pub type WalkCallback = extern "C" fn(user: *mut c_void, batch: *const u8, len: usize, end: u32);

struct Walking {
    stop: Arc<AtomicBool>,
}

/// One hit as a record: is_dir (1 byte), match class (1), size (u64), mtime
/// (i64), URI length (u32), the URI; numbers little-endian.
fn encode(hits: &[WalkHit], out: &mut Vec<u8>) {
    for h in hits {
        let uri = path_to_uri(&h.path);
        out.push(u8::from(h.is_dir));
        out.push(h.class);
        out.extend_from_slice(&h.size.to_le_bytes());
        out.extend_from_slice(&h.mtime.to_le_bytes());
        out.extend_from_slice(&(uri.len() as u32).to_le_bytes());
        out.extend_from_slice(uri.as_bytes());
    }
}

struct UserPtr(*mut c_void);
// SAFETY: the pointer is only handed back to the callback, which is written
// to be called from any thread.
unsafe impl Send for UserPtr {}

/// Starts a walk of `root` on its own thread; hits arrive through `callback`.
/// `max_hits` is capped at the core's limit. Returns a handle for
/// `telamon_walk_stop` and `telamon_walk_free`, or null when the thread
/// could not start (the callback is then not called).
///
/// # Safety
/// `root`, `query` and `tag` cover their lengths; `filter` is valid; `user` stays
/// valid until the callback has been called with a non-zero `end`.
#[allow(clippy::too_many_arguments)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_walk_start(
    root: *const u8,
    root_len: usize,
    query: *const u8,
    query_len: usize,
    filter: *const TelamonSearchFilter,
    include_hidden: bool,
    use_pattern: bool,
    tag: *const u8,
    tag_len: usize,
    max_hits: usize,
    callback: WalkCallback,
    user: *mut c_void,
) -> *mut c_void {
    // SAFETY: `tag` covers `tag_len` (contract).
    let tag = String::from_utf8_lossy(unsafe { bytes(tag, tag_len) }).into_owned();
    let tag = (!tag.is_empty()).then_some(tag);
    let Some(matcher) = matcher_of(query, query_len, filter, include_hidden, tag, use_pattern)
    else {
        return std::ptr::null_mut();
    };
    // SAFETY: `root` covers `root_len` (contract).
    let root = PathBuf::from(std::ffi::OsStr::from_bytes(unsafe {
        bytes(root, root_len)
    }));
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let user = UserPtr(user);
    let max = max_hits.clamp(1, MAX_LIVE_HITS);
    // For the smoke tests: a pause after every batch, so a walk lasts long
    // enough to look at and to stop. Test builds only (debug); a release
    // build does not read the variable.
    #[cfg(debug_assertions)]
    let pause = std::env::var("TELAMON_EXPLORER_TEST_WALK_BATCH_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(|ms| std::time::Duration::from_millis(ms.min(10_000)));
    let spawned = std::thread::Builder::new()
        .name("search-walk".into())
        .spawn(move || {
            let user = user;
            let mut buf: Vec<u8> = Vec::new();
            let end = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                walk(&root, &matcher, &flag, max, &mut |hits| {
                    buf.clear();
                    encode(&hits, &mut buf);
                    callback(user.0, buf.as_ptr(), buf.len(), 0);
                    #[cfg(debug_assertions)]
                    if let Some(p) = pause {
                        std::thread::sleep(p);
                    }
                })
            }))
            .unwrap_or(WalkEnd::Unreadable);
            let code = match end {
                WalkEnd::Done => 1,
                WalkEnd::Stopped => 2,
                WalkEnd::Capped => 3,
                WalkEnd::Unreadable => 4,
                WalkEnd::Limit => 5,
            };
            callback(user.0, std::ptr::null(), 0, code);
        });
    if spawned.is_err() {
        return std::ptr::null_mut();
    }
    Box::into_raw(Box::new(Walking { stop })).cast()
}

/// Asks a walk to end; its callback then gets `end` 2 soon after. Safe to
/// call on a walk that has already ended.
///
/// # Safety
/// `handle` is from `telamon_walk_start` and not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_walk_stop(handle: *mut c_void) {
    if !handle.is_null() {
        // SAFETY: a live Walking (contract).
        unsafe { &*handle.cast::<Walking>() }
            .stop
            .store(true, Ordering::Relaxed);
    }
}

/// Releases the handle. The walk goes on to its end unless stopped first.
///
/// # Safety
/// `handle` is from `telamon_walk_start` (or null) and is not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_walk_free(handle: *mut c_void) {
    if !handle.is_null() {
        // SAFETY: created by Box::into_raw in telamon_walk_start.
        drop(unsafe { Box::from_raw(handle.cast::<Walking>()) });
    }
}

// ---- Patterns and the folder filter (Ctrl+F) ----

/// Checks the text of a search or filter. With `use_pattern` it must be a
/// valid regular expression; with `contents` and no pattern it must be short
/// enough to look for. Returns the length of the reason (written to `out` if
/// it fits), or 0 when the text is fine (or empty).
///
/// # Safety
/// `text` covers `len` readable bytes (or is null with 0); `out` points to
/// `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_pattern_check(
    text: *const u8,
    len: usize,
    use_pattern: bool,
    contents: bool,
    out: *mut u8,
    cap: usize,
) -> usize {
    // SAFETY: forwarded from this function's contract.
    let t = String::from_utf8_lossy(unsafe { bytes(text, len) }).into_owned();
    let t = t.trim();
    let err = if t.is_empty() {
        None
    } else if use_pattern {
        pattern::check(t)
    } else if contents {
        Pattern::literal(t).err()
    } else {
        None
    };
    // SAFETY: `out` as promised.
    err.map_or(0, |e| unsafe { put(e.0.as_bytes(), out, cap) })
}

/// The matcher of the folder filter, for the text typed. Null when the text
/// is an invalid pattern (the reason is written to `err`, its length to
/// `*err_len`). Free it with `telamon_namefilter_free`.
///
/// # Safety
/// `text` covers `len` readable bytes (or is null with 0); `err` points to
/// `err_cap` writable bytes (or is null); `err_len` is writable or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_namefilter_new(
    text: *const u8,
    len: usize,
    use_pattern: bool,
    err: *mut u8,
    err_cap: usize,
    err_len: *mut usize,
) -> *mut c_void {
    // SAFETY: forwarded from this function's contract.
    let t = String::from_utf8_lossy(unsafe { bytes(text, len) }).into_owned();
    match NameFilter::new(&t, use_pattern) {
        Ok(f) => {
            if !err_len.is_null() {
                // SAFETY: `err_len` is writable (contract).
                unsafe { err_len.write(0) };
            }
            Box::into_raw(Box::new(f)).cast()
        }
        Err(e) => {
            // SAFETY: `err` as promised.
            let n = unsafe { put(e.0.as_bytes(), err, err_cap) };
            if !err_len.is_null() {
                // SAFETY: `err_len` is writable (contract).
                unsafe { err_len.write(n) };
            }
            std::ptr::null_mut()
        }
    }
}

/// Does everything stay (nothing is typed)?
///
/// # Safety
/// `filter` is from `telamon_namefilter_new` and not freed (or null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_namefilter_is_all(filter: *const c_void) -> bool {
    // SAFETY: a live NameFilter (contract).
    filter.is_null() || unsafe { &*filter.cast::<NameFilter>() }.is_all()
}

/// Does the name stay? (A null filter keeps everything.)
///
/// # Safety
/// `filter` is from `telamon_namefilter_new` and not freed (or null); `name`
/// covers `len` readable bytes (or is null with 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_namefilter_test(
    filter: *const c_void,
    name: *const u8,
    len: usize,
) -> bool {
    if filter.is_null() {
        return true;
    }
    // SAFETY: a live NameFilter and readable name bytes (contract).
    unsafe { &*filter.cast::<NameFilter>() }.matches(unsafe { bytes(name, len) })
}

/// # Safety
/// `filter` is from `telamon_namefilter_new` (or null) and is not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_namefilter_free(filter: *mut c_void) {
    if !filter.is_null() {
        // SAFETY: created by Box::into_raw in telamon_namefilter_new.
        drop(unsafe { Box::from_raw(filter.cast::<NameFilter>()) });
    }
}

// ---- Search inside files ----

/// What a search inside files looked at and left out (the C++ twin is
/// `TelamonContentStats` in RustBridge.h).
#[repr(C)]
pub struct TelamonContentStats {
    pub searched: u32,
    pub binary: u32,
    pub too_large: u32,
    pub pdf_no_tool: u32,
    pub pdf_timeout: u32,
    pub unreadable: u32,
}

/// Called from the content walk's thread with a batch of hits (see
/// [`encode_content`]), `end` 0 and no stats; and once at the end with no
/// hits, `end` 1 done, 2 stopped, 3 cut at the limit, 4 the folder could not
/// be read, 5 the time or size limit, and the stats. After the last call the
/// walk never touches `user` again.
pub type ContentCallback = extern "C" fn(
    user: *mut c_void,
    batch: *const u8,
    len: usize,
    end: u32,
    stats: *const TelamonContentStats,
);

/// One hit as a record: is_dir (1 byte, 0), class (1, 0), size (u64), mtime
/// (i64), URI length (u32), the URI, then the line number (u32), the further
/// matching lines (u32), the snippet's length (u32) and the snippet.
fn encode_content(hits: &[ContentHit], out: &mut Vec<u8>) {
    for h in hits {
        let uri = path_to_uri(&h.path);
        out.push(0);
        out.push(0);
        out.extend_from_slice(&h.size.to_le_bytes());
        out.extend_from_slice(&h.mtime.to_le_bytes());
        out.extend_from_slice(&(uri.len() as u32).to_le_bytes());
        out.extend_from_slice(uri.as_bytes());
        out.extend_from_slice(&h.found.line.to_le_bytes());
        out.extend_from_slice(&h.found.more.to_le_bytes());
        out.extend_from_slice(&(h.found.snippet.len() as u32).to_le_bytes());
        out.extend_from_slice(h.found.snippet.as_bytes());
    }
}

/// Starts a search inside the files of the tree at `root`, on its own
/// thread; hits arrive through `callback`. The filter, hidden flag and tag
/// choose which files are looked at (the filter's words are not used: `query`
/// is what to look for). Stop and release it with `telamon_walk_stop` and
/// `telamon_walk_free`. Null when the query is empty or invalid, or the
/// thread could not start (the callback is then not called).
///
/// # Safety
/// `root`, `query` and `tag` cover their lengths; `filter` is valid; `user`
/// stays valid until the callback has been called with a non-zero `end`.
#[allow(clippy::too_many_arguments)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_content_start(
    root: *const u8,
    root_len: usize,
    query: *const u8,
    query_len: usize,
    use_pattern: bool,
    filter: *const TelamonSearchFilter,
    include_hidden: bool,
    tag: *const u8,
    tag_len: usize,
    max_hits: usize,
    callback: ContentCallback,
    user: *mut c_void,
) -> *mut c_void {
    if filter.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: the contract: lengths as given, `filter` valid.
    let (q, tag, f, root) = unsafe {
        (
            String::from_utf8_lossy(bytes(query, query_len)).into_owned(),
            String::from_utf8_lossy(bytes(tag, tag_len)).into_owned(),
            &*filter,
            PathBuf::from(std::ffi::OsStr::from_bytes(bytes(root, root_len))),
        )
    };
    let Ok(query) = ContentQuery::new(&q, use_pattern) else {
        return std::ptr::null_mut();
    };
    let matcher = LiveMatcher::new(
        "",
        options_of(f, include_hidden, (!tag.is_empty()).then_some(tag)),
    );
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let user = UserPtr(user);
    let max = max_hits.clamp(1, content::MAX_CONTENT_HITS);
    // For the smoke tests (debug builds only): a pause after every batch.
    #[cfg(debug_assertions)]
    let pause = std::env::var("TELAMON_EXPLORER_TEST_WALK_BATCH_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(|ms| std::time::Duration::from_millis(ms.min(10_000)));
    let spawned = std::thread::Builder::new()
        .name("content-walk".into())
        .spawn(move || {
            let user = user;
            let mut buf: Vec<u8> = Vec::new();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                walk_content(&root, &matcher, &query, &flag, max, &mut |hits| {
                    buf.clear();
                    encode_content(&hits, &mut buf);
                    callback(user.0, buf.as_ptr(), buf.len(), 0, std::ptr::null());
                    #[cfg(debug_assertions)]
                    if let Some(p) = pause {
                        std::thread::sleep(p);
                    }
                })
            }))
            .unwrap_or((WalkEnd::Unreadable, Stats::default()));
            let (end, stats) = result;
            let code = match end {
                WalkEnd::Done => 1,
                WalkEnd::Stopped => 2,
                WalkEnd::Capped => 3,
                WalkEnd::Unreadable => 4,
                WalkEnd::Limit => 5,
            };
            let out = TelamonContentStats {
                searched: stats.searched,
                binary: stats.binary,
                too_large: stats.too_large,
                pdf_no_tool: stats.pdf_no_tool,
                pdf_timeout: stats.pdf_timeout,
                unreadable: stats.unreadable,
            };
            callback(user.0, std::ptr::null(), 0, code, &out);
        });
    if spawned.is_err() {
        return std::ptr::null_mut();
    }
    Box::into_raw(Box::new(Walking { stop })).cast()
}

/// The line under a search inside files. `end` is 0 while it runs, else the
/// callback's `end`; `found` the files listed; `stats` what was looked at.
///
/// # Safety
/// `stats` is valid (or null); `out` points to `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_content_text(
    found: u64,
    end: u32,
    stats: *const TelamonContentStats,
    out: *mut u8,
    cap: usize,
) -> usize {
    let ending = match end {
        0 => Ending::Running,
        2 => Ending::Stopped,
        3 => Ending::TooMany,
        4 => Ending::Unreadable,
        5 => Ending::Limit,
        _ => Ending::Done,
    };
    let st = if stats.is_null() {
        Stats::default()
    } else {
        // SAFETY: valid (contract); plain numbers.
        let s = unsafe { &*stats };
        Stats {
            searched: s.searched,
            binary: s.binary,
            too_large: s.too_large,
            pdf_no_tool: s.pdf_no_tool,
            pdf_timeout: s.pdf_timeout,
            unreadable: s.unreadable,
        }
    };
    let n = usize::try_from(found).unwrap_or(usize::MAX);
    let t = content::summary(n, ending, &st);
    // SAFETY: `out` as promised.
    unsafe { put(t.as_bytes(), out, cap) }
}

/// The one line that says what Inside Files does.
///
/// # Safety
/// `out` points to `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_content_note(out: *mut u8, cap: usize) -> usize {
    // SAFETY: `out` as promised.
    unsafe { put(content::NOTE.as_bytes(), out, cap) }
}

/// Is there a `pdftotext` to read PDFs with?
#[unsafe(no_mangle)]
pub extern "C" fn telamon_content_pdf_available() -> bool {
    content::pdftotext().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::mpsc;

    fn text(which: u32, n: u64, flags: u32) -> String {
        let mut buf = [0u8; 256];
        let len = unsafe { telamon_search_text(which, n, flags, buf.as_mut_ptr(), buf.len()) };
        String::from_utf8(buf[..len].to_vec()).unwrap()
    }

    #[test]
    fn filter_and_kinds_through_the_abi() {
        let mut f = std::mem::MaybeUninit::<TelamonSearchFilter>::uninit();
        unsafe { telamon_search_filter(2, 2, 3, 1_000_000, 900_000, f.as_mut_ptr()) };
        let f = unsafe { f.assume_init() };
        assert_eq!(f.kinds_mask, Category::Image.bit());
        assert!(f.files_only && f.has_after && f.has_min && !f.has_max);
        assert_eq!(f.after, 1_000_000 - 7 * 86_400);
        assert_eq!(f.min, search::LARGE_LIMIT);

        let mut buf = [0u8; 128];
        let n = unsafe { telamon_search_kinds(1, buf.as_mut_ptr(), buf.len()) };
        let names = String::from_utf8_lossy(&buf[..n]).into_owned();
        assert_eq!(names.lines().count(), 5);
        assert!(names.lines().any(|l| l == "pdf"));
        assert_eq!(
            unsafe { telamon_search_kinds(0, buf.as_mut_ptr(), buf.len()) },
            0
        );
        // A null out is tolerated.
        unsafe { telamon_search_filter(0, 0, 0, 0, 0, std::ptr::null_mut()) };
    }

    #[test]
    fn routes_states_and_texts() {
        assert_eq!(telamon_search_route(0, true, true, true), 1);
        assert_eq!(telamon_search_route(0, false, false, true), 4);
        assert_eq!(telamon_search_route(1, true, true, false), 3);
        let st = |s: &str| unsafe { telamon_search_index_state(s.as_ptr(), s.len()) };
        assert_eq!(
            (st("ready"), st("scanning"), st("disabled"), st("error")),
            (1, 2, 3, 4)
        );
        assert_eq!(st("zzz"), 0);
        assert!(!telamon_search_index_on(3) && telamon_search_index_on(2));
        let mut buf = [0u8; 256];
        let mut level = 9u32;
        let n = unsafe {
            telamon_search_chip(
                2,
                std::ptr::null(),
                0,
                buf.as_mut_ptr(),
                buf.len(),
                &mut level,
            )
        };
        assert_eq!(level, 2);
        assert_eq!(
            String::from_utf8_lossy(&buf[..n]),
            "Updating the search index, results may be missing"
        );
        let n = unsafe {
            telamon_search_chip(
                5,
                std::ptr::null(),
                0,
                buf.as_mut_ptr(),
                buf.len(),
                std::ptr::null_mut(),
            )
        };
        assert_eq!(String::from_utf8_lossy(&buf[..n]), "Search isn't available");
        assert_eq!(text(0, 3, 0), "3 results");
        assert_eq!(text(1, 2, 1), "Searching, 2 found");
        assert_eq!(text(2, 0, 0), "Search Isn't Available");
        assert!(text(3, 0, 0).contains("search index"));
        assert_eq!(text(4, 0, 0), "Search isn't available");
        assert_eq!(text(99, 0, 0), "");
        assert_eq!(telamon_search_limit(0), 500);
        assert_eq!(telamon_search_limit(1), 5000);
        assert_eq!(telamon_search_limit(2), 1024 * 1024);
        assert_eq!(telamon_search_limit(3), 100 * 1024 * 1024);
    }

    #[test]
    fn path_text_through_the_abi() {
        let (p, h) = ("file:///home/u/Documents", "/home/u");
        let mut buf = [0u8; 128];
        let n = unsafe {
            telamon_search_path_text(
                p.as_ptr(),
                p.len(),
                h.as_ptr(),
                h.len(),
                buf.as_mut_ptr(),
                buf.len(),
            )
        };
        assert_eq!(String::from_utf8_lossy(&buf[..n]), "~/Documents");
    }

    #[test]
    fn matcher_through_the_abi() {
        let mut f = std::mem::MaybeUninit::<TelamonSearchFilter>::uninit();
        unsafe { telamon_search_filter(2, 0, 0, 0, 0, f.as_mut_ptr()) };
        let f = unsafe { f.assume_init() };
        let q = "holiday";
        let m = unsafe { telamon_matcher_new(q.as_ptr(), q.len(), &f, false, false) };
        assert!(!m.is_null());
        let t = |name: &str| unsafe {
            telamon_matcher_test(m, name.as_ptr(), name.len(), false, 10, 5)
        };
        assert!(t("Holiday 2024.png") > 0);
        assert_eq!(t("Holiday 2024.txt"), 0, "not an image");
        assert_eq!(t("beach.png"), 0, "not the name");
        assert_eq!(t(".holiday.png"), 0, "hidden");
        assert!(t("holiday.png") > t("my-holiday-pics.png"));
        unsafe { telamon_matcher_free(m) };
        assert!(
            unsafe { telamon_matcher_new(q.as_ptr(), q.len(), std::ptr::null(), false, false) }
                .is_null()
        );
        assert_eq!(
            unsafe { telamon_matcher_test(std::ptr::null(), q.as_ptr(), q.len(), false, 0, 0) },
            0
        );
    }

    struct Sink {
        tx: Mutex<mpsc::Sender<(Vec<u8>, u32)>>,
    }

    /// A sink that lives for the rest of the process. The walk's thread is
    /// still inside this callback (releasing the lock, dropping the sender)
    /// when the receiver sees the `end` message, so a Sink the test dropped
    /// at its own end would be freed under that thread: a write into freed
    /// memory, seen as a corrupted heap and a SIGSEGV in whichever test
    /// allocates next. A few bytes leaked per test is the price.
    fn leaked_sink() -> (mpsc::Receiver<(Vec<u8>, u32)>, *mut c_void) {
        let (tx, rx) = mpsc::channel();
        let sink: &'static Sink = Box::leak(Box::new(Sink { tx: Mutex::new(tx) }));
        (rx, sink as *const Sink as *mut c_void)
    }

    extern "C" fn sink(user: *mut c_void, batch: *const u8, len: usize, end: u32) {
        // SAFETY: `user` is a leaked Sink (see `leaked_sink`), never freed.
        let s = unsafe { &*(user as *const Sink) };
        let data = if batch.is_null() {
            Vec::new()
        } else {
            unsafe { std::slice::from_raw_parts(batch, len) }.to_vec()
        };
        let _ = s.tx.lock().unwrap().send((data, end));
    }

    /// Reads the records back: (uri, is_dir, class, size, mtime).
    fn decode(mut b: &[u8]) -> Vec<(String, bool, u8, u64, i64)> {
        let mut out = Vec::new();
        while !b.is_empty() {
            let (is_dir, class) = (b[0] != 0, b[1]);
            let size = u64::from_le_bytes(b[2..10].try_into().unwrap());
            let mtime = i64::from_le_bytes(b[10..18].try_into().unwrap());
            let n = u32::from_le_bytes(b[18..22].try_into().unwrap()) as usize;
            out.push((
                String::from_utf8(b[22..22 + n].to_vec()).unwrap(),
                is_dir,
                class,
                size,
                mtime,
            ));
            b = &b[22 + n..];
        }
        out
    }

    #[test]
    fn a_walk_streams_hits_and_ends() {
        let dir = atlas_file_index::testdir::Scratch::new("ffi-walk");
        std::fs::create_dir_all(dir.0.join("a b/c")).unwrap();
        std::fs::write(dir.0.join("a b/c/needle one.txt"), "hello").unwrap();
        std::fs::write(dir.0.join("needle two.md"), "x").unwrap();
        std::fs::write(dir.0.join("other.txt"), "x").unwrap();
        let (rx, user) = leaked_sink();
        let root = dir.0.as_os_str().as_bytes().to_vec();
        let mut f = std::mem::MaybeUninit::<TelamonSearchFilter>::uninit();
        unsafe { telamon_search_filter(0, 0, 0, 0, 0, f.as_mut_ptr()) };
        let f = unsafe { f.assume_init() };
        let q = "needle";
        let h = unsafe {
            telamon_walk_start(
                root.as_ptr(),
                root.len(),
                q.as_ptr(),
                q.len(),
                &f,
                false,
                false,
                std::ptr::null(),
                0,
                100,
                sink,
                user,
            )
        };
        assert!(!h.is_null());
        let mut hits = Vec::new();
        let end = loop {
            let (data, end) = rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
            hits.extend(decode(&data));
            if end != 0 {
                break end;
            }
        };
        assert_eq!(end, 1);
        hits.sort();
        assert_eq!(hits.len(), 2);
        assert!(
            hits[0].0.starts_with("file://") && hits[0].0.contains("a%20b/c/needle%20one.txt"),
            "{hits:?}"
        );
        assert_eq!((hits[0].1, hits[0].3), (false, 5));
        unsafe {
            telamon_walk_stop(h); // after the end: harmless
            telamon_walk_free(h);
        }

        // A folder that is not there: the end says so, with no hits.
        let (rx, user) = leaked_sink();
        let nope = dir.0.join("nope");
        let nope = nope.as_os_str().as_bytes();
        let h = unsafe {
            telamon_walk_start(
                nope.as_ptr(),
                nope.len(),
                q.as_ptr(),
                q.len(),
                &f,
                false,
                false,
                std::ptr::null(),
                0,
                100,
                sink,
                user,
            )
        };
        let (data, end) = rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        assert!(data.is_empty());
        assert_eq!(end, 4);
        unsafe { telamon_walk_free(h) };
    }

    #[test]
    fn covers_through_the_abi() {
        let dir = atlas_file_index::testdir::Scratch::new("ffi-covers");
        std::fs::create_dir_all(dir.0.join("x/y")).unwrap();
        let root = dir.0.as_os_str().as_bytes().to_vec();
        let inner = dir.0.join("x/y");
        let inner = inner.as_os_str().as_bytes();
        assert!(unsafe {
            telamon_search_covers(inner.as_ptr(), inner.len(), root.as_ptr(), root.len())
        });
        assert!(!unsafe { telamon_search_covers(b"/usr".as_ptr(), 4, root.as_ptr(), root.len()) });
        assert!(!unsafe {
            telamon_search_covers(inner.as_ptr(), inner.len(), std::ptr::null(), 0)
        });
    }

    #[test]
    fn pattern_check_and_the_folder_filter_through_the_abi() {
        let check = |t: &str, pat: bool, contents: bool| {
            let mut buf = [0u8; 256];
            let n = unsafe {
                telamon_pattern_check(
                    t.as_ptr(),
                    t.len(),
                    pat,
                    contents,
                    buf.as_mut_ptr(),
                    buf.len(),
                )
            };
            String::from_utf8_lossy(&buf[..n.min(256)]).into_owned()
        };
        assert_eq!(
            check("(abc", false, false),
            "",
            "plain words are never a pattern error"
        );
        assert_eq!(check("", true, false), "");
        assert!(check("(abc", true, false).contains("unclosed group"));
        assert_eq!(check(&"a".repeat(600), false, false), "");
        assert!(check(&"a".repeat(600), false, true).contains("too long"));

        let mut err = [0u8; 256];
        let mut err_len = 9usize;
        let new = |t: &str, pat: bool, err: &mut [u8], len: &mut usize| unsafe {
            telamon_namefilter_new(t.as_ptr(), t.len(), pat, err.as_mut_ptr(), err.len(), len)
        };
        let f = new("^img_", true, &mut err, &mut err_len);
        assert!(!f.is_null() && err_len == 0);
        let test =
            |f: *mut c_void, n: &str| unsafe { telamon_namefilter_test(f, n.as_ptr(), n.len()) };
        assert!(test(f, "IMG_1.jpg") && !test(f, "my IMG_1.jpg"));
        assert!(!unsafe { telamon_namefilter_is_all(f) });
        unsafe { telamon_namefilter_free(f) };
        let f = new("", false, &mut err, &mut err_len);
        assert!(unsafe { telamon_namefilter_is_all(f) } && test(f, "anything"));
        unsafe { telamon_namefilter_free(f) };
        let f = new("(img", true, &mut err, &mut err_len);
        assert!(f.is_null());
        assert!(String::from_utf8_lossy(&err[..err_len]).contains("unclosed group"));
        // A null filter keeps everything; freeing null is harmless.
        assert!(test(std::ptr::null_mut(), "x"));
        unsafe { telamon_namefilter_free(std::ptr::null_mut()) };
    }

    /// A batch, its `end`, and the stats that come with the last call.
    type CMessage = (Vec<u8>, u32, Option<[u32; 6]>);

    struct CSink {
        tx: Mutex<mpsc::Sender<CMessage>>,
    }

    extern "C" fn csink(
        user: *mut c_void,
        batch: *const u8,
        len: usize,
        end: u32,
        stats: *const TelamonContentStats,
    ) {
        // SAFETY: `user` is a leaked CSink, never freed (see `leaked_sink`).
        let s = unsafe { &*(user as *const CSink) };
        let data = if batch.is_null() {
            Vec::new()
        } else {
            unsafe { std::slice::from_raw_parts(batch, len) }.to_vec()
        };
        let st = (!stats.is_null()).then(|| {
            let s = unsafe { &*stats };
            [
                s.searched,
                s.binary,
                s.too_large,
                s.pdf_no_tool,
                s.pdf_timeout,
                s.unreadable,
            ]
        });
        let _ = s.tx.lock().unwrap().send((data, end, st));
    }

    /// (uri, size, line, more, snippet)
    fn decode_content(mut b: &[u8]) -> Vec<(String, u64, u32, u32, String)> {
        let mut out = Vec::new();
        while !b.is_empty() {
            let size = u64::from_le_bytes(b[2..10].try_into().unwrap());
            let n = u32::from_le_bytes(b[18..22].try_into().unwrap()) as usize;
            let uri = String::from_utf8(b[22..22 + n].to_vec()).unwrap();
            let r = &b[22 + n..];
            let line = u32::from_le_bytes(r[0..4].try_into().unwrap());
            let more = u32::from_le_bytes(r[4..8].try_into().unwrap());
            let m = u32::from_le_bytes(r[8..12].try_into().unwrap()) as usize;
            let snip = String::from_utf8(r[12..12 + m].to_vec()).unwrap();
            out.push((uri, size, line, more, snip));
            b = &r[12 + m..];
        }
        out
    }

    #[test]
    fn a_content_search_streams_hits_with_snippets_and_ends_with_stats() {
        let dir = atlas_file_index::testdir::Scratch::new("ffi-content");
        std::fs::create_dir_all(dir.0.join("sub dir")).unwrap();
        std::fs::write(dir.0.join("sub dir/a.txt"), "one\ntwo needle here\n").unwrap();
        std::fs::write(dir.0.join("b.rs"), "needle\nneedle\n").unwrap();
        std::fs::write(dir.0.join("c.txt"), "nothing\n").unwrap();
        std::fs::write(dir.0.join("d.bin"), b"needle\0\0").unwrap();
        let (tx, rx) = mpsc::channel();
        let sink: &'static CSink = Box::leak(Box::new(CSink { tx: Mutex::new(tx) }));
        let user = sink as *const CSink as *mut c_void;
        let root = dir.0.as_os_str().as_bytes().to_vec();
        let mut f = std::mem::MaybeUninit::<TelamonSearchFilter>::uninit();
        unsafe { telamon_search_filter(0, 0, 0, 0, 0, f.as_mut_ptr()) };
        let f = unsafe { f.assume_init() };
        let q = "NEEDLE";
        let h = unsafe {
            telamon_content_start(
                root.as_ptr(),
                root.len(),
                q.as_ptr(),
                q.len(),
                false,
                &f,
                false,
                std::ptr::null(),
                0,
                100,
                csink,
                user,
            )
        };
        assert!(!h.is_null());
        let mut hits = Vec::new();
        let (end, stats) = loop {
            let (data, end, st) = rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
            hits.extend(decode_content(&data));
            if end != 0 {
                break (end, st.unwrap());
            }
        };
        assert_eq!(end, 1);
        hits.sort();
        assert_eq!(hits.len(), 2, "{hits:?}");
        assert!(
            hits[0].0.contains("b.rs") && hits[0].2 == 1 && hits[0].3 == 1 && hits[0].4 == "needle"
        );
        assert!(
            hits[1].0.contains("sub%20dir/a.txt")
                && hits[1].2 == 2
                && hits[1].4 == "two needle here"
        );
        // searched: the two with hits and c.txt; one binary.
        assert_eq!((stats[0], stats[1]), (3, 1), "{stats:?}");
        unsafe { telamon_walk_free(h) };
        // Nothing to look for, or a bad pattern: no search.
        let none = unsafe {
            telamon_content_start(
                root.as_ptr(),
                root.len(),
                b"  ".as_ptr(),
                2,
                false,
                &f,
                false,
                std::ptr::null(),
                0,
                100,
                csink,
                user,
            )
        };
        assert!(none.is_null());
        let bad = unsafe {
            telamon_content_start(
                root.as_ptr(),
                root.len(),
                b"(x".as_ptr(),
                2,
                true,
                &f,
                false,
                std::ptr::null(),
                0,
                100,
                csink,
                user,
            )
        };
        assert!(bad.is_null());
        let mut buf = [0u8; 256];
        let n =
            unsafe { telamon_content_text(2, 1, std::ptr::null(), buf.as_mut_ptr(), buf.len()) };
        assert_eq!(String::from_utf8_lossy(&buf[..n]), "2 files contain it");
        let n = unsafe { telamon_content_note(buf.as_mut_ptr(), buf.len()) };
        assert!(String::from_utf8_lossy(&buf[..n]).contains("Nothing is stored"));
        assert_eq!(telamon_search_route_live(0, true), 2);
        assert_eq!(telamon_search_route_live(1, true), 3);
        assert_eq!(telamon_search_route_live(0, false), 4);
    }
}
