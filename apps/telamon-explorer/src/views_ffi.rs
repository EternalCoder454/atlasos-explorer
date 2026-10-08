//! C ABI for the views: grouping ("Group by") and what each folder remembers
//! of how it is shown. The logic is in the core (`group`, `views`); these
//! functions move bytes. Text goes in as pointer and length and comes out in
//! a caller's buffer; the return value is the length, so a short buffer can be
//! retried with the right size.

use crate::ffi::{bytes, put};
use atlas_explorer_core::group::{self, Clock, GroupBy, GroupInput};
use atlas_explorer_core::sort::Column;
use atlas_explorer_core::views::{self, FolderViews, Mode, ViewPrefs};

/// The group of a row, written as its order key, a 0 byte and its label.
/// `by` is 1 Name, 2 Type, 3 Modified (0 and anything else write nothing).
/// `week_start` is 0 for Monday to 6 for Sunday; `tz` is seconds east of UTC.
///
/// # Safety
/// `key` and `kind` point to their lengths of readable bytes (or are null
/// with length 0); `out` points to `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_group_of(
    by: u32,
    key: *const u8,
    key_len: usize,
    kind: *const u8,
    kind_len: usize,
    is_dir: bool,
    mtime: i64,
    now: i64,
    tz: i64,
    week_start: u32,
    out: *mut u8,
    cap: usize,
) -> usize {
    let Some(by) = GroupBy::from_code(by).filter(|b| *b != GroupBy::None) else {
        return 0;
    };
    // SAFETY: forwarded from this function's contract.
    let (key, kind) = unsafe { (bytes(key, key_len), bytes(kind, kind_len)) };
    let kind = String::from_utf8_lossy(kind);
    let input = GroupInput {
        key,
        is_dir,
        kind: &kind,
        mtime,
    };
    let (mut order, label) = group::group_of(
        by,
        &input,
        Clock {
            now,
            tz,
            week_start,
        },
    );
    order.push(0);
    order.extend_from_slice(label.as_bytes());
    // SAFETY: forwarded from this function's contract.
    unsafe { put(&order, out, cap) }
}

/// Whether the groups come in the opposite order of their keys for this sort
/// (`column` as for `telamon_sort_permutation`).
#[unsafe(no_mangle)]
pub extern "C" fn telamon_group_reversed(by: u32, column: u32, descending: bool) -> bool {
    let (Some(by), Some(column)) = (GroupBy::from_code(by), column_of(column)) else {
        return false;
    };
    group::reversed(by, column, descending)
}

fn column_of(code: u32) -> Option<Column> {
    Some(match code {
        0 => Column::Name,
        1 => Column::Size,
        2 => Column::Type,
        3 => Column::Modified,
        4 => Column::Created,
        5 => Column::Accessed,
        _ => return None,
    })
}

/// One folder's way of being shown, as `views::ViewPrefs`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct TelamonViewPrefs {
    /// 0 Details, 1 Icons, 2 Compact, 3 Columns, 4 Gallery.
    pub mode: u32,
    pub sort: u32,
    pub descending: bool,
    pub icon: i32,
    /// 0 None, 1 Name, 2 Type, 3 Modified.
    pub group: u32,
}

impl TelamonViewPrefs {
    fn from_core(p: ViewPrefs) -> Self {
        TelamonViewPrefs {
            mode: p.mode as u32,
            sort: p.sort,
            descending: p.descending,
            icon: p.icon,
            group: p.group as u32,
        }
    }
    fn to_core(self) -> ViewPrefs {
        ViewPrefs {
            mode: Mode::from_code(self.mode).unwrap_or_default(),
            sort: self.sort,
            descending: self.descending,
            icon: self.icon,
            group: GroupBy::from_code(self.group).unwrap_or_default(),
        }
        .sanitized()
    }
}

fn list(saved: *const u8, len: usize) -> FolderViews {
    // SAFETY: callers pass what their own contract promises.
    FolderViews::parse(&String::from_utf8_lossy(unsafe { bytes(saved, len) }))
}

fn key_of<'a>(key: *const u8, len: usize) -> std::borrow::Cow<'a, str> {
    // SAFETY: callers pass what their own contract promises.
    String::from_utf8_lossy(unsafe { bytes(key, len) })
}

/// What the saved list remembers for `key`; false when nothing.
///
/// # Safety
/// Each pointer covers its length (or is null with length 0); `out` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_views_get(
    saved: *const u8,
    saved_len: usize,
    key: *const u8,
    key_len: usize,
    out: *mut TelamonViewPrefs,
) -> bool {
    if out.is_null() {
        return false;
    }
    match list(saved, saved_len).get(&key_of(key, key_len)) {
        Some(p) => {
            // SAFETY: `out` is writable (contract).
            unsafe { *out = TelamonViewPrefs::from_core(p) };
            true
        }
        None => false,
    }
}

/// The saved list with `key` remembered as most recent. A key that cannot be
/// kept leaves the list as it was (but cleaned up).
///
/// # Safety
/// Each pointer covers its length (or is null with length 0); `prefs` is
/// readable; `out` points to `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_views_set(
    saved: *const u8,
    saved_len: usize,
    key: *const u8,
    key_len: usize,
    prefs: *const TelamonViewPrefs,
    out: *mut u8,
    cap: usize,
) -> usize {
    let mut v = list(saved, saved_len);
    if !prefs.is_null() {
        // SAFETY: `prefs` is readable (contract).
        let p = unsafe { *prefs }.to_core();
        v.set(&key_of(key, key_len), p);
    }
    // SAFETY: `out` as promised.
    unsafe { put(v.to_text().as_bytes(), out, cap) }
}

/// The saved list without `key`.
///
/// # Safety
/// As for `telamon_views_set`, without `prefs`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_views_forget(
    saved: *const u8,
    saved_len: usize,
    key: *const u8,
    key_len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    let mut v = list(saved, saved_len);
    v.forget(&key_of(key, key_len));
    // SAFETY: `out` as promised.
    unsafe { put(v.to_text().as_bytes(), out, cap) }
}

/// Whether `key` can be a folder's key.
///
/// # Safety
/// `key` covers its length (or is null with length 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_views_valid_key(key: *const u8, key_len: usize) -> bool {
    views::valid_key(&key_of(key, key_len))
}

/// A number of the views: 0 the most folders remembered, 1 the longest key.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_views_limit(which: u32) -> usize {
    match which {
        0 => views::MAX_FOLDERS,
        1 => views::MAX_KEY,
        _ => 0,
    }
}

/// The window's name of a view mode ("details", "icons", ...); 0 bytes for none.
///
/// # Safety
/// `out` points to `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_views_mode_name(mode: u32, out: *mut u8, cap: usize) -> usize {
    match Mode::from_code(mode) {
        // SAFETY: `out` as promised.
        Some(m) => unsafe { put(m.name().as_bytes(), out, cap) },
        None => 0,
    }
}

/// The code of a view mode's name; -1 for a name that is none.
///
/// # Safety
/// `name` covers its length (or is null with length 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_views_mode_code(name: *const u8, len: usize) -> i32 {
    Mode::from_name(&key_of(name, len)).map_or(-1, |m| m as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_of_writes_key_zero_label() {
        let key = atlas_explorer_core::sort::name_key(b"apple");
        let mut buf = [0u8; 64];
        let n = unsafe {
            telamon_group_of(
                1,
                key.as_ptr(),
                key.len(),
                std::ptr::null(),
                0,
                false,
                0,
                0,
                0,
                0,
                buf.as_mut_ptr(),
                buf.len(),
            )
        };
        let out = &buf[..n];
        let zero = out.iter().position(|&b| b == 0).unwrap();
        assert_eq!(&out[zero + 1..], b"A");
        // none and unknown write nothing; a short buffer reports the size
        for by in [0, 9] {
            assert_eq!(
                unsafe {
                    telamon_group_of(
                        by,
                        key.as_ptr(),
                        key.len(),
                        std::ptr::null(),
                        0,
                        false,
                        0,
                        0,
                        0,
                        0,
                        buf.as_mut_ptr(),
                        buf.len(),
                    )
                },
                0
            );
        }
        let need = unsafe {
            telamon_group_of(
                1,
                key.as_ptr(),
                key.len(),
                std::ptr::null(),
                0,
                false,
                0,
                0,
                0,
                0,
                std::ptr::null_mut(),
                0,
            )
        };
        assert_eq!(need, n);
        assert!(telamon_group_reversed(1, 0, true));
        assert!(!telamon_group_reversed(1, 1, true));
        assert!(!telamon_group_reversed(1, 99, true));
    }

    #[test]
    fn views_round_trip_through_the_bridge() {
        let key = b"file:///home/a";
        let prefs = TelamonViewPrefs {
            mode: 4,
            sort: 3,
            descending: true,
            icon: 130,
            group: 3,
        };
        let mut text = vec![0u8; 256];
        let n = unsafe {
            telamon_views_set(
                std::ptr::null(),
                0,
                key.as_ptr(),
                key.len(),
                &prefs,
                text.as_mut_ptr(),
                text.len(),
            )
        };
        text.truncate(n);
        let mut got = TelamonViewPrefs {
            mode: 0,
            sort: 0,
            descending: false,
            icon: 0,
            group: 0,
        };
        assert!(unsafe {
            telamon_views_get(text.as_ptr(), text.len(), key.as_ptr(), key.len(), &mut got)
        });
        assert_eq!(
            (got.mode, got.sort, got.descending, got.group),
            (4, 3, true, 3)
        );
        // 130 is within the limits and is kept as given
        assert_eq!(got.icon, 130);
        let mut gone = vec![0u8; 256];
        let m = unsafe {
            telamon_views_forget(
                text.as_ptr(),
                text.len(),
                key.as_ptr(),
                key.len(),
                gone.as_mut_ptr(),
                gone.len(),
            )
        };
        assert_eq!(m, 0);
        assert!(!unsafe { telamon_views_get(gone.as_ptr(), 0, key.as_ptr(), key.len(), &mut got) });
        assert!(unsafe { telamon_views_valid_key(key.as_ptr(), key.len()) });
        assert!(!unsafe { telamon_views_valid_key(b"a\nb".as_ptr(), 3) });
        assert_eq!(telamon_views_limit(0), views::MAX_FOLDERS);
    }

    #[test]
    fn mode_names() {
        let mut buf = [0u8; 16];
        let n = unsafe { telamon_views_mode_name(3, buf.as_mut_ptr(), buf.len()) };
        assert_eq!(&buf[..n], b"columns");
        assert_eq!(
            unsafe { telamon_views_mode_code(b"gallery".as_ptr(), 7) },
            4
        );
        assert_eq!(unsafe { telamon_views_mode_code(b"nope".as_ptr(), 4) }, -1);
        assert_eq!(
            unsafe { telamon_views_mode_name(9, buf.as_mut_ptr(), 16) },
            0
        );
    }
}
