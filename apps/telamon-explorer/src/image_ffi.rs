//! C ABI for the quick actions on pictures (More Actions: Rotate, Convert,
//! Combine into PDF). The decisions are in the core (`imageops`, `pdfmerge`);
//! these functions move bytes. The pictures themselves are read and written
//! by Qt in `cpp/kio/ImageWork.cpp`, on a worker.

use crate::ffi::{bytes, put};
use atlas_explorer_core::imageops::{self, Action, Kind};
use atlas_explorer_core::pdfmerge;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

fn kind_code(kind: Option<Kind>) -> u32 {
    match kind {
        None => 0,
        Some(Kind::Jpeg) => 1,
        Some(Kind::Png) => 2,
        Some(Kind::Webp) => 3,
        Some(Kind::Bmp) => 4,
        Some(Kind::OtherRaster) => 5,
        Some(Kind::Pdf) => 6,
    }
}

fn kind_from(code: u32) -> Option<Kind> {
    Some(match code {
        1 => Kind::Jpeg,
        2 => Kind::Png,
        3 => Kind::Webp,
        4 => Kind::Bmp,
        5 => Kind::OtherRaster,
        6 => Kind::Pdf,
        _ => return None,
    })
}

/// What a MIME type is for the picture actions: 0 nothing, 1 JPEG, 2 PNG,
/// 3 WebP, 4 BMP, 5 another picture Qt may read, 6 PDF.
///
/// # Safety
/// `mime` points to `len` readable bytes (or is null with `len` 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_image_kind(mime: *const u8, len: usize) -> u32 {
    // SAFETY: forwarded from this function's contract.
    let text = String::from_utf8_lossy(unsafe { bytes(mime, len) });
    kind_code(imageops::kind_of_mime(&text))
}

/// Whether action `action` (0 Rotate Left, 1 Rotate Right, 2 PNG, 3 JPEG,
/// 4 WebP, 5 Combine) takes a file of kind `kind`.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_image_accepts(action: u32, kind: u32) -> bool {
    match (Action::from_code(action), kind_from(kind)) {
        (Some(a), Some(k)) => a.accepts(k),
        _ => false,
    }
}

/// Whether the action leaves a file of this kind as it is (already that format).
#[unsafe(no_mangle)]
pub extern "C" fn telamon_image_skips(action: u32, kind: u32) -> bool {
    match (Action::from_code(action), kind_from(kind)) {
        (Some(a), Some(k)) => a.skips(k),
        _ => false,
    }
}

/// The names of the files an action makes. `kinds` has `count` kind codes;
/// `names` holds `count` file names, each ended by a 0 byte. The result is
/// the new names in the same form. Returns the length written (retry with a
/// bigger buffer when it exceeds `cap`); 0 on a bad argument.
///
/// # Safety
/// `kinds` points to `count` readable `u32`; `names` to `names_len` readable
/// bytes; `out` to `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_image_names(
    action: u32,
    kinds: *const u32,
    count: usize,
    names: *const u8,
    names_len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        let Some(action) = Action::from_code(action) else {
            return 0;
        };
        if kinds.is_null() || count == 0 || count > imageops::MAX_ITEMS {
            return 0;
        }
        // SAFETY: forwarded from this function's contract.
        let (kinds, names) = unsafe {
            (
                std::slice::from_raw_parts(kinds, count),
                bytes(names, names_len),
            )
        };
        // The names, each ended by a 0 byte: exactly `count` of them, none empty.
        let mut parts: Vec<&[u8]> = names.split(|b| *b == 0).collect();
        if names.last() == Some(&0) {
            parts.pop();
        }
        if parts.len() != count || parts.iter().any(|p| p.is_empty()) {
            return 0;
        }
        let mut inputs = Vec::with_capacity(count);
        for (k, name) in kinds.iter().zip(parts) {
            let Some(kind) = kind_from(*k) else {
                return 0;
            };
            inputs.push((kind, String::from_utf8_lossy(name).into_owned()));
        }
        let mut joined = Vec::new();
        for n in imageops::output_names(action, &inputs) {
            joined.extend_from_slice(n.as_bytes());
            joined.push(0);
        }
        // SAFETY: forwarded from this function's contract.
        unsafe { put(&joined, out, cap) }
    })
}

/// The limits: 0 files per action, 1 bytes of one file, 2 pixels of one picture.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_image_limit(which: u32) -> u64 {
    match which {
        0 => imageops::MAX_ITEMS as u64,
        1 => imageops::MAX_INPUT_BYTES,
        2 => imageops::MAX_PIXELS,
        _ => 0,
    }
}

/// Why a file of this size and pixel count is refused ("" is written for
/// none; the length is returned, 0 when it is fine).
///
/// # Safety
/// `out` points to `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_image_check_input(
    size: u64,
    pixels: u64,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        match imageops::check_input(size, pixels) {
            Ok(()) => 0,
            // SAFETY: forwarded from this function's contract.
            Err(why) => unsafe { put(why.as_bytes(), out, cap) },
        }
    })
}

/// The EXIF orientation of a JPEG (1 to 8; 1 for none).
///
/// # Safety
/// `data` points to `len` readable bytes (or is null with `len` 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_jpeg_orientation(data: *const u8, len: usize) -> u32 {
    crate::ffi::guarded(0, || {
        // SAFETY: forwarded from this function's contract.
        u32::from(imageops::exif_orientation(unsafe { bytes(data, len) }))
    })
}

/// Writes orientation 1 into the EXIF data of a JPEG, in place; whether there
/// was an orientation.
///
/// # Safety
/// `data` points to `len` readable and writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_jpeg_reset_orientation(data: *mut u8, len: usize) -> bool {
    crate::ffi::guarded(false, || {
        if data.is_null() || len == 0 {
            return false;
        }
        // SAFETY: the caller promises `len` readable and writable bytes.
        imageops::reset_exif_orientation(unsafe { std::slice::from_raw_parts_mut(data, len) })
    })
}

/// The libjpeg-turbo transform (`TJXOP_*`) that turns a JPEG of this EXIF
/// orientation a quarter turn as it is shown.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_jpeg_turn(orientation: u32, clockwise: bool) -> u32 {
    imageops::jpeg_turn(orientation.min(255) as u8, clockwise).code()
}

/// Joins PDFs into a new one. `paths` holds `count` absolute paths each ended
/// by a 0 byte; `output` is the path written. `cancel` is a flag (a
/// `std::atomic<bool>` on the C++ side) looked at between files. Returns 0 on
/// success, else a reason (see `MergeError::code`); `pages` gets the page
/// count, `bad` the position of the file at fault (or `u32::MAX`).
///
/// # Safety
/// `paths` points to `paths_len` readable bytes, `output` to `output_len`;
/// `cancel` is null or points to a live flag; `pages` and `bad` are null or
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_pdf_merge(
    paths: *const u8,
    paths_len: usize,
    output: *const u8,
    output_len: usize,
    cancel: *const u8,
    pages: *mut u32,
    bad: *mut u32,
) -> i32 {
    crate::ffi::guarded(1, || {
        use std::os::unix::ffi::OsStrExt;
        static NEVER: AtomicBool = AtomicBool::new(false);
        // SAFETY: forwarded from this function's contract.
        let (paths, output) = unsafe { (bytes(paths, paths_len), bytes(output, output_len)) };
        let inputs: Vec<PathBuf> = paths
            .split(|b| *b == 0)
            .filter(|p| !p.is_empty())
            .map(|p| PathBuf::from(std::ffi::OsStr::from_bytes(p)))
            .collect();
        let output = PathBuf::from(std::ffi::OsStr::from_bytes(output));
        // SAFETY: an `AtomicBool` is one byte; the caller's flag lives through the call.
        let flag: &AtomicBool = if cancel.is_null() {
            &NEVER
        } else {
            unsafe { &*(cancel as *const AtomicBool) }
        };
        if !pages.is_null() {
            // SAFETY: writable, per the contract.
            unsafe { *pages = 0 };
        }
        if !bad.is_null() {
            // SAFETY: writable, per the contract.
            unsafe { *bad = u32::MAX };
        }
        match pdfmerge::merge(&inputs, &output, flag) {
            Ok(n) => {
                if !pages.is_null() {
                    // SAFETY: writable, per the contract.
                    unsafe { *pages = u32::try_from(n).unwrap_or(u32::MAX) };
                }
                0
            }
            Err(e) => {
                if let (Some(i), false) = (e.input, bad.is_null()) {
                    // SAFETY: writable, per the contract.
                    unsafe { *bad = u32::try_from(i).unwrap_or(u32::MAX) };
                }
                e.code()
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names_of(action: u32, kinds: &[u32], names: &[&str]) -> Vec<String> {
        let mut joined = Vec::new();
        for n in names {
            joined.extend_from_slice(n.as_bytes());
            joined.push(0);
        }
        let mut out = vec![0u8; 4];
        // SAFETY: the pointers and lengths describe live buffers.
        let mut n = unsafe {
            telamon_image_names(
                action,
                kinds.as_ptr(),
                kinds.len(),
                joined.as_ptr(),
                joined.len(),
                out.as_mut_ptr(),
                out.len(),
            )
        };
        if n > out.len() {
            out.resize(n, 0);
            // SAFETY: as above, with the bigger buffer.
            n = unsafe {
                telamon_image_names(
                    action,
                    kinds.as_ptr(),
                    kinds.len(),
                    joined.as_ptr(),
                    joined.len(),
                    out.as_mut_ptr(),
                    out.len(),
                )
            };
        }
        out.truncate(n);
        out.split(|b| *b == 0)
            .filter(|p| !p.is_empty())
            .map(|p| String::from_utf8_lossy(p).into_owned())
            .collect()
    }

    #[test]
    fn kinds_and_names_cross_the_boundary() {
        let jpeg = unsafe { telamon_image_kind(b"image/jpeg".as_ptr(), 10) };
        let png = unsafe { telamon_image_kind(b"image/png".as_ptr(), 9) };
        let pdf = unsafe { telamon_image_kind(b"application/pdf".as_ptr(), 15) };
        assert_eq!((jpeg, png, pdf), (1, 2, 6));
        assert_eq!(
            unsafe { telamon_image_kind(b"image/svg+xml".as_ptr(), 13) },
            0
        );
        assert_eq!(unsafe { telamon_image_kind(std::ptr::null(), 0) }, 0);
        assert!(telamon_image_accepts(0, jpeg));
        assert!(!telamon_image_accepts(0, pdf));
        assert!(telamon_image_accepts(5, pdf));
        assert!(telamon_image_skips(2, png));
        assert!(!telamon_image_skips(2, jpeg));
        assert!(!telamon_image_accepts(99, jpeg));
        assert_eq!(
            names_of(1, &[jpeg, png], &["a.jpg", "b.png"]),
            ["a (rotated).jpg", "b (rotated).png"]
        );
        assert_eq!(
            names_of(2, &[jpeg, jpeg], &["a.jpg", "a.jpeg"]),
            ["a.png", "a (2).png"]
        );
        // Bad arguments make nothing.
        assert!(names_of(99, &[jpeg], &["a.jpg"]).is_empty());
        assert!(names_of(1, &[jpeg, png], &["only-one.jpg"]).is_empty());
        assert!(names_of(1, &[], &[]).is_empty());
    }

    #[test]
    fn limits_and_reasons() {
        assert_eq!(telamon_image_limit(0), 500);
        assert_eq!(telamon_image_limit(99), 0);
        let mut why = [0u8; 128];
        assert_eq!(
            unsafe { telamon_image_check_input(10, 10, why.as_mut_ptr(), why.len()) },
            0
        );
        let n = unsafe { telamon_image_check_input(0, u64::MAX, why.as_mut_ptr(), why.len()) };
        assert!(n > 0 && n <= why.len());
        assert!(String::from_utf8_lossy(&why[..n]).contains("pixels"));
    }

    #[test]
    fn merging_nothing_or_a_missing_file_is_refused() {
        let mut pages = 7u32;
        let mut bad = 7u32;
        let out =
            std::env::temp_dir().join(format!("telamon-image-ffi-{}.pdf", std::process::id()));
        let o = out.to_str().unwrap().as_bytes();
        let rc = unsafe {
            telamon_pdf_merge(
                std::ptr::null(),
                0,
                o.as_ptr(),
                o.len(),
                std::ptr::null(),
                &mut pages,
                &mut bad,
            )
        };
        assert_eq!(rc, 8);
        assert_eq!((pages, bad), (0, u32::MAX));
        let missing = b"/nonexistent/telamon/none.pdf\0";
        let rc = unsafe {
            telamon_pdf_merge(
                missing.as_ptr(),
                missing.len(),
                o.as_ptr(),
                o.len(),
                std::ptr::null(),
                &mut pages,
                &mut bad,
            )
        };
        assert_eq!((rc, bad), (1, 0));
        assert!(!out.exists());
    }
}
