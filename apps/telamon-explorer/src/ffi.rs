//! C ABI over `atlas_explorer_core` for the C++ adapters (`cpp/kio/*`): display
//! names, sort keys and the sort permutation. The logic stays in the core
//! crate; these functions only move bytes. Every pointer is checked for null.

use atlas_explorer_core::display_name;
use atlas_explorer_core::sort::{Column, SortRow, name_key, sort_permutation};
use atlas_explorer_core::{address, location, names, tabs};

/// One row for `telamon_sort_permutation`; the C++ twin is in FolderModel.cpp.
#[repr(C)]
pub struct TelamonSortRow {
    pub key: *const u8,
    pub key_len: usize,
    pub kind: *const u8,
    pub kind_len: usize,
    pub size: u64,
    pub mtime: i64,
    pub ctime: i64,
    pub atime: i64,
    pub is_dir: bool,
}

/// # Safety
/// `ptr` is null or points to `len` readable bytes.
unsafe fn bytes<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
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
unsafe fn put(data: &[u8], out: *mut u8, cap: usize) -> usize {
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
    // SAFETY: forwarded from this function's contract.
    unsafe {
        let s = display_name(bytes(name, len));
        put(s.as_bytes(), out, cap)
    }
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
    // SAFETY: forwarded from this function's contract.
    unsafe {
        let k = name_key(bytes(name, len));
        put(&k, out, cap)
    }
}

/// Sorts `n` rows by `column` (0 Name, 1 Size, 2 Type, 3 Modified, 4 Created,
/// 5 Accessed) and writes the permutation to `out` (`n` u32s). False on bad input.
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
    out: *mut u32,
) -> bool {
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
            is_dir: r.is_dir,
            size: r.size,
            mtime: r.mtime,
            ctime: r.ctime,
            atime: r.atime,
        })
        .collect();
    let perm = sort_permutation(&rows, column, descending, folders_first);
    // SAFETY: `out` has `n` writable u32s and `perm.len() == n`.
    unsafe { std::ptr::copy_nonoverlapping(perm.as_ptr(), out, perm.len()) };
    true
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
                size: 0,
                mtime: 0,
                ctime: 0,
                atime: 0,
                is_dir: false,
            })
            .collect();
        let mut out = [0u32; 2];
        assert!(unsafe {
            telamon_sort_permutation(rows.as_ptr(), 2, 0, false, true, out.as_mut_ptr())
        });
        assert_eq!(out, [1, 0]);
        assert!(!unsafe {
            telamon_sort_permutation(rows.as_ptr(), 2, 9, false, true, out.as_mut_ptr())
        });
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
}
