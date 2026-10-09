//! C ABI for the Settings window's parts (`cpp/kio/ActionsLogic.*`,
//! `MenuPrefs.*`, `GitLogic.*`, `IndexSettings.*`): the custom actions, the
//! hidden menu entries, Git status badges and the index's folders. The logic
//! is in `atlas_explorer_core` (`actions`, `menuprefs`, `gitstatus`) and
//! `atlas_file_index::config`; these functions move bytes. Lists are text, the
//! caller keeps them in the settings file; every call takes the list as it was
//! last kept and gives the new one.

use crate::ffi::{bytes, put};
use atlas_explorer_core::actions::{self, ActionList, Item, Outcome};
use atlas_explorer_core::gitstatus::{self, Skip};
use atlas_explorer_core::menu;
use atlas_explorer_core::menuprefs::{self, Hidden};
use std::fmt::Write;
use std::path::Path;
use std::time::Duration;

fn text_of(ptr: *const u8, len: usize) -> String {
    // SAFETY: callers pass `len` readable bytes (their contracts).
    String::from_utf8_lossy(unsafe { bytes(ptr, len) }).into_owned()
}

/// Percent-encodes every byte that is not plain, so the text has no tab, no
/// line break and no `%` of its own, whatever the name held.
fn pct(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len());
    for &c in b {
        if c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-' | b'~') {
            s.push(char::from(c));
        } else {
            let _ = write!(s, "%{c:02X}");
        }
    }
    s
}

fn unpct(field: &str) -> Option<Vec<u8>> {
    let b = field.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = field.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    Some(out)
}

// ---- Custom actions ----

/// The limits: 0 most actions, 1 longest name (characters), 2 longest
/// program (bytes), 3 longest arguments (bytes), 4 most runs at once, 5 most
/// items at once.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_actions_limit(which: u32) -> usize {
    match which {
        0 => actions::MAX_ACTIONS,
        1 => actions::MAX_NAME_CHARS,
        2 => actions::MAX_PROGRAM_BYTES,
        3 => actions::MAX_ARGS_BYTES,
        4 => actions::MAX_RUNS,
        _ => actions::MAX_ITEMS,
    }
}

/// The list as it should be kept: every line that is fine, the rest dropped.
///
/// # Safety
/// `list` covers `list_len` readable bytes (or is null with 0); `out` points
/// to `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_actions_clean(
    list: *const u8,
    list_len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        let l = ActionList::parse(&text_of(list, list_len));
        // SAFETY: `out` as promised.
        unsafe { put(l.to_text().as_bytes(), out, cap) }
    })
}

/// Why `record` (`name program args types ask`, tab-separated, each field
/// percent-encoded) can't be kept, in plain words; nothing when it can.
///
/// # Safety
/// Each pointer pair covers its length (or is null with 0); `out` points to
/// `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_actions_problem(
    record: *const u8,
    record_len: usize,
    path: *const u8,
    path_len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        let path = text_of(path, path_len);
        let text = match actions::parse_record(&text_of(record, record_len)) {
            None => actions::Problem::BadTypes.text(),
            Some(a) => actions::problem(&a, &path).map_or_else(String::new, |p| p.text()),
        };
        // SAFETY: `out` as promised.
        unsafe { put(text.as_bytes(), out, cap) }
    })
}

fn outcome_code(o: Outcome) -> u32 {
    match o {
        Outcome::Done => 0,
        Outcome::Invalid => 1,
        Outcome::Full => 2,
        Outcome::Missing => 3,
    }
}

/// Adds an action at the end. `*status`: 0 added, 1 refused (see
/// `telamon_actions_problem`), 2 full. On a refusal the list comes back
/// unchanged.
///
/// # Safety
/// Each pointer pair covers its length (or is null with 0); `out` points to
/// `cap` writable bytes (or is null); `status` is writable (or null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_actions_add(
    list: *const u8,
    list_len: usize,
    record: *const u8,
    record_len: usize,
    path: *const u8,
    path_len: usize,
    out: *mut u8,
    cap: usize,
    status: *mut u32,
) -> usize {
    crate::ffi::guarded(0, || {
        let mut l = ActionList::parse(&text_of(list, list_len));
        let path = text_of(path, path_len);
        let st = match actions::parse_record(&text_of(record, record_len)) {
            None => Outcome::Invalid,
            Some(a) => match l.add(a, &path) {
                Ok(_) => Outcome::Done,
                Err(o) => o,
            },
        };
        if !status.is_null() {
            // SAFETY: writable (contract).
            unsafe { status.write(outcome_code(st)) };
        }
        // SAFETY: `out` as promised.
        unsafe { put(l.to_text().as_bytes(), out, cap) }
    })
}

/// Replaces the action with `id`. `*status`: 0 done, 1 refused, 3 no such action.
///
/// # Safety
/// As `telamon_actions_add`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_actions_update(
    list: *const u8,
    list_len: usize,
    id: u32,
    record: *const u8,
    record_len: usize,
    path: *const u8,
    path_len: usize,
    out: *mut u8,
    cap: usize,
    status: *mut u32,
) -> usize {
    crate::ffi::guarded(0, || {
        let mut l = ActionList::parse(&text_of(list, list_len));
        let path = text_of(path, path_len);
        let st = match actions::parse_record(&text_of(record, record_len)) {
            None => Outcome::Invalid,
            Some(a) => l.update(id, a, &path),
        };
        if !status.is_null() {
            // SAFETY: writable (contract).
            unsafe { status.write(outcome_code(st)) };
        }
        // SAFETY: `out` as promised.
        unsafe { put(l.to_text().as_bytes(), out, cap) }
    })
}

/// Removes the action with `id`.
///
/// # Safety
/// As `telamon_actions_add`, without `status`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_actions_remove(
    list: *const u8,
    list_len: usize,
    id: u32,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        let mut l = ActionList::parse(&text_of(list, list_len));
        l.remove(id);
        // SAFETY: `out` as promised.
        unsafe { put(l.to_text().as_bytes(), out, cap) }
    })
}

/// The file the program names (`PATH` is `path`). `*status` 0: the text is the
/// absolute path; 1: it is the reason, in plain words.
///
/// # Safety
/// Each pointer pair covers its length (or is null with 0); `out` points to
/// `cap` writable bytes (or is null); `status` is writable (or null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_actions_resolve(
    program: *const u8,
    program_len: usize,
    path: *const u8,
    path_len: usize,
    out: *mut u8,
    cap: usize,
    status: *mut u32,
) -> usize {
    crate::ffi::guarded(0, || {
        let r = actions::resolve_program(&text_of(program, program_len), &text_of(path, path_len));
        let (st, text) = match r {
            Ok(p) => (0, p.to_string_lossy().into_owned()),
            Err(p) => (1, p.text()),
        };
        if !status.is_null() {
            // SAFETY: writable (contract).
            unsafe { status.write(st) };
        }
        // SAFETY: `out` as promised.
        unsafe { put(text.as_bytes(), out, cap) }
    })
}

/// The argument lists to run. `items`: one line for each item, `path` and
/// `url`, tab-separated, each percent-encoded (an empty path: not on this
/// computer). The text starts with a line, 0 when the action can run (then
/// one line for each run, its arguments tab-separated and percent-encoded) or
/// 1 (then a line with the reason). Each run's line is `r` and then a tab
/// before each argument (so no arguments is `r`, and one empty argument `r`
/// and a tab). Returns the length of the text.
///
/// # Safety
/// Each pointer pair covers its length (or is null with 0); `out` points to
/// `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_actions_expand(
    args: *const u8,
    args_len: usize,
    items: *const u8,
    items_len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        let mut list: Vec<Item> = Vec::new();
        for line in text_of(items, items_len).lines() {
            let Some((p, u)) = line.split_once('\t') else {
                continue;
            };
            let (Some(p), Some(u)) = (unpct(p), unpct(u)) else {
                continue;
            };
            list.push(Item {
                path: if p.is_empty() {
                    None
                } else {
                    Some(String::from_utf8_lossy(&p).into_owned())
                },
                url: String::from_utf8_lossy(&u).into_owned(),
            });
        }
        let text = match actions::expand(&text_of(args, args_len), &list) {
            Err(r) => format!("1\n{}", r.text()),
            Ok(runs) => {
                let mut s = String::from("0\n");
                for run in runs {
                    s.push('r');
                    for a in &run {
                        s.push('\t');
                        s.push_str(&pct(a.as_bytes()));
                    }
                    s.push('\n');
                }
                s
            }
        };
        // SAFETY: `out` as promised.
        unsafe { put(text.as_bytes(), out, cap) }
    })
}

/// Whether an item with these MIME type names (one a line: its type, then the
/// types it inherits from) is one the action's `types` (space-separated
/// patterns) are for.
///
/// # Safety
/// Each pointer pair covers its length (or is null with 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_actions_type_matches(
    types: *const u8,
    types_len: usize,
    mimes: *const u8,
    mimes_len: usize,
) -> bool {
    crate::ffi::guarded(false, || {
        let types = actions::parse_types(&text_of(types, types_len)).unwrap_or_default();
        let m = text_of(mimes, mimes_len);
        let names: Vec<&str> = m.lines().collect();
        actions::type_matches(&types, &names)
    })
}

/// The command as it is shown: the program and the arguments, safe to read.
///
/// # Safety
/// Each pointer pair covers its length (or is null with 0); `out` points to
/// `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_actions_command_text(
    program: *const u8,
    program_len: usize,
    args: *const u8,
    args_len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        let a = actions::Action {
            program: text_of(program, program_len),
            args: text_of(args, args_len),
            ..actions::Action::default()
        };
        // SAFETY: `out` as promised.
        unsafe { put(actions::command_text(&a).as_bytes(), out, cap) }
    })
}

// ---- Hidden menu entries ----

/// The list as it should be kept.
///
/// # Safety
/// `list` covers `list_len` readable bytes (or is null with 0); `out` points
/// to `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_menuprefs_clean(
    list: *const u8,
    list_len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        let h = Hidden::parse(&text_of(list, list_len));
        // SAFETY: `out` as promised.
        unsafe { put(h.to_text().as_bytes(), out, cap) }
    })
}

/// Hides (`hidden`) or shows `key`; gives the new list, unchanged when the key
/// can't be kept.
///
/// # Safety
/// Each pointer pair covers its length (or is null with 0); `out` points to
/// `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_menuprefs_set(
    list: *const u8,
    list_len: usize,
    key: *const u8,
    key_len: usize,
    hidden: bool,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        let mut h = Hidden::parse(&text_of(list, list_len));
        h.set(&text_of(key, key_len), hidden);
        // SAFETY: `out` as promised.
        unsafe { put(h.to_text().as_bytes(), out, cap) }
    })
}

/// The keys of the built-in entries, one a line, in the order Settings lists them.
///
/// # Safety
/// `out` points to `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_menuprefs_builtin(out: *mut u8, cap: usize) -> usize {
    crate::ffi::guarded(0, || {
        // SAFETY: `out` as promised.
        unsafe { put(menuprefs::builtin_keys().join("\n").as_bytes(), out, cap) }
    })
}

/// The names of the hidden service-menu actions and plugins, one a line (for
/// `KFileItemActions`' exclude list).
///
/// # Safety
/// As `telamon_menuprefs_clean`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_menuprefs_excluded(
    list: *const u8,
    list_len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        let h = Hidden::parse(&text_of(list, list_len));
        // SAFETY: `out` as promised.
        unsafe { put(h.excluded_services().join("\n").as_bytes(), out, cap) }
    })
}

/// What a context menu offers (as `telamon_menu_state`) without the entries
/// the list hides.
///
/// # Safety
/// `hidden` covers `hidden_len` readable bytes (or is null with 0); `out`
/// points to `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_menu_state_hiding(
    kind: u32,
    count: usize,
    folders: usize,
    flags: u32,
    hidden: *const u8,
    hidden_len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        let entries = if kind == 0 {
            menu::item_menu(count, folders, flags)
        } else {
            menu::background_menu(flags)
        };
        let h = Hidden::parse(&text_of(hidden, hidden_len));
        let text = menu::state_text(&h.filter(entries));
        // SAFETY: `out` as promised.
        unsafe { put(text.as_bytes(), out, cap) }
    })
}

// ---- Git status badges ----

/// The badges of the items of `folder` (a path on this computer). `*status`:
/// 0 an answer (the text: a first line `all=0`, `all=2` (the folder is untracked: new) or `all=3` (ignored), then a line for
/// each marked item, `code` and `name` tab-separated, the name
/// percent-encoded; code 1 modified, 2 new, 3 ignored, 4 conflict); 1 not in
/// a git work tree; 2 the repository is another user's; 3 its configuration
/// runs programs; 4 git is not installed; 5 git was stopped for taking too
/// long; 6 it failed. Blocks for up to `timeout_ms`: call it from a worker.
///
/// # Safety
/// Each pointer pair covers its length (or is null with 0); `out` points to
/// `cap` writable bytes (or is null); `status` is writable (or null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_git_status(
    folder: *const u8,
    folder_len: usize,
    path: *const u8,
    path_len: usize,
    timeout_ms: u32,
    out: *mut u8,
    cap: usize,
    status: *mut u32,
) -> usize {
    crate::ffi::guarded(0, || {
        let folder = text_of(folder, folder_len);
        let path = text_of(path, path_len);
        let uid = gitstatus::effective_uid();
        let timeout = Duration::from_millis(u64::from(timeout_ms.clamp(100, 60_000)));
        let (st, text) = match gitstatus::status(Path::new(&folder), uid, &path, timeout) {
            Ok(b) => {
                // all=3: everything not listed is ignored; all=2: new; all=0: nothing is.
                let all = if b.all_ignored {
                    3
                } else if b.all_new {
                    2
                } else {
                    0
                };
                let mut s = format!("all={all}\n");
                for (name, badge) in &b.items {
                    let _ = writeln!(s, "{}\t{}", badge.code(), pct(name));
                }
                (0, s)
            }
            Err(Skip::NotARepo) => (1, String::new()),
            Err(Skip::OtherOwner) => (2, String::new()),
            Err(Skip::UnsafeConfig) => (3, String::new()),
            Err(Skip::NoGit) => (4, String::new()),
            Err(Skip::Timeout) => (5, String::new()),
            Err(Skip::Failed(e)) => (6, e),
        };
        if !status.is_null() {
            // SAFETY: writable (contract).
            unsafe { status.write(st) };
        }
        // SAFETY: `out` as promised.
        unsafe { put(text.as_bytes(), out, cap) }
    })
}

// ---- The index's folders ----

/// The folders `indexrc` (its text) says to index, one a line; the home
/// folder when there is no `Roots=` line.
///
/// # Safety
/// Each pointer pair covers its length (or is null with 0); `out` points to
/// `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_indexrc_roots(
    text: *const u8,
    text_len: usize,
    home: *const u8,
    home_len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    crate::ffi::guarded(0, || {
        let cfg = atlas_file_index::config::Config::parse(
            &text_of(text, text_len),
            Path::new(&text_of(home, home_len)),
        );
        let lines: Vec<String> = cfg
            .roots
            .iter()
            .map(|r| r.to_string_lossy().into_owned())
            .collect();
        // SAFETY: `out` as promised.
        unsafe { put(lines.join("\n").as_bytes(), out, cap) }
    })
}

/// `indexrc` with its folders replaced by `roots` (one a line). `*status` 0:
/// the text is the new file; 1: a folder is not acceptable, the text says which.
///
/// # Safety
/// Each pointer pair covers its length (or is null with 0); `out` points to
/// `cap` writable bytes (or is null); `status` is writable (or null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_indexrc_set_roots(
    text: *const u8,
    text_len: usize,
    roots: *const u8,
    roots_len: usize,
    out: *mut u8,
    cap: usize,
    status: *mut u32,
) -> usize {
    crate::ffi::guarded(0, || {
        let list: Vec<String> = text_of(roots, roots_len)
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(str::to_string)
            .collect();
        let (st, t) = match atlas_file_index::config::with_roots(&text_of(text, text_len), &list) {
            Ok(t) => (0, t),
            Err(e) => (1, e),
        };
        if !status.is_null() {
            // SAFETY: writable (contract).
            unsafe { status.write(st) };
        }
        // SAFETY: `out` as promised.
        unsafe { put(t.as_bytes(), out, cap) }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(mut f: impl FnMut(*mut u8, usize) -> usize) -> String {
        let mut buf = vec![0u8; 16384];
        let n = f(buf.as_mut_ptr(), buf.len());
        String::from_utf8(buf[..n].to_vec()).unwrap()
    }

    #[test]
    fn percent_encoding_round_trips_any_bytes() {
        let all: Vec<u8> = (0..=255u8).collect();
        let e = pct(&all);
        assert!(!e.contains(['\t', '\n', ' ']));
        assert_eq!(unpct(&e).unwrap(), all);
        assert!(unpct("%zz").is_none());
        assert!(unpct("%4").is_none());
    }

    #[test]
    fn expand_reports_argument_lists_through_the_abi() {
        let items = format!(
            "{}\t{}\n{}\t{}\n",
            pct(b"/tmp/a b; $(x)"),
            pct(b"file:///tmp/a%20b"),
            pct(b"/tmp/-rf\nx"),
            pct(b"file:///tmp/-rf")
        );
        let args = "--in %f --tag 'q r'";
        let t = call(|o, c| unsafe {
            telamon_actions_expand(args.as_ptr(), args.len(), items.as_ptr(), items.len(), o, c)
        });
        let mut lines = t.lines();
        assert_eq!(lines.next(), Some("0"));
        let runs: Vec<Vec<Vec<u8>>> = lines
            .map(|l| {
                let mut f = l.split('\t');
                assert_eq!(f.next(), Some("r"));
                f.map(|a| unpct(a).unwrap()).collect()
            })
            .collect();
        assert_eq!(runs.len(), 2);
        assert_eq!(
            runs[0],
            vec![
                b"--in".to_vec(),
                b"/tmp/a b; $(x)".to_vec(),
                b"--tag".to_vec(),
                b"q r".to_vec()
            ]
        );
        assert_eq!(runs[1][1], b"/tmp/-rf\nx".to_vec());
        // One empty argument is not no argument.
        let t = call(|o, c| unsafe {
            let (a, i) = ("''", "x\ty\n");
            telamon_actions_expand(a.as_ptr(), a.len(), i.as_ptr(), i.len(), o, c)
        });
        assert_eq!(t, "0\nr\t\n");
        let t = call(|o, c| unsafe {
            let (a, i) = ("", "");
            telamon_actions_expand(a.as_ptr(), a.len(), i.as_ptr(), i.len(), o, c)
        });
        assert_eq!(t, "0\nr\n");
        // A refusal says why.
        let none = "";
        let t = call(|o, c| unsafe {
            telamon_actions_expand(args.as_ptr(), args.len(), none.as_ptr(), 0, o, c)
        });
        assert!(t.starts_with("1\nSelect a file"), "{t}");
        // A short buffer gets the length it needs.
        let mut tiny = [0u8; 2];
        let n = unsafe {
            telamon_actions_expand(
                args.as_ptr(),
                args.len(),
                items.as_ptr(),
                items.len(),
                tiny.as_mut_ptr(),
                tiny.len(),
            )
        };
        assert!(n > 2);
    }

    #[test]
    fn actions_are_added_checked_and_removed_through_the_abi() {
        let path = "/usr/bin:/bin";
        let rec = |name: &str, program: &str, args: &str, types: &str| {
            let e = |s: &str| pct(s.as_bytes());
            format!("{}\t{}\t{}\t{}\t1", e(name), e(program), e(args), e(types))
        };
        let add = |list: &str, r: &str| {
            let mut st = 9u32;
            let t = call(|o, c| unsafe {
                telamon_actions_add(
                    list.as_ptr(),
                    list.len(),
                    r.as_ptr(),
                    r.len(),
                    path.as_ptr(),
                    path.len(),
                    o,
                    c,
                    &mut st,
                )
            });
            (t, st)
        };
        let (l, st) = add("", &rec("Show it", "true", "%f", "image/*"));
        assert_eq!(st, 0, "`true` is in /usr/bin or /bin");
        assert_eq!(l.lines().count(), 1);
        let (l2, st) = add(&l, &rec("Shell", "sh", "-c %f", ""));
        assert_eq!((st, l2.as_str()), (1, l.as_str()));
        let (l2, st) = add(&l, &rec("Missing", "no-such-program-xyz", "", ""));
        assert_eq!((st, l2.as_str()), (1, l.as_str()));
        let (_, st) = add(&l, "garbage");
        assert_eq!(st, 1);
        let problem = call(|o, c| {
            let r = rec("Shell", "bash", "", "");
            unsafe { telamon_actions_problem(r.as_ptr(), r.len(), path.as_ptr(), path.len(), o, c) }
        });
        assert!(problem.contains("runs command lines"), "{problem}");
        let fine = call(|o, c| {
            let r = rec("Fine", "true", "", "");
            unsafe { telamon_actions_problem(r.as_ptr(), r.len(), path.as_ptr(), path.len(), o, c) }
        });
        assert_eq!(fine, "");
        let mut st = 9u32;
        let r = rec("Renamed", "true", "-x", "");
        let l3 = call(|o, c| unsafe {
            telamon_actions_update(
                l.as_ptr(),
                l.len(),
                1,
                r.as_ptr(),
                r.len(),
                path.as_ptr(),
                path.len(),
                o,
                c,
                &mut st,
            )
        });
        assert_eq!(st, 0);
        assert!(l3.starts_with("1\tRenamed\t"), "{l3}");
        let l4 = call(|o, c| unsafe { telamon_actions_remove(l3.as_ptr(), l3.len(), 1, o, c) });
        assert_eq!(l4, "");
        let m = call(|o, c| unsafe {
            let (t, mm) = ("image/*", "image/png\napplication/octet-stream");
            let ok = telamon_actions_type_matches(t.as_ptr(), t.len(), mm.as_ptr(), mm.len());
            put(if ok { b"yes" } else { b"no" }, o, c)
        });
        assert_eq!(m, "yes");
    }

    #[test]
    fn hidden_entries_filter_the_menu_through_the_abi() {
        let flags = menu::F_WRITABLE | menu::F_LOCAL | menu::F_TERMINAL;
        let hidden = "copyPath\nopenTerminal\nsvc:thing\n";
        let t = call(|o, c| unsafe {
            telamon_menu_state_hiding(0, 1, 0, flags, hidden.as_ptr(), hidden.len(), o, c)
        });
        assert!(!t.contains("copyPath"), "{t}");
        assert!(!t.contains("openTerminal"), "{t}");
        assert!(t.contains("open+"), "{t}");
        let all = call(|o, c| unsafe {
            telamon_menu_state_hiding(0, 1, 0, flags, std::ptr::null(), 0, o, c)
        });
        assert!(all.contains("copyPath"));
        let ex =
            call(|o, c| unsafe { telamon_menuprefs_excluded(hidden.as_ptr(), hidden.len(), o, c) });
        assert_eq!(ex, "thing");
        let set = call(|o, c| unsafe {
            telamon_menuprefs_set(
                hidden.as_ptr(),
                hidden.len(),
                b"new".as_ptr(),
                3,
                true,
                o,
                c,
            )
        });
        assert!(set.ends_with("new\n"));
        let set = call(|o, c| unsafe {
            telamon_menuprefs_set(
                set.as_ptr(),
                set.len(),
                b"copyPath".as_ptr(),
                8,
                false,
                o,
                c,
            )
        });
        assert!(!set.contains("copyPath"));
        let b = call(|o, c| unsafe { telamon_menuprefs_builtin(o, c) });
        assert!(b.lines().any(|l| l == "properties") && b.lines().any(|l| l == "rotateLeft"));
    }

    #[test]
    fn index_folders_are_read_and_written_through_the_abi() {
        let text = "[Index]\nRoots=/a;/b\nExclude=x\n";
        let r = call(|o, c| unsafe {
            telamon_indexrc_roots(text.as_ptr(), text.len(), b"/home/t".as_ptr(), 7, o, c)
        });
        assert_eq!(r, "/a\n/b");
        let r = call(|o, c| unsafe {
            telamon_indexrc_roots(std::ptr::null(), 0, b"/home/t".as_ptr(), 7, o, c)
        });
        assert_eq!(r, "/home/t");
        let mut st = 9u32;
        let roots = "/x\n/y\n";
        let t = call(|o, c| unsafe {
            telamon_indexrc_set_roots(
                text.as_ptr(),
                text.len(),
                roots.as_ptr(),
                roots.len(),
                o,
                c,
                &mut st,
            )
        });
        assert_eq!((st, t.as_str()), (0, "[Index]\nRoots=/x;/y\nExclude=x\n"));
        let bad = "relative";
        let t = call(|o, c| unsafe {
            telamon_indexrc_set_roots(
                text.as_ptr(),
                text.len(),
                bad.as_ptr(),
                bad.len(),
                o,
                c,
                &mut st,
            )
        });
        assert_eq!(st, 1);
        assert!(t.contains("relative"));
    }

    #[test]
    fn git_status_reports_why_through_the_abi() {
        let d = std::env::temp_dir().join(format!("telamon-gitffi-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.to_string_lossy().into_owned();
        let path = "/usr/bin:/bin";
        let mut st = 9u32;
        let t = call(|o, c| unsafe {
            telamon_git_status(
                f.as_ptr(),
                f.len(),
                path.as_ptr(),
                path.len(),
                2000,
                o,
                c,
                &mut st,
            )
        });
        assert_eq!((st, t.as_str()), (1, ""));
        let _ = std::fs::remove_dir_all(&d);
    }
}
