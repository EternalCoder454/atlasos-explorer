//! C ABI for Batch Rename: the new names of a set of items and what is the
//! matter with each (`atlas_explorer_core::batch`). The logic is in the core;
//! this moves bytes.
//!
//! The items come in as records of one byte (1 for a folder), a little-endian
//! u32 length and the name's bytes. The answer goes out in a caller's buffer
//! (the return value is its length, so a short buffer can be retried):
//!
//!   u8 can_apply, u32 changed, u32 blocked, u32 length + the problem text,
//!   then for each item: u8 code (`Check::code`), u32 length + the new name,
//!   u32 length + the reason in plain words.

use crate::ffi::{bytes, put};
use atlas_explorer_core::batch::{self, CaseMode, Edge, Item, Op};

/// What to do with the names, as the dialog has it.
#[repr(C)]
pub struct TelamonBatchSpec {
    /// 0 find and replace, 1 add a number, 2 change case, 3 add text.
    pub mode: u32,
    /// The text to find (0), the separator after a number (1), the text to add (3).
    pub first: *const u8,
    pub first_len: usize,
    /// The replacement (0).
    pub second: *const u8,
    pub second_len: usize,
    pub match_case: bool,
    pub regex: bool,
    pub start: u64,
    pub step: u64,
    pub padding: u32,
    /// Number and text go after the name (before the extension) rather than before it.
    pub at_end: bool,
    /// 0 lower, 1 UPPER, 2 Title, 3 Sentence.
    pub case_mode: u32,
}

fn items_of(buf: &[u8]) -> Option<Vec<(String, bool)>> {
    let mut out = Vec::new();
    let mut rest = buf;
    while !rest.is_empty() {
        let (&dir, tail) = rest.split_first()?;
        let len = u32::from_le_bytes(tail.get(..4)?.try_into().ok()?) as usize;
        let name = tail.get(4..4usize.checked_add(len)?)?;
        out.push((String::from_utf8_lossy(name).into_owned(), dir != 0));
        rest = &tail[4 + len..];
    }
    Some(out)
}

fn push_text(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&(s.len() as u32).to_le_bytes());
    out.extend_from_slice(s.as_bytes());
}

/// Plans a batch rename. Returns the length of the answer (see the module
/// text), or 0 when the items are not well formed. `exists(ctx, name, len)`
/// says whether a name is already taken in the folder; null: nothing is.
///
/// # Safety
/// `spec` points to a valid spec whose pointers cover their lengths (or are
/// null with length 0); `items` covers `items_len` bytes; `exists` can be
/// called with `ctx` and any name; `out` points to `cap` writable bytes (or
/// is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_batch_plan(
    spec: *const TelamonBatchSpec,
    items: *const u8,
    items_len: usize,
    exists: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const u8, usize) -> bool>,
    ctx: *mut std::ffi::c_void,
    out: *mut u8,
    cap: usize,
) -> usize {
    if spec.is_null() {
        return 0;
    }
    // SAFETY: forwarded from this function's contract.
    let (spec, items) = unsafe { (&*spec, bytes(items, items_len)) };
    let Some(parsed) = items_of(items) else {
        return 0;
    };
    // SAFETY: forwarded from this function's contract.
    let (first, second) = unsafe {
        (
            String::from_utf8_lossy(bytes(spec.first, spec.first_len)).into_owned(),
            String::from_utf8_lossy(bytes(spec.second, spec.second_len)).into_owned(),
        )
    };
    let at = if spec.at_end { Edge::End } else { Edge::Start };
    let op = match spec.mode {
        0 => Op::Replace {
            find: &first,
            with: &second,
            match_case: spec.match_case,
            regex: spec.regex,
        },
        1 => Op::Number {
            start: spec.start,
            step: spec.step,
            padding: spec.padding as usize,
            at,
            separator: &first,
        },
        2 => match CaseMode::from_code(spec.case_mode) {
            Some(m) => Op::Case(m),
            None => return 0,
        },
        3 => Op::AddText { text: &first, at },
        _ => return 0,
    };
    let list: Vec<Item> = parsed
        .iter()
        .map(|(name, is_dir)| Item {
            name,
            is_dir: *is_dir,
        })
        .collect();
    let plan = batch::plan(&list, &op, |name| match exists {
        // SAFETY: the callback is valid for `ctx` and any name, as promised.
        Some(f) => unsafe { f(ctx, name.as_ptr(), name.len()) },
        None => false,
    });
    let mut data = Vec::new();
    data.push(plan.can_apply() as u8);
    data.extend_from_slice(&(plan.changed as u32).to_le_bytes());
    data.extend_from_slice(&(plan.blocked as u32).to_le_bytes());
    push_text(&mut data, &plan.problem);
    for row in &plan.rows {
        data.push(row.check.code());
        push_text(&mut data, &row.new);
        push_text(&mut data, &row.check.describe());
    }
    // SAFETY: `out` as promised above.
    unsafe { put(&data, out, cap) }
}

/// How many bytes at the start of `name` an in-place rename selects first
/// (the name without its extension; all of a folder's name).
///
/// # Safety
/// `name` points to `len` readable bytes (or is null with `len` 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_name_stem_len(name: *const u8, len: usize, is_dir: bool) -> usize {
    // SAFETY: forwarded from this function's contract.
    let name = String::from_utf8_lossy(unsafe { bytes(name, len) }).into_owned();
    atlas_explorer_core::names::stem_len(&name, is_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(out: &mut Vec<u8>, name: &str, dir: bool) {
        out.push(dir as u8);
        push_text(out, name);
    }
    fn spec(mode: u32) -> TelamonBatchSpec {
        TelamonBatchSpec {
            mode,
            first: std::ptr::null(),
            first_len: 0,
            second: std::ptr::null(),
            second_len: 0,
            match_case: true,
            regex: false,
            start: 1,
            step: 1,
            padding: 2,
            at_end: true,
            case_mode: 1,
        }
    }
    fn run(s: &TelamonBatchSpec, items: &[u8]) -> Vec<u8> {
        let need = unsafe {
            telamon_batch_plan(
                s,
                items.as_ptr(),
                items.len(),
                None,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
            )
        };
        let mut buf = vec![0u8; need];
        let n = unsafe {
            telamon_batch_plan(
                s,
                items.as_ptr(),
                items.len(),
                None,
                std::ptr::null_mut(),
                buf.as_mut_ptr(),
                buf.len(),
            )
        };
        assert_eq!(n, need);
        buf
    }

    #[test]
    fn plans_through_the_abi() {
        let mut items = Vec::new();
        item(&mut items, "a.txt", false);
        item(&mut items, "b", true);
        let out = run(&spec(2), &items);
        assert_eq!(out[0], 1, "can apply");
        assert_eq!(u32::from_le_bytes(out[1..5].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(out[5..9].try_into().unwrap()), 0);
        // No problem text, then "A.txt".
        assert_eq!(u32::from_le_bytes(out[9..13].try_into().unwrap()), 0);
        assert_eq!(out[13], 1);
        let len = u32::from_le_bytes(out[14..18].try_into().unwrap()) as usize;
        assert_eq!(&out[18..18 + len], b"A.txt");
    }

    #[test]
    fn the_folder_is_asked_about_names() {
        unsafe extern "C" fn taken(_: *mut std::ffi::c_void, n: *const u8, l: usize) -> bool {
            // SAFETY: the test passes valid names.
            unsafe { std::slice::from_raw_parts(n, l) == b"A.txt" }
        }
        let mut items = Vec::new();
        item(&mut items, "a.txt", false);
        let s = spec(2);
        let mut buf = vec![0u8; 256];
        let n = unsafe {
            telamon_batch_plan(
                &s,
                items.as_ptr(),
                items.len(),
                Some(taken),
                std::ptr::null_mut(),
                buf.as_mut_ptr(),
                buf.len(),
            )
        };
        assert!(n > 0);
        assert_eq!(buf[0], 0, "cannot apply");
        assert_eq!(buf[13], 6, "already in the folder");
    }

    #[test]
    fn the_stem_through_the_abi() {
        let n = b"a.tar.gz";
        assert_eq!(
            unsafe { telamon_name_stem_len(n.as_ptr(), n.len(), false) },
            1
        );
        assert_eq!(
            unsafe { telamon_name_stem_len(n.as_ptr(), n.len(), true) },
            8
        );
        assert_eq!(
            unsafe { telamon_name_stem_len(std::ptr::null(), 0, false) },
            0
        );
    }

    #[test]
    fn malformed_input_is_refused() {
        let s = spec(2);
        let bad = [0u8, 9, 0, 0, 0, b'a'];
        let mut buf = [0u8; 16];
        let n = unsafe {
            telamon_batch_plan(
                &s,
                bad.as_ptr(),
                bad.len(),
                None,
                std::ptr::null_mut(),
                buf.as_mut_ptr(),
                buf.len(),
            )
        };
        assert_eq!(n, 0);
        let n = unsafe {
            telamon_batch_plan(
                std::ptr::null(),
                bad.as_ptr(),
                0,
                None,
                std::ptr::null_mut(),
                buf.as_mut_ptr(),
                buf.len(),
            )
        };
        assert_eq!(n, 0);
        let mut worse = spec(9);
        worse.mode = 9;
        let n = unsafe {
            telamon_batch_plan(
                &worse,
                std::ptr::null(),
                0,
                None,
                std::ptr::null_mut(),
                buf.as_mut_ptr(),
                buf.len(),
            )
        };
        assert_eq!(n, 0);
    }
}
