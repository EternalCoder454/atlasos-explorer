//! C ABI for the Trash: emptying what is old (`atlas_explorer_core::trash`)
//! and the texts around it. The rules are in the core; these functions find
//! the places on this computer, move bytes and report counts.

use crate::ffi::{bytes, put};
use atlas_explorer_core::trash::{self, Report};
use std::path::{Path, PathBuf};

/// What a run did or found, the core's [`Report`] for C++.
#[repr(C)]
#[derive(Default)]
pub struct TelamonTrashReport {
    pub folders: usize,
    pub removed: usize,
    pub kept: usize,
    pub skipped: usize,
    pub failed: usize,
}

impl From<Report> for TelamonTrashReport {
    fn from(r: Report) -> Self {
        TelamonTrashReport {
            folders: r.folders,
            removed: r.removed,
            kept: r.kept,
            skipped: r.skipped,
            failed: r.failed,
        }
    }
}

impl From<&TelamonTrashReport> for Report {
    fn from(r: &TelamonTrashReport) -> Self {
        Report {
            folders: r.folders,
            removed: r.removed,
            kept: r.kept,
            skipped: r.skipped,
            failed: r.failed,
        }
    }
}

/// A bigger table than a machine has; a longer one is not read.
const MAX_MOUNTINFO: u64 = 8 * 1024 * 1024;

fn read_mountinfo() -> String {
    use std::io::Read;
    let Ok(f) = std::fs::File::open("/proc/self/mountinfo") else {
        return String::new();
    };
    let mut text = String::new();
    let _ = f.take(MAX_MOUNTINFO).read_to_string(&mut text);
    text
}

/// Every trash folder of this user: the home one under `data_home`, then the
/// volumes' (`<mount>/.Trash/<uid>` or `<mount>/.Trash-<uid>`).
pub(crate) fn known_trash_dirs(data_home: &Path) -> Vec<PathBuf> {
    let uid = trash::current_uid();
    let mounts = trash::parse_mountinfo(&read_mountinfo());
    trash::trash_dirs(&data_home.join("Trash"), &mounts, uid)
}

/// Looks at every trash folder under `data_home` (the home Trash) and on the
/// mounted volumes, and acts on the items deleted more than `days` days
/// before `now_local` (seconds of local wall-clock time counted as if UTC).
/// `mode`: 0 only looks (`removed` counts what would go; nothing is logged or
/// changed), 1 looks, and when nothing is old enough says so in the journal (a
/// scheduled run that has nothing to do), 2 removes what is old and says how
/// much in the journal. Returns false when the arguments are not usable.
///
/// # Safety
/// `data_home` points to `len` readable bytes; `out` is null or points to a
/// writable report.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_trash_purge(
    days: u32,
    now_local: i64,
    mode: u32,
    data_home: *const u8,
    len: usize,
    out: *mut TelamonTrashReport,
) -> bool {
    crate::ffi::guarded(false, || {
        use std::os::unix::ffi::OsStrExt;
        // SAFETY: forwarded from this function's contract.
        let home = unsafe { bytes(data_home, len) };
        if home.is_empty() || home.first() != Some(&b'/') || days < trash::MIN_DAYS || mode > 2 {
            return false;
        }
        let dirs = known_trash_dirs(Path::new(std::ffi::OsStr::from_bytes(home)));
        let report = trash::purge_all(&dirs, now_local, days, mode == 2);
        // What the journal keeps of a run: counts only, never names.
        if mode == 2 || (mode == 1 && report.removed == 0) {
            log::info!("{}", trash::log_line(&report, days, true));
        }
        if !out.is_null() {
            // SAFETY: `out` is a writable report (contract above).
            unsafe { *out = report.into() };
        }
        true
    })
}

/// The line the journal gets for a report.
///
/// # Safety
/// `report` points to a report; `out` points to `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_trash_log_line(
    report: *const TelamonTrashReport,
    days: u32,
    applied: bool,
    out: *mut u8,
    cap: usize,
) -> usize {
    if report.is_null() {
        return 0;
    }
    // SAFETY: `report` points to a report (contract above).
    let r: Report = unsafe { &*report }.into();
    let line = trash::log_line(&r, days, applied);
    // SAFETY: forwarded from this function's contract.
    unsafe { put(line.as_bytes(), out, cap) }
}

/// Whether an item of the Trash may be put back at `target` (see
/// `trash::restore_allowed`); `from_home_trash` is true for the home folder's
/// own Trash.
///
/// # Safety
/// Each pointer pair covers its length (or is null with length 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_trash_restore_allowed(
    from_home_trash: bool,
    target: *const u8,
    target_len: usize,
    home: *const u8,
    home_len: usize,
) -> bool {
    crate::ffi::guarded(false, || {
        // SAFETY: forwarded from this function's contract.
        unsafe {
            trash::restore_allowed(
                from_home_trash,
                bytes(target, target_len),
                bytes(home, home_len),
            )
        }
    })
}

/// Brings a number of days into the limits.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_trash_clamp_days(days: i64) -> u32 {
    trash::clamp_days(days)
}

/// 0 the days "Empty items older than" starts at, 1 the least, 2 the most.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_trash_limit(which: u32) -> u32 {
    match which {
        0 => trash::DEFAULT_DAYS,
        1 => trash::MIN_DAYS,
        _ => trash::MAX_DAYS,
    }
}

/// A `DeletionDate` as the Trash lists it (`YYYY-MM-DDThh:mm:ss`) in seconds
/// of local wall-clock time counted as if UTC; 0 when it is not a date.
///
/// # Safety
/// `text` points to `len` readable bytes (or is null with length 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_trash_parse_date(text: *const u8, len: usize) -> i64 {
    crate::ffi::guarded(-1, || {
        // SAFETY: forwarded from this function's contract.
        let t = unsafe { bytes(text, len) };
        std::str::from_utf8(t)
            .ok()
            .and_then(trash::parse_date)
            .unwrap_or(0)
    })
}

/// Texts. `which`: 0 the title and 1 the text of the question before missing
/// folders are made again (`a`: the folders, written to be shown, one a line;
/// `n`: the number of items), 2 the question before the auto-empty switch is
/// turned on (`n`: days in the low 32 bits, the items that would go now above).
///
/// # Safety
/// `a` points to `a_len` readable bytes (or is null with length 0); `out`
/// points to `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_trash_text(
    which: u32,
    a: *const u8,
    a_len: usize,
    n: u64,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        // SAFETY: forwarded from this function's contract.
        let a = String::from_utf8_lossy(unsafe { bytes(a, a_len) }).into_owned();
        let text = match which {
            0 | 1 => {
                let folders: Vec<String> = a
                    .lines()
                    .filter(|l| !l.is_empty())
                    .map(str::to_string)
                    .collect();
                let (title, text) = trash::recreate_question(&folders, n as usize);
                if which == 0 { title } else { text }
            }
            2 => trash::auto_empty_question((n & 0xFFFF_FFFF) as u32, (n >> 32) as usize),
            _ => String::new(),
        };
        // SAFETY: forwarded from this function's contract.
        unsafe { put(text.as_bytes(), out, cap) }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(which: u32, a: &str, n: u64) -> String {
        let mut buf = [0u8; 600];
        let len = unsafe {
            telamon_trash_text(which, a.as_ptr(), a.len(), n, buf.as_mut_ptr(), buf.len())
        };
        String::from_utf8_lossy(&buf[..len]).into_owned()
    }

    #[test]
    fn the_abi_reads_dates_limits_and_words() {
        let d = b"2026-10-07T10:00:00";
        assert!(unsafe { telamon_trash_parse_date(d.as_ptr(), d.len()) } > 0);
        assert_eq!(unsafe { telamon_trash_parse_date(b"x".as_ptr(), 1) }, 0);
        assert_eq!(unsafe { telamon_trash_parse_date(std::ptr::null(), 0) }, 0);
        assert_eq!(telamon_trash_clamp_days(0), 1);
        assert_eq!(telamon_trash_clamp_days(99999), 3650);
        assert_eq!(telamon_trash_limit(0), 30);
        assert_eq!(text(0, "~/a", 2), "Folder Is Gone");
        assert!(text(1, "~/a\n~/b", 3).starts_with("2 folders don't exist any more"));
        assert!(text(2, "", 30 | (12u64 << 32)).contains("12 items in the Trash are older"));
    }

    #[test]
    fn purge_refuses_bad_arguments_and_reports() {
        let mut r = TelamonTrashReport::default();
        let home = b"/nonexistent/telamon-test-data";
        assert!(!unsafe { telamon_trash_purge(0, 0, 0, home.as_ptr(), home.len(), &mut r) });
        assert!(!unsafe { telamon_trash_purge(30, 0, 3, home.as_ptr(), home.len(), &mut r) });
        assert!(!unsafe { telamon_trash_purge(30, 0, 0, b"rel".as_ptr(), 3, &mut r) });
        assert!(!unsafe { telamon_trash_purge(30, 0, 0, std::ptr::null(), 0, &mut r) });
        // No home Trash there: nothing found (volumes of this machine may be looked at).
        assert!(unsafe { telamon_trash_purge(30, 0, 0, home.as_ptr(), home.len(), &mut r) });
        assert_eq!(r.removed, 0);
        let mut buf = [0u8; 200];
        let n = unsafe { telamon_trash_log_line(&r, 30, false, buf.as_mut_ptr(), buf.len()) };
        assert!(
            String::from_utf8_lossy(&buf[..n])
                .starts_with("trash auto-empty: would remove 0 items")
        );
    }
}
