//! C ABI over `atlas_explorer_core` for the C++ adapters (`cpp/kio/*`): display
//! names, sort keys and the sort permutation. The logic stays in the core
//! crate; these functions only move bytes. Every pointer is checked for null.

use atlas_explorer_core::display_name;
use atlas_explorer_core::sort::{Column, SortRow, name_key, sort_permutation};
use atlas_explorer_core::{address, names};

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
}
