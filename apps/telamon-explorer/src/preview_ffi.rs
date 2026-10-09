//! C ABI for Quick Look, the preview pane and the zoom keys
//! (`cpp/kio/PreviewLoader.*`, `cpp/kio/PreviewLogic.*`). The decisions are in
//! `atlas_explorer_core::preview` and `::zoom`; these functions only move bytes.

use crate::ffi::{bytes, put};
use atlas_explorer_core::preview::{self, TEXT_CAP};
use atlas_explorer_core::zoom::{self, Kind};
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

/// The category (`preview::Category`) of a MIME type name, as its number.
///
/// # Safety
/// `mime` points to `len` readable bytes (or is null with `len` 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_preview_classify(mime: *const u8, len: usize) -> u32 {
    // SAFETY: forwarded from this function's contract.
    let m = unsafe { bytes(mime, len) };
    preview::classify_mime(&String::from_utf8_lossy(m)) as u32
}

/// Bytes of a text file that are shown.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_preview_text_cap() -> usize {
    TEXT_CAP
}

/// Reads the start of the file at `path` as text that is safe to show (see
/// `preview::read_text`), at most `TEXT_CAP` bytes of it read and `cap` bytes
/// written to `out`. `*status` is the outcome (0 text, 1 binary, 2 not a
/// regular file, 3 unreadable) and `*truncated` says the file has more. Returns
/// the length written.
///
/// # Safety
/// `path` points to `path_len` readable bytes; `out` to `cap` writable bytes
/// (or is null); `status` and `truncated` to writable memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_preview_read_text(
    path: *const u8,
    path_len: usize,
    out: *mut u8,
    cap: usize,
    status: *mut u32,
    truncated: *mut bool,
) -> usize {
    crate::ffi::guarded(0, || {
        if status.is_null() || truncated.is_null() {
            return 0;
        }
        // SAFETY: forwarded from this function's contract.
        let p = unsafe { bytes(path, path_len) };
        if p.is_empty() || p.contains(&0) {
            // SAFETY: both are writable (checked non-null above).
            unsafe {
                *status = preview::TextOutcome::Unreadable as u32;
                *truncated = false;
            }
            return 0;
        }
        let t = preview::read_text(Path::new(OsStr::from_bytes(p)), TEXT_CAP, cap);
        // SAFETY: as above; `out` has `cap` writable bytes and the text is at most `cap` long.
        unsafe {
            *status = t.outcome as u32;
            *truncated = t.truncated;
            put(t.text.as_bytes(), out, cap)
        }
    })
}

/// Text for the details: 0 a duration in milliseconds (`n`), 1 dimensions
/// (`n` is the width in the high 32 bits and the height in the low 32).
///
/// # Safety
/// `out` points to `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_preview_text(
    which: u32,
    n: u64,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        let s = match which {
            0 => preview::format_duration(n),
            1 => preview::format_dimensions((n >> 32) as u32, n as u32),
            _ => String::new(),
        };
        // SAFETY: forwarded from this function's contract.
        unsafe { put(s.as_bytes(), out, cap) }
    })
}

/// A saved size brought into the limits. `kind`: 0 icons, 1 rows. Another
/// kind gives 0.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_zoom_clamp(kind: u32, value: i32) -> i32 {
    Kind::from_code(kind).map_or(0, |k| zoom::clamp(k, value))
}

/// `current` moved by `steps` within the limits.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_zoom_step(kind: u32, current: i32, steps: i32) -> i32 {
    Kind::from_code(kind).map_or(0, |k| zoom::step(k, current, steps))
}

/// The size when none is saved; `default_row` is the rows' (two grid units).
#[unsafe(no_mangle)]
pub extern "C" fn telamon_zoom_default(kind: u32, default_row: i32) -> i32 {
    Kind::from_code(kind).map_or(0, |k| zoom::default_size(k, default_row))
}

/// Wheel movement into whole steps; the rest to keep goes to `*rest`.
///
/// # Safety
/// `rest` points to a writable i32 (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_zoom_wheel(pending: i32, delta: i32, rest: *mut i32) -> i32 {
    let (steps, left) = zoom::wheel_steps(pending, delta);
    if !rest.is_null() {
        // SAFETY: non-null and writable by the contract.
        unsafe { *rest = left };
    }
    steps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_and_zoom_cross_the_boundary() {
        let m = b"image/png";
        assert_eq!(
            unsafe { telamon_preview_classify(m.as_ptr(), m.len()) },
            preview::Category::Image as u32
        );
        assert_eq!(
            unsafe { telamon_preview_classify(std::ptr::null(), 0) },
            preview::Category::Other as u32
        );
        assert_eq!(telamon_zoom_step(0, 96, 1), 112);
        assert_eq!(telamon_zoom_step(1, 36, -1), 32);
        assert_eq!(telamon_zoom_step(9, 36, 1), 0);
        assert_eq!(telamon_zoom_clamp(0, 9999), zoom::ICON_MAX);
        assert_eq!(telamon_zoom_default(1, 36), 36);
        let mut rest = 0;
        assert_eq!(unsafe { telamon_zoom_wheel(60, 120, &mut rest) }, 1);
        assert_eq!(rest, 60);
        assert_eq!(
            unsafe { telamon_zoom_wheel(0, 120, std::ptr::null_mut()) },
            1
        );
    }

    #[test]
    fn read_text_reports_its_outcome() {
        let dir = std::env::temp_dir().join(format!("telamon-previewffi-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.txt");
        std::fs::write(&file, "hi\u{202E}there\n").unwrap();
        let p = file.as_os_str().as_bytes();
        let mut buf = [0u8; 256];
        let (mut status, mut truncated) = (9u32, true);
        let n = unsafe {
            telamon_preview_read_text(
                p.as_ptr(),
                p.len(),
                buf.as_mut_ptr(),
                buf.len(),
                &mut status,
                &mut truncated,
            )
        };
        assert_eq!((status, truncated), (0, false));
        let text = std::str::from_utf8(&buf[..n]).unwrap();
        assert!(text.starts_with("hi") && text.contains("U+202E") && text.ends_with("there\n"));
        // a folder, a missing path, and no output room at all
        let d = dir.as_os_str().as_bytes();
        unsafe {
            telamon_preview_read_text(
                d.as_ptr(),
                d.len(),
                buf.as_mut_ptr(),
                256,
                &mut status,
                &mut truncated,
            );
        }
        assert_eq!(status, preview::TextOutcome::NotRegular as u32);
        let none = b"/nonexistent/telamon/x";
        unsafe {
            telamon_preview_read_text(
                none.as_ptr(),
                none.len(),
                buf.as_mut_ptr(),
                256,
                &mut status,
                &mut truncated,
            );
        }
        assert_eq!(status, preview::TextOutcome::Unreadable as u32);
        assert_eq!(
            unsafe {
                telamon_preview_read_text(
                    p.as_ptr(),
                    p.len(),
                    std::ptr::null_mut(),
                    0,
                    &mut status,
                    &mut truncated,
                )
            },
            0
        );
        assert!(truncated);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn details_texts() {
        let mut buf = [0u8; 32];
        let n = unsafe { telamon_preview_text(0, 125_000, buf.as_mut_ptr(), buf.len()) };
        assert_eq!(&buf[..n], b"2:05");
        let n =
            unsafe { telamon_preview_text(1, (640u64 << 32) | 480, buf.as_mut_ptr(), buf.len()) };
        assert_eq!(std::str::from_utf8(&buf[..n]).unwrap(), "640 \u{00D7} 480");
        assert_eq!(
            unsafe { telamon_preview_text(7, 1, buf.as_mut_ptr(), buf.len()) },
            0
        );
    }
}
