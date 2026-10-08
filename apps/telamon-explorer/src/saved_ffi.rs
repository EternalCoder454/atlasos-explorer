//! C ABI for the saved searches (`cpp/kio/SavedLogic.*`): the list is text
//! (see `atlas_explorer_core::saved`), read and changed here and kept by the
//! window in the settings file. Every call takes the list as it was last kept
//! and gives the new one.

use crate::ffi::{bytes, put};
use atlas_explorer_core::saved::{self, Outcome, SavedList};

fn text_of(ptr: *const u8, len: usize) -> String {
    // SAFETY: callers pass `len` readable bytes (their contracts).
    String::from_utf8_lossy(unsafe { bytes(ptr, len) }).into_owned()
}

fn status_of(o: Outcome) -> u32 {
    match o {
        Outcome::Done => 0,
        Outcome::Invalid => 1,
        Outcome::Full => 2,
        Outcome::Missing => 3,
    }
}

/// The most saved searches, the longest name (characters) and the longest words (bytes).
#[unsafe(no_mangle)]
pub extern "C" fn telamon_saved_limit(which: u32) -> usize {
    match which {
        0 => saved::MAX_SAVED,
        1 => saved::MAX_NAME_CHARS,
        _ => saved::MAX_QUERY_BYTES,
    }
}

/// The list as it should be kept: every line that is fine, the rest dropped.
///
/// # Safety
/// `list` covers `list_len` readable bytes (or is null with 0); `out` points
/// to `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_saved_clean(
    list: *const u8,
    list_len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    let l = SavedList::parse(&text_of(list, list_len));
    // SAFETY: `out` as promised.
    unsafe { put(l.to_text().as_bytes(), out, cap) }
}

/// Adds a search (`record`: the line without its id) at the end. The new list
/// goes to `out`; `*status` is 0 added, 1 refused (no name, nothing to look
/// for, no folder), 2 full. On a refusal the list comes back unchanged.
///
/// # Safety
/// Each pointer pair covers its length (or is null with 0); `out` points to
/// `cap` writable bytes (or is null); `status` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_saved_add(
    list: *const u8,
    list_len: usize,
    record: *const u8,
    record_len: usize,
    out: *mut u8,
    cap: usize,
    status: *mut u32,
) -> usize {
    let mut l = SavedList::parse(&text_of(list, list_len));
    let st = match saved::parse_record(&text_of(record, record_len)) {
        None => Outcome::Invalid,
        Some(s) => match l.add(s) {
            Ok(_) => Outcome::Done,
            Err(o) => o,
        },
    };
    if !status.is_null() {
        // SAFETY: writable (contract).
        unsafe { status.write(status_of(st)) };
    }
    // SAFETY: `out` as promised.
    unsafe { put(l.to_text().as_bytes(), out, cap) }
}

/// Renames the search with `id`. `*status`: 0 done, 1 no usable name, 3 no such search.
///
/// # Safety
/// As `telamon_saved_add`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_saved_rename(
    list: *const u8,
    list_len: usize,
    id: u32,
    name: *const u8,
    name_len: usize,
    out: *mut u8,
    cap: usize,
    status: *mut u32,
) -> usize {
    let mut l = SavedList::parse(&text_of(list, list_len));
    let st = l.rename(id, &text_of(name, name_len));
    if !status.is_null() {
        // SAFETY: writable (contract).
        unsafe { status.write(status_of(st)) };
    }
    // SAFETY: `out` as promised.
    unsafe { put(l.to_text().as_bytes(), out, cap) }
}

/// Removes the search with `id`.
///
/// # Safety
/// As `telamon_saved_add`, without `status`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_saved_remove(
    list: *const u8,
    list_len: usize,
    id: u32,
    out: *mut u8,
    cap: usize,
) -> usize {
    let mut l = SavedList::parse(&text_of(list, list_len));
    l.remove(id);
    // SAFETY: `out` as promised.
    unsafe { put(l.to_text().as_bytes(), out, cap) }
}

/// A name to offer for the search `record` describes (the line without its id).
///
/// # Safety
/// `record` covers `len` readable bytes (or is null with 0); `out` points to
/// `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_saved_default_name(
    record: *const u8,
    len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    let name = saved::parse_record(&text_of(record, len))
        .map_or_else(|| "Saved Search".to_string(), |s| saved::default_name(&s));
    // SAFETY: `out` as promised.
    unsafe { put(name.as_bytes(), out, cap) }
}

/// What the search with `id` does, in a line (`folder_label`: its folder as places are written).
///
/// # Safety
/// Each pointer pair covers its length (or is null with 0); `out` points to
/// `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_saved_describe(
    list: *const u8,
    list_len: usize,
    id: u32,
    folder_label: *const u8,
    label_len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    let l = SavedList::parse(&text_of(list, list_len));
    let t = l.get(id).map_or_else(String::new, |s| {
        saved::describe(s, &text_of(folder_label, label_len))
    });
    // SAFETY: `out` as promised.
    unsafe { put(t.as_bytes(), out, cap) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call_add(list: &str, record: &str) -> (String, u32) {
        let mut buf = vec![0u8; 8192];
        let mut st = 9u32;
        let n = unsafe {
            telamon_saved_add(
                list.as_ptr(),
                list.len(),
                record.as_ptr(),
                record.len(),
                buf.as_mut_ptr(),
                buf.len(),
                &mut st,
            )
        };
        (String::from_utf8(buf[..n].to_vec()).unwrap(), st)
    }

    #[test]
    fn add_rename_remove_through_the_abi() {
        let (l, st) = call_add("", "Reports\treport\t1\t\t0\t0\t0\t\t0\t0");
        assert_eq!(st, 0);
        assert_eq!(l.lines().count(), 1);
        let (l2, st) = call_add(&l, "\tnothing\t1\t\t0\t0\t0\t\t0\t0");
        assert_eq!((st, l2.as_str()), (1, l.as_str()));
        let (l3, st) = call_add(&l, "garbage");
        assert_eq!((st, l3.as_str()), (1, l.as_str()));
        let (l4, st) = call_add(&l, "More\tmore\t1\t\t0\t0\t0\t\t0\t0");
        assert_eq!((st, l4.lines().count()), (0, 2));

        let mut buf = vec![0u8; 8192];
        let mut st = 9u32;
        let name = "Old reports";
        let n = unsafe {
            telamon_saved_rename(
                l4.as_ptr(),
                l4.len(),
                1,
                name.as_ptr(),
                name.len(),
                buf.as_mut_ptr(),
                buf.len(),
                &mut st,
            )
        };
        assert_eq!(st, 0);
        let renamed = String::from_utf8(buf[..n].to_vec()).unwrap();
        assert!(renamed.starts_with("1\tOld reports\t"), "{renamed}");
        let n = unsafe {
            telamon_saved_rename(
                renamed.as_ptr(),
                renamed.len(),
                77,
                name.as_ptr(),
                name.len(),
                buf.as_mut_ptr(),
                buf.len(),
                &mut st,
            )
        };
        assert_eq!((st, n), (3, renamed.len()));
        let n = unsafe {
            telamon_saved_remove(
                renamed.as_ptr(),
                renamed.len(),
                1,
                buf.as_mut_ptr(),
                buf.len(),
            )
        };
        let left = String::from_utf8(buf[..n].to_vec()).unwrap();
        assert_eq!(left.lines().count(), 1);
        assert!(left.starts_with("2\tMore\t"));
        // Clean drops damage.
        let dirty = format!("{left}junk\n");
        let n = unsafe {
            telamon_saved_clean(dirty.as_ptr(), dirty.len(), buf.as_mut_ptr(), buf.len())
        };
        assert_eq!(String::from_utf8(buf[..n].to_vec()).unwrap(), left);
        let n = unsafe {
            telamon_saved_describe(
                left.as_ptr(),
                left.len(),
                2,
                b"~/Docs".as_ptr(),
                6,
                buf.as_mut_ptr(),
                buf.len(),
            )
        };
        assert!(String::from_utf8_lossy(&buf[..n]).contains("Everywhere"));
    }
}
