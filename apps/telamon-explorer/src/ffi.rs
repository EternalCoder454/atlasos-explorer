//! C ABI over `atlas_explorer_core` for the C++ adapters (`cpp/kio/*`): display
//! names, sort keys and the sort permutation. The logic stays in the core
//! crate; these functions only move bytes. Every pointer is checked for null.

use atlas_explorer_core::display_name;
use atlas_explorer_core::sort::{Column, SortRow, name_key, sort_permutation_grouped};
use atlas_explorer_core::{address, archive, location, menu, names, places, tabs};

/// One row for `telamon_sort_permutation`; the C++ twin is in FolderModel.cpp.
#[repr(C)]
pub struct TelamonSortRow {
    pub key: *const u8,
    pub key_len: usize,
    pub kind: *const u8,
    pub kind_len: usize,
    /// The order key of the row's group (`telamon_group_of`); empty: no groups.
    pub group: *const u8,
    pub group_len: usize,
    pub size: u64,
    pub mtime: i64,
    pub ctime: i64,
    pub atime: i64,
    /// The Trash: the sort key of the folder an item came from (empty elsewhere).
    pub origin: *const u8,
    pub origin_len: usize,
    /// The Trash: when it was deleted (0 elsewhere).
    pub deleted: i64,
    pub is_dir: bool,
}

/// Runs `f`; if it panics, the panic stops here and `failure` is the answer.
/// A panic that crosses an `extern "C"` function ends the whole program (and
/// with it the operations queued), and these functions read file names, file
/// contents, settings text and other programs' arguments: all untrusted. The
/// message never holds the input.
pub(crate) fn guarded<T>(failure: T, f: impl FnOnce() -> T) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(v) => v,
        Err(_) => {
            log::error!("an internal error was contained while reading untrusted input");
            failure
        }
    }
}

/// # Safety
/// `ptr` is null or points to `len` readable bytes.
pub(crate) unsafe fn bytes<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    if ptr.is_null() || len == 0 {
        &[]
    } else {
        // SAFETY: the caller promises `len` readable bytes.
        unsafe { std::slice::from_raw_parts(ptr, len) }
    }
}

/// Copies `data` into `out` if it fits; returns its length either way, so a
/// caller with a short buffer can retry with the right size.
///
/// # Safety
/// `out` is null or points to `cap` writable bytes.
pub(crate) unsafe fn put(data: &[u8], out: *mut u8, cap: usize) -> usize {
    if !out.is_null() && data.len() <= cap {
        // SAFETY: `out` has `cap >= data.len()` writable bytes.
        unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), out, data.len()) };
    }
    data.len()
}

/// The display form of a file name (controls and bidi made visible), UTF-8.
///
/// # Safety
/// `name` points to `len` readable bytes (or is null with `len` 0); `out`
/// points to `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_display_name(
    name: *const u8,
    len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        // SAFETY: forwarded from this function's contract.
        unsafe {
            let s = display_name(bytes(name, len));
            put(s.as_bytes(), out, cap)
        }
    })
}

/// The natural sort key of a file name.
///
/// # Safety
/// As for `telamon_display_name`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_name_key(
    name: *const u8,
    len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        // SAFETY: forwarded from this function's contract.
        unsafe {
            let k = name_key(bytes(name, len));
            put(&k, out, cap)
        }
    })
}

/// Sorts `n` rows by `column` (0 Name, 1 Size, 2 Type, 3 Modified, 4 Created,
/// 5 Accessed, 7 Original Location, 8 Date Deleted; the Trash's two read the
/// rows' `origin` and `deleted`) and writes the permutation to `out` (`n` u32s). False on bad input.
///
/// # Safety
/// `rows` points to `n` valid rows whose pointers cover their lengths; `out`
/// points to `n` writable u32s.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_sort_permutation(
    rows: *const TelamonSortRow,
    n: usize,
    column: u32,
    descending: bool,
    folders_first: bool,
    groups_reversed: bool,
    out: *mut u32,
) -> bool {
    crate::ffi::guarded(false, || {
        if n == 0 {
            return true;
        }
        if rows.is_null() || out.is_null() || n > u32::MAX as usize {
            return false;
        }
        let column = match column {
            0 => Column::Name,
            1 => Column::Size,
            2 => Column::Type,
            3 => Column::Modified,
            4 => Column::Created,
            5 => Column::Accessed,
            7 => Column::Original,
            8 => Column::Deleted,
            _ => return false,
        };
        // SAFETY: `rows` has `n` valid rows (contract above).
        let rows = unsafe { std::slice::from_raw_parts(rows, n) };
        let rows: Vec<SortRow> = rows
            .iter()
            .map(|r| SortRow {
                // SAFETY: each row's pointers cover their lengths (contract).
                key: unsafe { bytes(r.key, r.key_len) }.to_vec(),
                kind: String::from_utf8_lossy(unsafe { bytes(r.kind, r.kind_len) }).into_owned(),
                // SAFETY: as for the key and the kind.
                group: unsafe { bytes(r.group, r.group_len) }.to_vec(),
                // SAFETY: as for the key.
                origin: unsafe { bytes(r.origin, r.origin_len) }.to_vec(),
                deleted: r.deleted,
                is_dir: r.is_dir,
                size: r.size,
                mtime: r.mtime,
                ctime: r.ctime,
                atime: r.atime,
            })
            .collect();
        let perm =
            sort_permutation_grouped(&rows, column, descending, folders_first, groups_reversed);
        // SAFETY: `out` has `n` writable u32s and `perm.len() == n`.
        unsafe { std::ptr::copy_nonoverlapping(perm.as_ptr(), out, perm.len()) };
        true
    })
}

/// Checks a new file or folder name. Returns 0 when it is fine (the text, if
/// any, lists warnings, one per line), 1 when it is refused (the text says why).
/// The text's length is stored in `*text_len`, as `telamon_display_name` does.
///
/// # Safety
/// As for `telamon_display_name`; `text_len` points to a writable usize.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_validate_name(
    name: *const u8,
    len: usize,
    out: *mut u8,
    cap: usize,
    text_len: *mut usize,
) -> i32 {
    crate::ffi::guarded(1, || {
        if text_len.is_null() {
            return 1;
        }
        // SAFETY: forwarded from this function's contract.
        let name = String::from_utf8_lossy(unsafe { bytes(name, len) }).into_owned();
        let (code, text) = match names::validate(&name) {
            Ok(w) => (
                0,
                w.iter()
                    .map(|w| w.describe())
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            Err(e) => (1, e.describe().to_string()),
        };
        // SAFETY: `out` and `text_len` as promised above.
        unsafe { *text_len = put(text.as_bytes(), out, cap) };
        code
    })
}

/// Reads typed address text. Returns 0 and the URL in `out`, or 1 and the
/// reason in plain words; the length is stored in `*text_len`.
///
/// # Safety
/// Each pointer pair covers its length (or is null with length 0); `out`
/// points to `cap` writable bytes (or is null); `text_len` is writable.
#[allow(clippy::too_many_arguments)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_parse_address(
    text: *const u8,
    len: usize,
    current: *const u8,
    current_len: usize,
    home: *const u8,
    home_len: usize,
    out: *mut u8,
    cap: usize,
    text_len: *mut usize,
) -> i32 {
    crate::ffi::guarded(1, || {
        if text_len.is_null() {
            return 1;
        }
        // SAFETY: forwarded from this function's contract.
        let (text, current, home) = unsafe {
            (
                String::from_utf8_lossy(bytes(text, len)).into_owned(),
                String::from_utf8_lossy(bytes(current, current_len)).into_owned(),
                String::from_utf8_lossy(bytes(home, home_len)).into_owned(),
            )
        };
        let (code, msg) = match address::parse(&text, &current, std::path::Path::new(&home)) {
            Ok(url) => (0, url),
            Err(why) => (1, why.to_string()),
        };
        // SAFETY: `out` and `text_len` as promised above.
        unsafe { *text_len = put(msg.as_bytes(), out, cap) };
        code
    })
}

/// The clickable parts of a location, for the path bar: one line per segment,
/// `label`, a tab, then its URL, lines joined by `\n` (labels hold no control
/// character, URLs none either). `home` is the home folder as a plain path.
/// Nothing for a text that is not a location. Returns the length of the
/// output; more than `cap` means it did not fit (nothing was written).
///
/// # Safety
/// Each pointer pair covers its length (or is null with length 0); `out`
/// points to `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_path_segments(
    url: *const u8,
    len: usize,
    home: *const u8,
    home_len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        // SAFETY: forwarded from this function's contract.
        let (url, home) = unsafe {
            (
                String::from_utf8_lossy(bytes(url, len)).into_owned(),
                String::from_utf8_lossy(bytes(home, home_len)).into_owned(),
            )
        };
        let text = location::segments(&url, &home)
            .into_iter()
            .map(|s| format!("{}\t{}", s.label, s.url))
            .collect::<Vec<_>>()
            .join("\n");
        // SAFETY: `out` as promised above.
        unsafe { put(text.as_bytes(), out, cap) }
    })
}

/// Splits typed address text for completion: the folder to list (a URL) and
/// the name prefix typed, written as `url`, `\n`, `prefix` (0), or the reason
/// in plain words (1). The length goes to `*text_len`, as in
/// `telamon_parse_address`.
///
/// # Safety
/// Each pointer pair covers its length (or is null with length 0); `out`
/// points to `cap` writable bytes (or is null); `text_len` is writable.
#[allow(clippy::too_many_arguments)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_split_for_completion(
    text: *const u8,
    len: usize,
    current: *const u8,
    current_len: usize,
    home: *const u8,
    home_len: usize,
    out: *mut u8,
    cap: usize,
    text_len: *mut usize,
) -> i32 {
    crate::ffi::guarded(1, || {
        if text_len.is_null() {
            return 1;
        }
        // SAFETY: forwarded from this function's contract.
        let (text, current, home) = unsafe {
            (
                String::from_utf8_lossy(bytes(text, len)).into_owned(),
                String::from_utf8_lossy(bytes(current, current_len)).into_owned(),
                String::from_utf8_lossy(bytes(home, home_len)).into_owned(),
            )
        };
        let (code, msg) =
            match address::split_for_completion(&text, &current, std::path::Path::new(&home)) {
                Ok((dir, prefix)) => (0, format!("{dir}\n{prefix}")),
                Err(why) => (1, why.to_string()),
            };
        // SAFETY: `out` and `text_len` as promised above.
        unsafe { *text_len = put(msg.as_bytes(), out, cap) };
        code
    })
}

/// Ranks the names of a folder. `entries` holds one record per name: a flag
/// byte (1 = folder, 2 = hidden), the name's bytes, then a 0 byte. Mode 0 is
/// completion of `prefix` (best first, at most `address::MAX_COMPLETIONS`),
/// mode 1 the subfolder menu (natural order, hidden ones only with
/// `show_hidden`, not capped). The chosen records' positions are written to
/// `out` (`u32`s); returns how many there are, and when that is more than
/// `cap` nothing was written.
///
/// # Safety
/// Each pointer pair covers its length (or is null with length 0); `out`
/// points to `cap` writable `u32`s (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_rank_names(
    mode: u32,
    prefix: *const u8,
    prefix_len: usize,
    entries: *const u8,
    entries_len: usize,
    show_hidden: bool,
    out: *mut u32,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        // SAFETY: forwarded from this function's contract.
        let (prefix, entries) = unsafe {
            (
                String::from_utf8_lossy(bytes(prefix, prefix_len)).into_owned(),
                bytes(entries, entries_len),
            )
        };
        let candidates: Vec<address::Candidate> = entries
            .split(|&b| b == 0)
            .filter(|r| !r.is_empty())
            .map(|r| address::Candidate {
                name: String::from_utf8_lossy(&r[1..]).into_owned(),
                is_dir: r[0] & 1 != 0,
                hidden: r[0] & 2 != 0,
            })
            .collect();
        let chosen = match mode {
            0 => address::rank_completions(&prefix, &candidates),
            1 => location::subfolder_order(&candidates, show_hidden),
            _ => Vec::new(),
        };
        if chosen.len() <= cap && !out.is_null() {
            for (k, &i) in chosen.iter().enumerate() {
                // SAFETY: `out` has `cap >= chosen.len()` writable slots.
                unsafe { *out.add(k) = i as u32 };
            }
        }
        chosen.len()
    })
}

/// The address bar's text after a completion is taken, `dir` being the folder
/// listed (see `address::completion_text`). Returns the length of the output; more than
/// `cap` means it did not fit.
///
/// # Safety
/// Each pointer pair covers its length (or is null with length 0); `out`
/// points to `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_completion_text(
    typed: *const u8,
    len: usize,
    name: *const u8,
    name_len: usize,
    dir: *const u8,
    dir_len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        // SAFETY: forwarded from this function's contract.
        let (typed, name, dir) = unsafe {
            (
                String::from_utf8_lossy(bytes(typed, len)).into_owned(),
                String::from_utf8_lossy(bytes(name, name_len)).into_owned(),
                String::from_utf8_lossy(bytes(dir, dir_len)).into_owned(),
            )
        };
        let text = address::completion_text(&typed, &name, &dir);
        // SAFETY: `out` as promised above.
        unsafe { put(text.as_bytes(), out, cap) }
    })
}

/// Limits of the path bar and the history menus: 0 rows of a subfolder menu,
/// 1 completions offered, 2 places a Back or Forward menu lists; 0 for
/// anything else.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_location_limit(which: u32) -> usize {
    match which {
        0 => location::MAX_MENU,
        1 => address::MAX_COMPLETIONS,
        2 => location::MAX_HISTORY_MENU,
        _ => 0,
    }
}

/// The section a place is listed in (see `places::Section`): `group` is
/// KFilePlacesModel's group number and `scheme` the scheme of its URL.
///
/// # Safety
/// `scheme` points to `len` readable bytes (or is null with `len` 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_places_section(group: i32, scheme: *const u8, len: usize) -> u32 {
    // SAFETY: forwarded from this function's contract.
    let scheme = String::from_utf8_lossy(unsafe { bytes(scheme, len) }).into_owned();
    places::section_for(group, &scheme) as u32
}

/// What a place is (see `places::Kind`).
///
/// # Safety
/// `scheme` points to `len` readable bytes (or is null with `len` 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_places_kind(
    section: u32,
    scheme: *const u8,
    len: usize,
    flags: u32,
) -> u32 {
    // SAFETY: forwarded from this function's contract.
    let scheme = String::from_utf8_lossy(unsafe { bytes(scheme, len) }).into_owned();
    let section = match section {
        0 => places::Section::Favourites,
        1 => places::Section::Drives,
        2 => places::Section::Network,
        3 => places::Section::Trash,
        _ => places::Section::Unlisted,
    };
    places::kind_of(
        section,
        &scheme,
        flags & 1 != 0,
        flags & 2 != 0,
        flags & 4 != 0,
        flags & 8 != 0,
    ) as u32
}

fn kind_from(n: u32) -> places::Kind {
    use places::Kind::*;
    [
        Folder, Recent, Network, Server, Trash, Drive, Removable, Phone, Other,
    ]
    .get(n as usize)
    .copied()
    .unwrap_or(Other)
}

/// The actions a place's menu offers: bits `places::ACT_*`. `flags`: 1 the
/// place is hidden, 2 a drive is mounted, 4 the Trash is empty, 8 Telamon
/// Disks is installed.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_places_actions(kind: u32, flags: u32) -> u32 {
    places::actions(
        kind_from(kind),
        flags & 1 != 0,
        flags & 2 != 0,
        flags & 4 != 0,
        flags & 8 != 0,
    )
}

/// The `row` for `KFilePlacesModel::movePlace` when place `src` is dropped
/// on `dst`; -1 for a drop that changes nothing.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_places_reorder_row(src: usize, dst: usize) -> i64 {
    places::reorder_row(src, dst).map_or(-1, |r| r as i64)
}

/// Whether a folder with this URL scheme can be pinned.
///
/// # Safety
/// `scheme` points to `len` readable bytes (or is null with `len` 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_places_pinnable(scheme: *const u8, len: usize) -> bool {
    crate::ffi::guarded(false, || {
        // SAFETY: forwarded from this function's contract.
        let scheme = String::from_utf8_lossy(unsafe { bytes(scheme, len) }).into_owned();
        places::pinnable(&scheme)
    })
}

/// How full a disk is, in whole percent; -1 when not known.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_places_usage_percent(total: i64, free: i64) -> i32 {
    places::usage_percent(total, free)
}

/// A sidebar text (UTF-8). `which`: 0 a place's name as typed (`a`; empty
/// when refused), 1 the name for a new pin (`a` the folder's URL, `b` the
/// home folder), 2 what the Trash shows beside its name (`n` items), 3 the
/// Trash's tooltip (`n`), 4 the Empty Trash question (`n` items, `a` the
/// size as written), 5 what is said after a drive is unmounted (`n` the
/// place's kind, `a` its name), 6 the desktop file IDs of Telamon Disks and
/// 7 its program names (one per line). Returns the length of the output;
/// more than `cap` means it did not fit (nothing was written).
///
/// # Safety
/// Each pointer pair covers its length (or is null with length 0); `out`
/// points to `cap` writable bytes (or is null).
#[allow(clippy::too_many_arguments)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_places_text(
    which: u32,
    a: *const u8,
    a_len: usize,
    b: *const u8,
    b_len: usize,
    n: u64,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        // SAFETY: forwarded from this function's contract.
        let (a, b) = unsafe {
            (
                String::from_utf8_lossy(bytes(a, a_len)).into_owned(),
                String::from_utf8_lossy(bytes(b, b_len)).into_owned(),
            )
        };
        let n_items = usize::try_from(n).unwrap_or(usize::MAX);
        let text = match which {
            0 => places::clean_label(&a).unwrap_or_default(),
            1 => places::pin_label(&a, &b),
            2 => places::trash_value(n_items),
            3 => places::trash_tip(n_items),
            4 => places::empty_trash_text(n_items, &a),
            5 => places::unmounted_text(kind_from(n as u32), &a),
            6 => places::DISKS_DESKTOP_IDS.join("\n"),
            7 => places::DISKS_PROGRAMS.join("\n"),
            _ => String::new(),
        };
        // SAFETY: `out` as promised above.
        unsafe { put(text.as_bytes(), out, cap) }
    })
}

/// What a context menu offers. `kind` 0 is the menu of `count` items (`folders`
/// of them folders), 1 the menu of the background; `flags` are the `menu::F_*`
/// bits. The text has one line per entry: its key and `+` (enabled) or `-`
/// (disabled); entries that don't apply are left out. Returns the length of
/// the text; more than `cap` means it did not fit (nothing was written).
///
/// # Safety
/// `out` points to `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_menu_state(
    kind: u32,
    count: usize,
    folders: usize,
    flags: u32,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        let entries = if kind == 0 {
            menu::item_menu(count, folders, flags)
        } else {
            menu::background_menu(flags)
        };
        let text = menu::state_text(&entries);
        // SAFETY: `out` as promised above.
        unsafe { put(text.as_bytes(), out, cap) }
    })
}

/// Whether Paste in the menu of items pastes into the one folder selected.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_menu_paste_into_folder(count: usize, folders: usize) -> bool {
    menu::paste_into_selected_folder(count, folders)
}

/// Texts of "New". `which`: 0 the label of the template named `a`, 1 the name
/// proposed for a file made from template `a`, 2 the templates to list out of
/// the file names in `a` (separated by NUL), in order (separated by NUL),
/// 3 the proposed name of a new text file, 4 the proposed name of a new
/// folder, 5 the desktop file IDs of Telamon Archive (one per line).
///
/// # Safety
/// `a` points to `a_len` readable bytes (or is null with length 0); `out`
/// points to `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_menu_text(
    which: u32,
    a: *const u8,
    a_len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        // SAFETY: forwarded from this function's contract.
        let a = String::from_utf8_lossy(unsafe { bytes(a, a_len) }).into_owned();
        let text = match which {
            0 => menu::template_label(&a),
            1 => menu::new_file_name(&a),
            2 => {
                let names: Vec<String> = a
                    .split('\0')
                    .filter(|n| !n.is_empty())
                    .map(str::to_string)
                    .collect();
                menu::pick_templates(&names).join("\0")
            }
            3 => menu::NEW_TEXT_FILE.to_string(),
            4 => menu::NEW_FOLDER.to_string(),
            5 => menu::ARCHIVE_DESKTOP_IDS.join("\n"),
            _ => String::new(),
        };
        // SAFETY: `out` as promised above.
        unsafe { put(text.as_bytes(), out, cap) }
    })
}

/// Whether `scheme` is one of the archive worker's (`zip`, `tar`, `sevenz`, `ar`).
///
/// # Safety
/// `scheme` points to `len` readable bytes (or is null with length 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_archive_is_scheme(scheme: *const u8, len: usize) -> bool {
    crate::ffi::guarded(false, || {
        // SAFETY: forwarded from this function's contract.
        let s = String::from_utf8_lossy(unsafe { bytes(scheme, len) }).into_owned();
        archive::is_scheme(&s)
    })
}

/// Looks at what an archive lists before KIO copies anything out of it.
/// `records` holds, for each entry, `f`, its path and a NUL, or `l`, its path,
/// a NUL, the link's target and a NUL. Returns 0 when every entry can be
/// taken out below a folder, 1 when not, with the refusal in plain words in
/// `out` (its length in `*text_len`).
///
/// # Safety
/// Each pointer pair covers its length (or is null with length 0); `out`
/// points to `cap` writable bytes (or is null); `text_len` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_archive_check(
    records: *const u8,
    records_len: usize,
    archive_installed: bool,
    out: *mut u8,
    cap: usize,
    text_len: *mut usize,
) -> i32 {
    crate::ffi::guarded(1, || {
        if text_len.is_null() {
            return 1;
        }
        // SAFETY: forwarded from this function's contract.
        let buf = unsafe { bytes(records, records_len) };
        let report = archive::check_entries(archive::parse_records(buf));
        let (code, text) = if report.ok() {
            (0, String::new())
        } else {
            (1, archive::refusal_text(&report, archive_installed))
        };
        // SAFETY: `out` and `text_len` as promised above.
        unsafe { *text_len = put(text.as_bytes(), out, cap) };
        code
    })
}

/// What the last bytes of a file (`tail`, of a file `file_len` long) say about
/// it being a zip: 0 not a zip, 1 a zip whose central directory is described
/// by `out` (offset, size, entries and where the end record is: four u64s),
/// 2 a zip64 one whose own record is at `out[0]`, 3 a zip that can't be
/// placed. -1 on bad arguments.
///
/// # Safety
/// `tail` points to `len` readable bytes (or is null with length 0); `out`
/// points to four writable u64s.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_zip_end(
    tail: *const u8,
    len: usize,
    file_len: u64,
    out: *mut u64,
) -> i32 {
    crate::ffi::guarded(-1, || {
        if out.is_null() {
            return -1;
        }
        // SAFETY: forwarded from this function's contract.
        let tail = unsafe { bytes(tail, len) };
        // SAFETY (all writes): `out` has four writable u64s.
        match archive::zip_end(tail, file_len) {
            archive::ZipEnd::NotZip => 0,
            archive::ZipEnd::Directory(d) => {
                unsafe {
                    *out = d.offset;
                    *out.add(1) = d.size;
                    *out.add(2) = d.entries;
                    *out.add(3) = d.end_at;
                }
                1
            }
            archive::ZipEnd::Zip64At(at) => {
                unsafe { *out = at };
                2
            }
            archive::ZipEnd::Unreadable => 3,
        }
    })
}

/// The directory a zip64 end record describes (`record` is what was read at
/// the offset `telamon_zip_end` gave); fills `out` as for `telamon_zip_end`.
///
/// # Safety
/// `record` points to `len` readable bytes (or is null with length 0); `out`
/// points to four writable u64s.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_zip64_directory(
    record: *const u8,
    len: usize,
    end_at: u64,
    out: *mut u64,
) -> bool {
    crate::ffi::guarded(false, || {
        if out.is_null() {
            return false;
        }
        // SAFETY: forwarded from this function's contract.
        match archive::zip64_directory(unsafe { bytes(record, len) }, end_at) {
            Some(d) => {
                // SAFETY: `out` has four writable u64s.
                unsafe {
                    *out = d.offset;
                    *out.add(1) = d.size;
                    *out.add(2) = d.entries;
                    *out.add(3) = d.end_at;
                }
                true
            }
            None => false,
        }
    })
}

/// Whether any entry of a zip's central directory is marked encrypted.
///
/// # Safety
/// `directory` points to `len` readable bytes (or is null with length 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_zip_encrypted(directory: *const u8, len: usize) -> bool {
    crate::ffi::guarded(true, || {
        // SAFETY: forwarded from this function's contract.
        archive::zip_directory_encrypted(unsafe { bytes(directory, len) })
    })
}

/// Where a location in an archive is: five lines, the archive file's URL,
/// the URL of the archive's top as browsed, the path inside (empty at the
/// top), the archive file's name and the name of the folder to extract it to.
/// The length is 0 when `url` isn't a location in an archive.
///
/// # Safety
/// As for `telamon_display_name`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_archive_locate(
    url: *const u8,
    len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        // SAFETY: forwarded from this function's contract.
        let url = String::from_utf8_lossy(unsafe { bytes(url, len) }).into_owned();
        let text = archive::locate(&url)
            .map(|l| {
                format!(
                    "{}\n{}\n{}\n{}\n{}",
                    l.file_url,
                    l.root_url,
                    l.inner,
                    l.name.replace('\n', " "),
                    l.folder_name.replace('\n', " ")
                )
            })
            .unwrap_or_default();
        // SAFETY: `out` as promised above.
        unsafe { put(text.as_bytes(), out, cap) }
    })
}

/// Where Up goes from a location in an archive (empty: `url` isn't one).
///
/// # Safety
/// As for `telamon_display_name`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_archive_parent(
    url: *const u8,
    len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        // SAFETY: forwarded from this function's contract.
        let url = String::from_utf8_lossy(unsafe { bytes(url, len) }).into_owned();
        let text = archive::parent(&url).unwrap_or_default();
        // SAFETY: `out` as promised above.
        unsafe { put(text.as_bytes(), out, cap) }
    })
}

/// The first free name out of `wanted`, "wanted (2)"...: `exists(ctx, name,
/// len)` says whether a name is taken. The name is written to `out`; the
/// return value is its length (more than `cap`: it did not fit).
///
/// # Safety
/// `wanted` points to `wanted_len` readable bytes (or is null with length 0);
/// `exists` is a valid function that can be called with `ctx` and a name; `out`
/// points to `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_menu_free_name(
    wanted: *const u8,
    wanted_len: usize,
    exists: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const u8, usize) -> bool>,
    ctx: *mut std::ffi::c_void,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        // SAFETY: forwarded from this function's contract.
        let wanted = String::from_utf8_lossy(unsafe { bytes(wanted, wanted_len) }).into_owned();
        let free = match exists {
            // SAFETY: the callback is valid for `ctx` and any name, as promised.
            Some(f) => menu::free_name(&wanted, |n| unsafe { f(ctx, n.as_ptr(), n.len()) }),
            None => wanted,
        };
        // SAFETY: `out` as promised above.
        unsafe { put(free.as_bytes(), out, cap) }
    })
}

/// Adds (`add`) or removes the NUL-separated `names` in the text of a
/// `.hidden` file. Returns 0 with the new text in `out` (its length in
/// `*text_len`), 1 when nothing changes, 2 when refused with the reason in
/// plain words in `out`.
///
/// # Safety
/// Each pointer pair covers its length (or is null with length 0); `out`
/// points to `cap` writable bytes (or is null); `text_len` is writable.
#[allow(clippy::too_many_arguments)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_menu_hidden(
    add: bool,
    content: *const u8,
    content_len: usize,
    names: *const u8,
    names_len: usize,
    out: *mut u8,
    cap: usize,
    text_len: *mut usize,
) -> i32 {
    crate::ffi::guarded(2, || {
        if text_len.is_null() {
            return 2;
        }
        // SAFETY: forwarded from this function's contract.
        let (content, names) = unsafe {
            (
                String::from_utf8_lossy(bytes(content, content_len)).into_owned(),
                String::from_utf8_lossy(bytes(names, names_len)).into_owned(),
            )
        };
        let names: Vec<String> = names
            .split('\0')
            .filter(|n| !n.is_empty())
            .map(str::to_string)
            .collect();
        let result = if add {
            menu::hidden_add(&content, &names)
        } else {
            menu::hidden_remove(&content, &names)
        };
        let (code, text) = match result {
            Ok(Some(t)) => (0, t),
            Ok(None) => (1, String::new()),
            Err(e) => (2, e.describe().to_string()),
        };
        // SAFETY: `out` and `text_len` as promised above.
        unsafe { *text_len = put(text.as_bytes(), out, cap) };
        code
    })
}

/// The disk usage percentage at which a drive is shown as nearly full.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_places_nearly_full() -> i32 {
    places::NEARLY_FULL_PERCENT
}

/// The tab to show after the tab at `closed` of `len` tabs is removed
/// (`current` is the one shown), or -1 when none is left or an index is out
/// of range. See `tabs::after_close`.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_tabs_after_close(len: usize, current: usize, closed: usize) -> i64 {
    tabs::after_close(len, current, closed).map_or(-1, |i| i as i64)
}

/// Where the current tab is after a tab moved from `from` to `to`.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_tabs_after_move(
    len: usize,
    current: usize,
    from: usize,
    to: usize,
) -> usize {
    tabs::after_move(len, current, from, to)
}

/// The tab `step` places from `current`, wrapping around.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_tabs_cycle(len: usize, current: usize, step: i64) -> usize {
    tabs::cycle(len, current, step)
}

/// The tab for Alt+`n`, or -1.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_tabs_jump(len: usize, n: usize) -> i64 {
    tabs::jump(len, n).map_or(-1, |i| i as i64)
}

/// Where a tab opened from `opener` goes after the `run` already opened from it.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_tabs_insert_after_opener(len: usize, opener: usize, run: usize) -> usize {
    tabs::insert_after_opener(len, opener, run)
}

/// Where a reopened tab goes.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_tabs_reopen_index(len: usize, original: usize) -> usize {
    tabs::reopen_index(len, original)
}

/// Limits the window enforces: 0 tabs in one window, 1 closed tabs kept,
/// 2 history entries kept per closed tab; 0 for anything else.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_tabs_limit(which: u32) -> usize {
    match which {
        0 => tabs::MAX_TABS,
        1 => tabs::MAX_CLOSED,
        2 => tabs::MAX_HISTORY,
        _ => 0,
    }
}

/// Checks a saved session: `saved` holds the locations, one per line. The
/// kept locations are written to `out` the same way, and the index of the tab
/// to show to `*current_out`. Returns the length of the output (more than
/// `cap` means it did not fit; nothing was written), or 0 when nothing is
/// usable. See `tabs::restore`.
///
/// # Safety
/// `saved` points to `len` readable bytes (or is null with `len` 0); `out`
/// points to `cap` writable bytes (or is null); `current_out` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_tabs_restore(
    saved: *const u8,
    len: usize,
    current: usize,
    out: *mut u8,
    cap: usize,
    current_out: *mut usize,
) -> usize {
    crate::ffi::guarded(0, || {
        if current_out.is_null() {
            return 0;
        }
        // SAFETY: forwarded from this function's contract.
        let text = String::from_utf8_lossy(unsafe { bytes(saved, len) }).into_owned();
        let lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
        let Some(session) = tabs::restore(&lines, current) else {
            return 0;
        };
        // SAFETY: `current_out` is writable (contract).
        unsafe { *current_out = session.current };
        let joined = session.urls.join("\n");
        // SAFETY: `out` as promised above.
        unsafe { put(joined.as_bytes(), out, cap) }
    })
}

#[cfg(test)]
mod guard_tests {
    use super::guarded;

    #[test]
    fn a_panic_is_contained_and_gives_the_failure_value() {
        assert_eq!(guarded(7usize, || panic!("boom")), 7);
        assert!(guarded(true, || -> bool { panic!("boom") }));
        assert_eq!(guarded(0usize, || 42), 42);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_name_reports_length_and_copies() {
        let name = b"a\nb";
        let need =
            unsafe { telamon_display_name(name.as_ptr(), name.len(), std::ptr::null_mut(), 0) };
        let mut buf = vec![0u8; need];
        let got =
            unsafe { telamon_display_name(name.as_ptr(), name.len(), buf.as_mut_ptr(), need) };
        assert_eq!(got, need);
        assert!(!String::from_utf8(buf).unwrap().contains('\n'));
    }

    #[test]
    fn validate_and_parse_report_through_the_abi() {
        let mut n = 0usize;
        let bad = b"a/b";
        let mut buf = [0u8; 256];
        let r = unsafe {
            telamon_validate_name(bad.as_ptr(), bad.len(), buf.as_mut_ptr(), buf.len(), &mut n)
        };
        assert_eq!(r, 1);
        assert!(n > 0);
        let ok = b"fine";
        let r = unsafe {
            telamon_validate_name(ok.as_ptr(), ok.len(), buf.as_mut_ptr(), buf.len(), &mut n)
        };
        assert_eq!((r, n), (0, 0));
        let t = b"/tmp/x";
        let r = unsafe {
            telamon_parse_address(
                t.as_ptr(),
                t.len(),
                std::ptr::null(),
                0,
                b"/home/u".as_ptr(),
                7,
                buf.as_mut_ptr(),
                buf.len(),
                &mut n,
            )
        };
        assert_eq!(r, 0);
        assert_eq!(&buf[..n], b"file:///tmp/x");
    }

    #[test]
    fn path_segments_report_through_the_abi() {
        let url = b"file:///home/u/a%20b";
        let home = b"/home/u";
        let need = unsafe {
            telamon_path_segments(
                url.as_ptr(),
                url.len(),
                home.as_ptr(),
                home.len(),
                std::ptr::null_mut(),
                0,
            )
        };
        let mut buf = vec![0u8; need];
        let got = unsafe {
            telamon_path_segments(
                url.as_ptr(),
                url.len(),
                home.as_ptr(),
                home.len(),
                buf.as_mut_ptr(),
                need,
            )
        };
        assert_eq!(got, need);
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "Home\tfile:///home/u\na b\tfile:///home/u/a%20b"
        );
        let none = unsafe {
            telamon_path_segments(
                b"x".as_ptr(),
                1,
                home.as_ptr(),
                home.len(),
                std::ptr::null_mut(),
                0,
            )
        };
        assert_eq!(none, 0);
    }

    #[test]
    fn completion_goes_through_the_abi() {
        let mut n = 0usize;
        let mut buf = [0u8; 256];
        let text = b"~/Do";
        let r = unsafe {
            telamon_split_for_completion(
                text.as_ptr(),
                text.len(),
                std::ptr::null(),
                0,
                b"/home/u".as_ptr(),
                7,
                buf.as_mut_ptr(),
                buf.len(),
                &mut n,
            )
        };
        assert_eq!(r, 0);
        assert_eq!(&buf[..n], b"file:///home/u\nDo");
        let bad = b"a\nb";
        let r = unsafe {
            telamon_split_for_completion(
                bad.as_ptr(),
                bad.len(),
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                buf.as_mut_ptr(),
                buf.len(),
                &mut n,
            )
        };
        assert_eq!(r, 1);

        // Records: flag byte, name, NUL.
        let entries = b"\x01Docs\0\x04docs.txt\0\x01Downloads\0\x03.hid\0\x01a\nb\0";
        let mut idx = [0u32; 8];
        let k = unsafe {
            telamon_rank_names(
                0,
                b"do".as_ptr(),
                2,
                entries.as_ptr(),
                entries.len(),
                false,
                idx.as_mut_ptr(),
                idx.len(),
            )
        };
        assert_eq!(&idx[..k], [0, 2]);
        // The menu: every folder, in natural order.
        let k = unsafe {
            telamon_rank_names(
                1,
                std::ptr::null(),
                0,
                entries.as_ptr(),
                entries.len(),
                false,
                idx.as_mut_ptr(),
                idx.len(),
            )
        };
        assert_eq!(&idx[..k], [4, 0, 2]);
        let k = unsafe {
            telamon_rank_names(
                1,
                std::ptr::null(),
                0,
                entries.as_ptr(),
                entries.len(),
                true,
                idx.as_mut_ptr(),
                idx.len(),
            )
        };
        assert_eq!(k, 4);
        // Too small a buffer: the count is reported, nothing is written.
        let mut tiny = [9u32; 1];
        let k = unsafe {
            telamon_rank_names(
                1,
                std::ptr::null(),
                0,
                entries.as_ptr(),
                entries.len(),
                false,
                tiny.as_mut_ptr(),
                1,
            )
        };
        assert_eq!((k, tiny[0]), (3, 9));
        assert_eq!(telamon_location_limit(0), location::MAX_MENU);
        assert_eq!(telamon_location_limit(9), 0);

        let t = b"smb://nas/s";
        let name = b"a b";
        let n = unsafe {
            telamon_completion_text(
                t.as_ptr(),
                t.len(),
                name.as_ptr(),
                name.len(),
                std::ptr::null(),
                0,
                buf.as_mut_ptr(),
                buf.len(),
            )
        };
        assert_eq!(&buf[..n], b"smb://nas/a%20b/");
        // A bare name in a server's folder starts from the folder.
        let n = unsafe {
            telamon_completion_text(
                b"s".as_ptr(),
                1,
                name.as_ptr(),
                name.len(),
                b"smb://nas/".as_ptr(),
                10,
                buf.as_mut_ptr(),
                buf.len(),
            )
        };
        assert_eq!(&buf[..n], b"smb://nas/a%20b/");
    }

    #[test]
    fn sort_orders_numbers_naturally() {
        let keys: Vec<Vec<u8>> = ["f10", "f9"]
            .iter()
            .map(|n| name_key(n.as_bytes()))
            .collect();
        let rows: Vec<TelamonSortRow> = keys
            .iter()
            .map(|k| TelamonSortRow {
                key: k.as_ptr(),
                key_len: k.len(),
                kind: std::ptr::null(),
                kind_len: 0,
                group: std::ptr::null(),
                group_len: 0,
                size: 0,
                mtime: 0,
                ctime: 0,
                atime: 0,
                origin: std::ptr::null(),
                origin_len: 0,
                deleted: 0,
                is_dir: false,
            })
            .collect();
        let mut out = [0u32; 2];
        assert!(unsafe {
            telamon_sort_permutation(rows.as_ptr(), 2, 0, false, true, false, out.as_mut_ptr())
        });
        assert_eq!(out, [1, 0]);
        for bad in [6, 9] {
            assert!(!unsafe {
                telamon_sort_permutation(
                    rows.as_ptr(),
                    2,
                    bad,
                    false,
                    true,
                    false,
                    out.as_mut_ptr(),
                )
            });
        }
        // The Trash's columns are accepted (rows with nothing in them keep the name order).
        for trash_column in [7, 8] {
            assert!(unsafe {
                telamon_sort_permutation(
                    rows.as_ptr(),
                    2,
                    trash_column,
                    false,
                    true,
                    false,
                    out.as_mut_ptr(),
                )
            });
            assert_eq!(out, [1, 0]);
        }
    }

    fn places_text(which: u32, a: &str, b: &str, n: u64) -> String {
        let mut buf = [0u8; 512];
        let len = unsafe {
            telamon_places_text(
                which,
                a.as_ptr(),
                a.len(),
                b.as_ptr(),
                b.len(),
                n,
                buf.as_mut_ptr(),
                buf.len(),
            )
        };
        assert!(len <= buf.len());
        String::from_utf8(buf[..len].to_vec()).unwrap()
    }

    #[test]
    fn places_report_through_the_abi() {
        let sec = |g: i32, s: &str| unsafe { telamon_places_section(g, s.as_ptr(), s.len()) };
        assert_eq!(sec(0, "file"), 0);
        assert_eq!(sec(5, "file"), 1);
        assert_eq!(sec(1, "remote"), 2);
        assert_eq!(sec(0, "trash"), 3);
        assert_eq!(sec(3, "baloosearch"), 4);
        // Kind: flags 1 device, 2 storage, 4 removable, 8 player.
        let kind =
            |sec: u32, s: &str, f: u32| unsafe { telamon_places_kind(sec, s.as_ptr(), s.len(), f) };
        assert_eq!(kind(0, "file", 0), places::Kind::Folder as u32);
        assert_eq!(kind(1, "file", 1 | 2 | 4), places::Kind::Removable as u32);
        assert_eq!(kind(1, "mtp", 1 | 8), places::Kind::Phone as u32);
        assert_eq!(kind(3, "trash", 0), places::Kind::Trash as u32);
        // Actions: flags 1 hidden, 2 mounted, 4 trash empty, 8 Disks installed.
        let removable = places::Kind::Removable as u32;
        let a = telamon_places_actions(removable, 2 | 8);
        assert!(a & places::ACT_UNMOUNT != 0 && a & places::ACT_OPEN_IN_DISKS != 0);
        assert!(telamon_places_actions(removable, 2) & places::ACT_OPEN_IN_DISKS == 0);
        assert!(telamon_places_actions(99, 0) & places::ACT_NEW_TAB != 0);
        assert_eq!(telamon_places_reorder_row(0, 3), 4);
        assert_eq!(telamon_places_reorder_row(2, 2), -1);
        assert!(unsafe { telamon_places_pinnable(b"sftp".as_ptr(), 4) });
        assert!(!unsafe { telamon_places_pinnable(b"trash".as_ptr(), 5) });
        assert!(!unsafe { telamon_places_pinnable(std::ptr::null(), 0) });
        assert_eq!(telamon_places_usage_percent(200, 50), 75);
        assert_eq!(telamon_places_usage_percent(0, 0), -1);
        assert_eq!(telamon_places_nearly_full(), places::NEARLY_FULL_PERCENT);

        assert_eq!(places_text(0, "  Work \u{7}", "", 0), "Work");
        assert_eq!(places_text(0, " ", "", 0), "");
        assert_eq!(places_text(1, "file:///home/u/Pics", "/home/u", 0), "Pics");
        assert_eq!(places_text(2, "", "", 7), "7");
        assert_eq!(places_text(3, "", "", 0), "Trash is empty");
        assert!(places_text(4, "4.2 MiB", "", 12).contains("12 items (4.2 MiB)"));
        assert_eq!(
            places_text(5, "Stick", "", places::Kind::Removable as u64),
            "Stick: Safe to remove"
        );
        assert!(places_text(6, "", "", 0).contains("telamon.disks.desktop"));
        assert!(places_text(7, "", "", 0).starts_with("telamon-disks"));
        assert_eq!(places_text(99, "", "", 0), "");
        // Too small a buffer: the size is reported, nothing is written.
        let mut tiny = [0u8; 2];
        let n = unsafe {
            telamon_places_text(
                3,
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                0,
                tiny.as_mut_ptr(),
                2,
            )
        };
        assert_eq!(n, "Trash is empty".len());
        assert_eq!(tiny, [0u8; 2]);
    }

    #[test]
    fn tab_functions_report_through_the_abi() {
        assert_eq!(telamon_tabs_after_close(3, 1, 1), 1);
        assert_eq!(telamon_tabs_after_close(1, 0, 0), -1);
        assert_eq!(telamon_tabs_after_move(3, 0, 0, 2), 2);
        assert_eq!(telamon_tabs_cycle(3, 2, 1), 0);
        assert_eq!(telamon_tabs_jump(4, 9), 3);
        assert_eq!(telamon_tabs_jump(4, 5), -1);
        assert_eq!(telamon_tabs_insert_after_opener(5, 1, 2), 4);
        assert_eq!(telamon_tabs_reopen_index(2, 7), 2);
        assert_eq!(telamon_tabs_limit(0), tabs::MAX_TABS);
        assert_eq!(telamon_tabs_limit(7), 0);
    }

    #[test]
    fn restore_checks_the_saved_locations() {
        let saved = b"file:///a\nhttp://x/\nrel\nfile:///b";
        let mut cur = 99usize;
        let mut buf = [0u8; 256];
        let n = unsafe {
            telamon_tabs_restore(
                saved.as_ptr(),
                saved.len(),
                3,
                buf.as_mut_ptr(),
                buf.len(),
                &mut cur,
            )
        };
        assert_eq!(&buf[..n], b"file:///a\nfile:///b");
        assert_eq!(cur, 1);
        // Too short a buffer: the size is reported, nothing is written.
        let mut tiny = [0u8; 4];
        let n = unsafe {
            telamon_tabs_restore(
                saved.as_ptr(),
                saved.len(),
                0,
                tiny.as_mut_ptr(),
                tiny.len(),
                &mut cur,
            )
        };
        assert_eq!(n, b"file:///a\nfile:///b".len());
        assert_eq!(tiny, [0u8; 4]);
        // Nothing usable, and null pointers.
        let bad = b"rel";
        let n = unsafe {
            telamon_tabs_restore(
                bad.as_ptr(),
                bad.len(),
                0,
                buf.as_mut_ptr(),
                buf.len(),
                &mut cur,
            )
        };
        assert_eq!(n, 0);
        let n = unsafe {
            telamon_tabs_restore(std::ptr::null(), 0, 0, std::ptr::null_mut(), 0, &mut cur)
        };
        assert_eq!(n, 0);
        let n = unsafe {
            telamon_tabs_restore(
                saved.as_ptr(),
                saved.len(),
                0,
                buf.as_mut_ptr(),
                buf.len(),
                std::ptr::null_mut(),
            )
        };
        assert_eq!(n, 0);
    }

    fn text_of(f: impl Fn(*mut u8, usize) -> usize) -> String {
        let mut buf = vec![0u8; 16];
        let n = f(buf.as_mut_ptr(), buf.len());
        if n > buf.len() {
            buf = vec![0u8; n];
            assert_eq!(f(buf.as_mut_ptr(), buf.len()), n);
        }
        String::from_utf8(buf[..n].to_vec()).unwrap()
    }

    #[test]
    fn menu_state_reports_through_the_abi() {
        // Items: a file in a writable folder on this computer.
        let flags = menu::F_WRITABLE | menu::F_LOCAL | menu::F_TERMINAL | menu::F_HIDEABLE;
        let t = text_of(|o, c| unsafe { telamon_menu_state(0, 1, 0, flags, o, c) });
        assert!(t.lines().any(|l| l == "open+"), "{t}");
        assert!(t.lines().any(|l| l == "rename+"), "{t}");
        assert!(t.lines().any(|l| l == "paste-"), "{t}");
        assert!(t.lines().any(|l| l == "openWith-"), "{t}");
        // The background.
        let t = text_of(|o, c| unsafe { telamon_menu_state(1, 0, 0, menu::F_WRITABLE, o, c) });
        assert!(t.lines().any(|l| l == "new+"), "{t}");
        assert!(t.lines().any(|l| l == "undo-"), "{t}");
        // A short buffer reports the size and writes nothing.
        let mut one = [0u8; 1];
        let n = unsafe { telamon_menu_state(1, 0, 0, menu::F_WRITABLE, one.as_mut_ptr(), 1) };
        assert!(n > 1 && one[0] == 0);
        assert!(telamon_menu_paste_into_folder(1, 1));
        assert!(!telamon_menu_paste_into_folder(2, 2));
    }

    #[test]
    fn menu_texts_and_hidden_report_through_the_abi() {
        let t = text_of(|o, c| unsafe { telamon_menu_text(0, b"Sheet.ods".as_ptr(), 9, o, c) });
        assert_eq!(t, "Sheet");
        let t = text_of(|o, c| unsafe { telamon_menu_text(1, b"Sheet.ods".as_ptr(), 9, o, c) });
        assert_eq!(t, "New Sheet.ods");
        let list = b"b.txt\0.hidden\0A.odt\0x~";
        let t = text_of(|o, c| unsafe { telamon_menu_text(2, list.as_ptr(), list.len(), o, c) });
        assert_eq!(t, "A.odt\0b.txt");
        assert_eq!(
            text_of(|o, c| unsafe { telamon_menu_text(3, std::ptr::null(), 0, o, c) }),
            "New Text File.txt"
        );
        assert!(
            text_of(|o, c| unsafe { telamon_menu_text(5, std::ptr::null(), 0, o, c) })
                .contains("archive.desktop")
        );

        let mut buf = [0u8; 64];
        let mut n = 0usize;
        let names = b"a\0b\0";
        let rc = unsafe {
            telamon_menu_hidden(
                true,
                b"x\n".as_ptr(),
                2,
                names.as_ptr(),
                names.len(),
                buf.as_mut_ptr(),
                buf.len(),
                &mut n,
            )
        };
        assert_eq!((rc, &buf[..n]), (0, &b"x\na\nb\n"[..]));
        let rc = unsafe {
            telamon_menu_hidden(
                true,
                b"a\nb\n".as_ptr(),
                4,
                names.as_ptr(),
                names.len(),
                buf.as_mut_ptr(),
                buf.len(),
                &mut n,
            )
        };
        assert_eq!(rc, 1);
        let bad = b"a/b\0";
        let rc = unsafe {
            telamon_menu_hidden(
                true,
                std::ptr::null(),
                0,
                bad.as_ptr(),
                bad.len(),
                buf.as_mut_ptr(),
                buf.len(),
                &mut n,
            )
        };
        assert_eq!(rc, 2);
        assert!(n > 0);
        let rc = unsafe {
            telamon_menu_hidden(
                true,
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
            )
        };
        assert_eq!(rc, 2);
    }

    unsafe extern "C" fn taken_if_new(
        _ctx: *mut std::ffi::c_void,
        name: *const u8,
        len: usize,
    ) -> bool {
        // SAFETY: the caller passes `len` readable bytes.
        let n = unsafe { std::slice::from_raw_parts(name, len) };
        n == b"New Folder" || n == b"New Folder (2)"
    }

    #[test]
    fn free_name_asks_the_callback() {
        let t = text_of(|o, c| unsafe {
            telamon_menu_free_name(
                b"New Folder".as_ptr(),
                10,
                Some(taken_if_new),
                std::ptr::null_mut(),
                o,
                c,
            )
        });
        assert_eq!(t, "New Folder (3)");
        let t = text_of(|o, c| unsafe {
            telamon_menu_free_name(b"Other".as_ptr(), 5, None, std::ptr::null_mut(), o, c)
        });
        assert_eq!(t, "Other");
    }

    #[test]
    fn archives_report_through_the_abi() {
        // The record format of `telamon_archive_check`: `f` path NUL, or `l` path NUL target NUL.
        let check = |records: &[u8], installed: bool| {
            let mut buf = [0u8; 512];
            let mut n = 0usize;
            let rc = unsafe {
                telamon_archive_check(
                    records.as_ptr(),
                    records.len(),
                    installed,
                    buf.as_mut_ptr(),
                    buf.len(),
                    &mut n,
                )
            };
            (
                rc,
                String::from_utf8_lossy(&buf[..n.min(buf.len())]).into_owned(),
            )
        };
        assert_eq!(
            check(b"fa.txt\0fd/b.txt\0lalias\0a.txt\0", true),
            (0, String::new())
        );
        let (rc, text) = check(b"fok\0f../../evil.txt\0", false);
        assert_eq!(rc, 1);
        assert!(
            text.contains("evil.txt") && text.contains("won't extract"),
            "{text}"
        );
        assert!(!text.contains("Telamon Archive"), "{text}");
        let (rc, text) = check(b"fok\0llink\0/etc\0", true);
        assert_eq!(rc, 1);
        assert!(
            text.contains("is a link") && text.contains("Telamon Archive"),
            "{text}"
        );
        // A record that is cut off is refused, not skipped.
        assert_eq!(check(b"fno-end", false).0, 1);
        assert_eq!(
            unsafe {
                telamon_archive_check(
                    std::ptr::null(),
                    0,
                    false,
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                )
            },
            1
        );

        let locate = |url: &str| {
            let mut buf = [0u8; 512];
            let n = unsafe {
                telamon_archive_locate(url.as_ptr(), url.len(), buf.as_mut_ptr(), buf.len())
            };
            String::from_utf8_lossy(&buf[..n]).into_owned()
        };
        assert_eq!(
            locate("zip:/home/u/a%20b.zip/sub"),
            "file:///home/u/a%20b.zip\nzip:/home/u/a%20b.zip\n/sub\na b.zip\na b"
        );
        assert_eq!(locate("file:///home/u"), "");
        let parent = |url: &str| {
            let mut buf = [0u8; 512];
            let n = unsafe {
                telamon_archive_parent(url.as_ptr(), url.len(), buf.as_mut_ptr(), buf.len())
            };
            String::from_utf8_lossy(&buf[..n]).into_owned()
        };
        assert_eq!(parent("zip:/home/u/a.zip"), "file:///home/u");
        assert_eq!(parent("file:///home/u"), "");
        for (scheme, yes) in [
            ("zip", true),
            ("sevenz", true),
            ("file", false),
            ("", false),
        ] {
            assert_eq!(
                unsafe { telamon_archive_is_scheme(scheme.as_ptr(), scheme.len()) },
                yes,
                "{scheme}"
            );
        }
        // Not a zip: no directory, and nothing encrypted.
        let mut out = [0u64; 4];
        assert_eq!(
            unsafe { telamon_zip_end(b"hello".as_ptr(), 5, 5, out.as_mut_ptr()) },
            0
        );
        assert!(!unsafe { telamon_zip_encrypted(b"hello".as_ptr(), 5) });
        assert_eq!(
            unsafe { telamon_zip_end(std::ptr::null(), 0, 0, out.as_mut_ptr()) },
            0
        );
        assert!(!unsafe { telamon_zip64_directory(b"no".as_ptr(), 2, 0, out.as_mut_ptr()) });
    }
}
