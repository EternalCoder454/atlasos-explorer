//! C ABI over the operation queue, the undo history, the conflict rules and
//! the pre-flight checks (`atlas_explorer_core`), for `cpp/kio/OperationQueue`.
//! The state machines are the core's; this holds one `Engine` per window, a
//! clock for them, and moves bytes. Everything here is called on the GUI
//! thread, except the free functions (`telamon_preflight`, `telamon_keep_both`)
//! which are pure or read the disk and are meant for workers.

use crate::ffi::{bytes, put};
use atlas_explorer_core::conflict::{self, Case, Newer};
use atlas_explorer_core::history::{History, Side};
use atlas_explorer_core::optext::{self, About};
use atlas_explorer_core::preflight::{self, Transfer};
use atlas_explorer_core::queue::{Action, Answer, ConflictKind, Kind, OpId, Queue, Reply, State};
use atlas_explorer_core::undo::{self, Undo};
use std::collections::{HashMap, VecDeque};
use std::ffi::c_void;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The queue, the history and the actions waiting for the app to carry out.
pub struct Engine {
    queue: Queue,
    history: History,
    start: Instant,
    actions: VecDeque<Action>,
}

fn kind_from(n: u32) -> Option<Kind> {
    Some(match n {
        0 => Kind::Copy,
        1 => Kind::Move,
        2 => Kind::Link,
        3 => Kind::Trash,
        4 => Kind::Delete,
        5 => Kind::Rename,
        6 => Kind::NewFolder,
        7 => Kind::Restore,
        8 => Kind::EmptyTrash,
        9 => Kind::External,
        10 => Kind::Attrs,
        _ => return None,
    })
}

fn answer_from(n: u32) -> Option<Answer> {
    Some(match n {
        0 => Answer::Replace,
        1 => Answer::Skip,
        2 => Answer::KeepBoth,
        3 => Answer::Merge,
        _ => return None,
    })
}

fn answer_to(a: Answer) -> u32 {
    match a {
        Answer::Replace => 0,
        Answer::Skip => 1,
        Answer::KeepBoth => 2,
        Answer::Merge => 3,
    }
}

fn side_from(n: u32) -> Side {
    if n == 0 { Side::Undo } else { Side::Redo }
}

fn state_code(s: &State) -> u32 {
    match s {
        State::Waiting => 0,
        State::Running => 1,
        State::Paused => 2,
        State::NeedsAnswer => 3,
        State::Done => 4,
        State::Failed(_) => 5,
        State::Cancelled => 6,
    }
}

fn action_code(a: Action) -> (u32, OpId) {
    match a {
        Action::Start(id) => (0, id),
        Action::Suspend(id) => (1, id),
        Action::Resume(id) => (2, id),
        Action::Kill(id) => (3, id),
    }
}

impl Engine {
    fn new() -> Engine {
        Engine {
            queue: Queue::new(),
            history: History::new(),
            start: Instant::now(),
            actions: VecDeque::new(),
        }
    }

    fn now(&self) -> Duration {
        self.start.elapsed()
    }

    fn take(&mut self, actions: Vec<Action>) {
        self.actions.extend(actions);
    }
}

/// What `telamon_ops_info` fills in. The C++ twin is in RustBridge.h.
#[repr(C)]
pub struct TelamonOpInfo {
    pub id: u64,
    pub kind: u32,
    /// 0 waiting, 1 running, 2 paused, 3 needs an answer, 4 done, 5 failed, 6 cancelled.
    pub state: u32,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub items_done: u64,
    pub items_total: u64,
    /// Bytes per second; negative while unknown.
    pub speed: f64,
    /// Milliseconds; negative while unknown.
    pub time_left_ms: i64,
    /// 0 none, 1 a file conflict is waiting, 2 a folder conflict.
    pub pending: u32,
}

/// # Safety
/// `h` is null or from `telamon_ops_new` and not freed.
unsafe fn engine<'a>(h: *mut c_void) -> Option<&'a mut Engine> {
    if h.is_null() {
        None
    } else {
        // SAFETY: a live Engine (contract).
        Some(unsafe { &mut *h.cast::<Engine>() })
    }
}

fn string(ptr: *const u8, len: usize) -> String {
    // SAFETY: callers pass a pointer covering `len` bytes (their contract).
    String::from_utf8_lossy(unsafe { bytes(ptr, len) }).into_owned()
}

/// A new engine. Free it with `telamon_ops_free`.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_ops_new() -> *mut c_void {
    Box::into_raw(Box::new(Engine::new())).cast()
}

/// # Safety
/// `h` is from `telamon_ops_new` (or null) and is not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_ops_free(h: *mut c_void) {
    if !h.is_null() {
        // SAFETY: made by Box::into_raw in telamon_ops_new.
        drop(unsafe { Box::from_raw(h.cast::<Engine>()) });
    }
}

/// Adds an operation. `kind` is 0 copy, 1 move, 2 link, 3 trash, 4 delete,
/// 5 rename, 6 new folder, 7 restore, 8 empty trash, 9 external. Returns its
/// id (0 for a kind that isn't one); what to start is queued for
/// `telamon_ops_next_action`.
///
/// # Safety
/// `h` as in `telamon_ops_free`; `label` covers `len` bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_ops_add(
    h: *mut c_void,
    kind: u32,
    label: *const u8,
    len: usize,
) -> u64 {
    // SAFETY: contract.
    let (Some(e), Some(kind)) = (unsafe { engine(h) }, kind_from(kind)) else {
        return 0;
    };
    let now = e.now();
    let (id, actions) = e.queue.add(kind, &string(label, len), now);
    e.take(actions);
    id
}

/// Pops the next thing the app must do: `kind` 0 start, 1 suspend, 2 resume,
/// 3 kill, on operation `id`. False when there is none.
///
/// # Safety
/// `h` as above; `kind` and `id` are writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_ops_next_action(
    h: *mut c_void,
    kind: *mut u32,
    id: *mut u64,
) -> bool {
    // SAFETY: contract.
    let Some(e) = (unsafe { engine(h) }) else {
        return false;
    };
    let Some(a) = e.actions.pop_front() else {
        return false;
    };
    let (k, i) = action_code(a);
    if !kind.is_null() && !id.is_null() {
        // SAFETY: writable (contract).
        unsafe {
            *kind = k;
            *id = i;
        }
    }
    true
}

/// The job gave its first sign of life.
///
/// # Safety
/// `h` as above.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_ops_started(h: *mut c_void, id: u64) {
    // SAFETY: contract.
    if let Some(e) = unsafe { engine(h) } {
        let now = e.now();
        e.queue.started(id, now);
    }
}

/// Progress of a running operation.
///
/// # Safety
/// `h` as above.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_ops_progress(
    h: *mut c_void,
    id: u64,
    bytes_done: u64,
    bytes_total: u64,
    items_done: u64,
    items_total: u64,
) {
    // SAFETY: contract.
    if let Some(e) = unsafe { engine(h) } {
        let now = e.now();
        e.queue
            .progress(id, bytes_done, bytes_total, items_done, items_total, now);
    }
}

/// `what`: 0 pause, 1 resume, 2 run now, 3 cancel, 4 finished. What the app
/// must do about it (a finished or cancelled operation may let the next one
/// start) is queued for `telamon_ops_next_action`.
///
/// # Safety
/// `h` as above.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_ops_event(h: *mut c_void, id: u64, what: u32) {
    // SAFETY: contract.
    let Some(e) = (unsafe { engine(h) }) else {
        return;
    };
    let now = e.now();
    let actions = match what {
        0 => e.queue.pause(id),
        1 => e.queue.resume(id, now),
        2 => e.queue.run_now(id, now),
        3 => e.queue.cancel(id, now),
        4 => e.queue.finished(id, now),
        _ => Vec::new(),
    };
    e.take(actions);
}

/// The job failed, with the reason in plain words.
///
/// # Safety
/// `h` as above; `reason` covers `len` bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_ops_failed(
    h: *mut c_void,
    id: u64,
    reason: *const u8,
    len: usize,
) {
    // SAFETY: contract.
    if let Some(e) = unsafe { engine(h) } {
        let now = e.now();
        let actions = e.queue.failed(id, &string(reason, len), now);
        e.take(actions);
    }
}

/// A job reports a conflict (`kind` 1 file, 2 folder). Returns 0 when the
/// operation isn't running, 1 when the user must be asked (the operation now
/// waits for `telamon_ops_answer`), or 2 + the answer (0 replace, 1 skip,
/// 2 keep both, 3 merge) a stored "do this for all" gives.
///
/// # Safety
/// `h` as above.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_ops_conflict(h: *mut c_void, id: u64, kind: u32) -> u32 {
    // SAFETY: contract.
    let Some(e) = (unsafe { engine(h) }) else {
        return 0;
    };
    let kind = if kind == 2 {
        ConflictKind::Folder
    } else {
        ConflictKind::File
    };
    match e.queue.conflict_asked(id, kind) {
        None => 0,
        Some(Reply::Ask) => 1,
        Some(Reply::Auto(a)) => 2 + answer_to(a),
    }
}

/// The user answered (0 replace, 1 skip, 2 keep both, 3 merge). False when
/// the operation wasn't waiting, or the answer doesn't fit the question.
///
/// # Safety
/// `h` as above.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_ops_answer(
    h: *mut c_void,
    id: u64,
    answer: u32,
    apply_to_all: bool,
) -> bool {
    // SAFETY: contract.
    let (Some(e), Some(answer)) = (unsafe { engine(h) }, answer_from(answer)) else {
        return false;
    };
    let now = e.now();
    e.queue.answered(id, answer, apply_to_all, now).is_ok()
}

/// Fills `out` with an operation's numbers; false for an id that isn't listed.
///
/// # Safety
/// `h` as above; `out` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_ops_info(
    h: *mut c_void,
    id: u64,
    out: *mut TelamonOpInfo,
) -> bool {
    // SAFETY: contract.
    let Some(e) = (unsafe { engine(h) }) else {
        return false;
    };
    let Some(o) = e.queue.get(id) else {
        return false;
    };
    if out.is_null() {
        return false;
    }
    let now = e.now();
    let info = TelamonOpInfo {
        id,
        kind: match o.kind {
            Kind::Copy => 0,
            Kind::Move => 1,
            Kind::Link => 2,
            Kind::Trash => 3,
            Kind::Delete => 4,
            Kind::Rename => 5,
            Kind::NewFolder => 6,
            Kind::Restore => 7,
            Kind::EmptyTrash => 8,
            Kind::External => 9,
            Kind::Attrs => 10,
        },
        state: state_code(&o.state),
        bytes_done: o.bytes_done,
        bytes_total: o.bytes_total,
        items_done: o.items_done,
        items_total: o.items_total,
        speed: o.speed(now).unwrap_or(-1.0),
        time_left_ms: o.time_left(now).map_or(-1, |d| d.as_millis() as i64),
        pending: match o.pending {
            None => 0,
            Some(ConflictKind::File) => 1,
            Some(ConflictKind::Folder) => 2,
        },
    };
    // SAFETY: `out` is writable (contract).
    unsafe { out.write(info) };
    true
}

/// Writes the ids of the listed operations, in the order added; returns how
/// many there are (nothing is written when that is more than `cap`).
///
/// # Safety
/// `h` as above; `out` has `cap` writable u64s (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_ops_ids(h: *mut c_void, out: *mut u64, cap: usize) -> usize {
    // SAFETY: contract.
    let Some(e) = (unsafe { engine(h) }) else {
        return 0;
    };
    let n = e.queue.ops().len();
    if n <= cap && !out.is_null() {
        for (i, o) in e.queue.ops().iter().enumerate() {
            // SAFETY: `out` has `cap >= n` slots.
            unsafe { *out.add(i) = o.id };
        }
    }
    n
}

/// Text of an operation: 0 its label, 1 why it failed (empty otherwise),
/// 2 its numbers in words ("1.2 GiB of 4 GiB, 85 MiB/s, 35 s left").
///
/// # Safety
/// `h` as above; `out` has `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_ops_text(
    h: *mut c_void,
    id: u64,
    which: u32,
    out: *mut u8,
    cap: usize,
) -> usize {
    // SAFETY: contract.
    let Some(e) = (unsafe { engine(h) }) else {
        return 0;
    };
    let Some(o) = e.queue.get(id) else {
        return 0;
    };
    let now = e.now();
    let text = match which {
        0 => o.label.clone(),
        1 => match &o.state {
            State::Failed(why) => why.clone(),
            _ => String::new(),
        },
        _ => optext::progress_line(
            o.bytes_done,
            o.bytes_total,
            o.items_done,
            o.items_total,
            o.speed(now),
            o.time_left(now),
        ),
    };
    // SAFETY: `out` as promised.
    unsafe { put(text.as_bytes(), out, cap) }
}

/// Removes a finished operation from the list (1), or every finished one (0
/// as `id`, returning how many went).
///
/// # Safety
/// `h` as above.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_ops_dismiss(h: *mut c_void, id: u64) -> usize {
    // SAFETY: contract.
    let Some(e) = (unsafe { engine(h) }) else {
        return 0;
    };
    if id == 0 {
        e.queue.clear_finished()
    } else {
        usize::from(e.queue.dismiss(id))
    }
}

// ---- History ----

/// Records a finished operation that can be undone. `rec` is the text form of
/// an undo record (`undo::Undo::to_text`). False when it doesn't parse.
///
/// # Safety
/// `h` as above; the pointers cover their lengths.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_hist_record(
    h: *mut c_void,
    title: *const u8,
    title_len: usize,
    rec: *const u8,
    rec_len: usize,
) -> bool {
    // SAFETY: contract.
    let Some(e) = (unsafe { engine(h) }) else {
        return false;
    };
    let Some(rec) = Undo::from_text(&string(rec, rec_len)) else {
        return false;
    };
    e.history.record(&string(title, title_len), rec);
    true
}

/// An operation that can't be undone finished: the lists are emptied.
///
/// # Safety
/// `h` as above.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_hist_barrier(h: *mut c_void) {
    // SAFETY: contract.
    if let Some(e) = unsafe { engine(h) } {
        e.history.barrier();
    }
}

/// `side` 0 undo, 1 redo. Writes the titles, newest first, one per line;
/// returns the length.
///
/// # Safety
/// `h` as above; `out` has `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_hist_titles(
    h: *mut c_void,
    side: u32,
    out: *mut u8,
    cap: usize,
) -> usize {
    // SAFETY: contract.
    let Some(e) = (unsafe { engine(h) }) else {
        return 0;
    };
    let text = e.history.titles(side_from(side)).join("\n");
    // SAFETY: `out` as promised.
    unsafe { put(text.as_bytes(), out, cap) }
}

/// The paths to look at before `telamon_hist_plan` (`after` 0), or before
/// `telamon_hist_complete` (`after` 1), one per line.
///
/// # Safety
/// `h` as above; `out` has `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_hist_paths(
    h: *mut c_void,
    side: u32,
    after: bool,
    out: *mut u8,
    cap: usize,
) -> usize {
    // SAFETY: contract.
    let Some(e) = (unsafe { engine(h) }) else {
        return 0;
    };
    let side = side_from(side);
    let paths = if after {
        e.history.paths_after(side)
    } else {
        e.history.paths(side)
    };
    let text = paths.join("\n");
    // SAFETY: `out` as promised.
    unsafe { put(text.as_bytes(), out, cap) }
}

/// What the next undo (`side` 0) or redo (1) would do, given how the paths
/// are (`states`: `undo::parse_states`). Returns 0 and writes one step per
/// line (`kind`, tab, path, tab, destination; kinds are `trash_copy`,
/// `move_back`, `remove_folder`, `restore`, `make_folder`, and `set_attr` whose
/// destination field holds the attribute, the value it must have and the value
/// to give it, tab-separated), or 1 and writes
/// the refusal as the reason, a tab and the path it is about (empty for
/// none). The length goes to `*len`.
///
/// # Safety
/// `h` as above; the pointers cover their lengths; `len` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_hist_plan(
    h: *mut c_void,
    side: u32,
    states: *const u8,
    states_len: usize,
    out: *mut u8,
    cap: usize,
    len: *mut usize,
) -> i32 {
    // SAFETY: contract.
    let Some(e) = (unsafe { engine(h) }) else {
        return 1;
    };
    let states = undo::parse_states(&string(states, states_len));
    let (code, text) = match e.history.plan(side_from(side), &states) {
        Ok(plan) => (
            0,
            plan.steps
                .iter()
                .map(|s| {
                    let kind = match s.kind {
                        undo::StepKind::TrashCopy => "trash_copy",
                        undo::StepKind::MoveBack => "move_back",
                        undo::StepKind::RemoveEmptyFolder => "remove_folder",
                        undo::StepKind::RestoreFromTrash => "restore",
                        undo::StepKind::MakeFolder => "make_folder",
                        undo::StepKind::SetAttr => "set_attr",
                    };
                    format!("{kind}\t{}\t{}", s.path, s.to.as_deref().unwrap_or(""))
                })
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        Err(why) => (1, format!("{}\t{}", why.reason, why.path)),
    };
    if !len.is_null() {
        // SAFETY: `out` and `len` as promised.
        unsafe { *len = put(text.as_bytes(), out, cap) };
    }
    code
}

/// The plan ran: moves the entry to the other side as its inverse. `states`
/// is how the paths of `telamon_hist_paths(after)` are now; `trashes` holds
/// lines `path`, tab, `URL it got in the Trash`. Writes the entry's title.
///
/// # Safety
/// `h` as above; the pointers cover their lengths; `out` has `cap` writable bytes.
#[allow(clippy::too_many_arguments)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_hist_complete(
    h: *mut c_void,
    side: u32,
    states: *const u8,
    states_len: usize,
    trashes: *const u8,
    trashes_len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    // SAFETY: contract.
    let Some(e) = (unsafe { engine(h) }) else {
        return 0;
    };
    let states = undo::parse_states(&string(states, states_len));
    let trash_urls: HashMap<String, String> = string(trashes, trashes_len)
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
    let title = e
        .history
        .complete(side_from(side), &states, &trash_urls)
        .unwrap_or_default();
    // SAFETY: `out` as promised.
    unsafe { put(title.as_bytes(), out, cap) }
}

/// The undo or redo failed or can't run any more: the entry is dropped.
///
/// # Safety
/// `h` as above.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_hist_drop(h: *mut c_void, side: u32) {
    // SAFETY: contract.
    if let Some(e) = unsafe { engine(h) } {
        e.history.drop_top(side_from(side));
    }
}

// ---- Words, conflicts and pre-flight ----

/// The words of an operation. `which` 0 the title ("Move 3 Items to Backup"),
/// 1 the running line ("Moving 3 items to Backup"). `names` holds the display
/// names of the first items, one per line; `count` is how many there are in
/// all. `to` and `new_name` may be empty.
///
/// # Safety
/// The pointers cover their lengths (or are null with length 0); `out` has
/// `cap` writable bytes (or is null).
#[allow(clippy::too_many_arguments)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_op_text(
    which: u32,
    kind: u32,
    names: *const u8,
    names_len: usize,
    count: usize,
    to: *const u8,
    to_len: usize,
    new_name: *const u8,
    new_len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    let Some(kind) = kind_from(kind) else {
        return 0;
    };
    let names: Vec<String> = string(names, names_len)
        .lines()
        .map(str::to_string)
        .collect();
    let to = string(to, to_len);
    let new_name = string(new_name, new_len);
    let about = About {
        kind,
        names: &names,
        count,
        to: (!to.is_empty()).then_some(to.as_str()),
        new_name: (!new_name.is_empty()).then_some(new_name.as_str()),
    };
    let text = if which == 0 {
        optext::title(&about)
    } else {
        optext::running(&about)
    };
    // SAFETY: `out` as promised.
    unsafe { put(text.as_bytes(), out, cap) }
}

/// A size in words ("4.2 GiB").
///
/// # Safety
/// `out` has `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_format_size(bytes: u64, out: *mut u8, cap: usize) -> usize {
    // SAFETY: `out` as promised.
    unsafe { put(optext::format_size(bytes).as_bytes(), out, cap) }
}

/// The buttons a conflict offers: bit 0 replace, 1 merge, 2 keep both, 3 skip,
/// then the default answer (0 replace, 1 skip, 2 keep both, 3 merge) shifted
/// left by 8. Bit 16 is set when it is a question about folders.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_conflict_choices(
    source_is_dir: bool,
    dest_is_dir: bool,
    same_file: bool,
) -> u32 {
    let case = Case {
        source_is_dir,
        dest_is_dir,
        same_file,
    };
    let c = conflict::choices(case);
    u32::from(c.replace)
        | u32::from(c.merge) << 1
        | u32::from(c.keep_both) << 2
        | u32::from(c.skip) << 3
        | answer_to(c.default) << 8
        | u32::from(case.kind() == ConflictKind::Folder) << 16
}

/// Whether the answer is one the case offers.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_conflict_allowed(
    source_is_dir: bool,
    dest_is_dir: bool,
    same_file: bool,
    answer: u32,
) -> bool {
    answer_from(answer).is_some_and(|a| {
        conflict::allowed(
            Case {
                source_is_dir,
                dest_is_dir,
                same_file,
            },
            a,
        )
    })
}

/// Which file is newer: 1 the source, 2 the destination, 0 neither (or not
/// known: pass `has_*` false).
#[unsafe(no_mangle)]
pub extern "C" fn telamon_conflict_newer(
    has_source: bool,
    source: i64,
    has_dest: bool,
    dest: i64,
) -> u32 {
    match conflict::newer(has_source.then_some(source), has_dest.then_some(dest)) {
        Newer::Source => 1,
        Newer::Destination => 2,
        Newer::Neither => 0,
    }
}

/// Whether the answer touches files that were there before (replace, merge):
/// the operation can then not be undone.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_conflict_endangers(answer: u32) -> bool {
    answer_from(answer).is_some_and(conflict::endangers_undo)
}

/// The "Keep Both" name for `name` in the local folder `dir`: the first
/// "name (2)", "name (3)"... that doesn't exist. With an empty `dir` (a
/// server) nothing is checked and it is "name (2)".
///
/// # Safety
/// The pointers cover their lengths (or are null with length 0); `out` has
/// `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_keep_both(
    dir: *const u8,
    dir_len: usize,
    name: *const u8,
    name_len: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    // SAFETY: contract.
    let (dir, name) = unsafe { (bytes(dir, dir_len), bytes(name, name_len)) };
    let name = String::from_utf8_lossy(name).into_owned();
    let dir = (!dir.is_empty()).then(|| PathBuf::from(std::ffi::OsStr::from_bytes(dir)));
    let kept = atlas_explorer_core::names::keep_both_name(&name, |candidate| {
        dir.as_ref()
            .is_some_and(|d| std::fs::symlink_metadata(d.join(candidate)).is_ok())
    });
    // SAFETY: `out` as promised.
    unsafe { put(kept.as_bytes(), out, cap) }
}

/// Whether URL or path `dest` is `source` or inside it (text compare of clean,
/// absolute forms).
///
/// # Safety
/// The pointers cover their lengths (or are null with length 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_is_inside(
    source: *const u8,
    source_len: usize,
    dest: *const u8,
    dest_len: usize,
) -> bool {
    preflight::is_inside(&string(source, source_len), &string(dest, dest_len))
}

/// The refusal for a folder into itself, in plain words. `transfer` 0 copy,
/// 1 move; `same` when the destination is the folder itself.
///
/// # Safety
/// The pointers cover their lengths (or are null with length 0); `out` has
/// `cap` writable bytes (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_into_itself_text(
    transfer: u32,
    folder: *const u8,
    folder_len: usize,
    dest: *const u8,
    dest_len: usize,
    same: bool,
    out: *mut u8,
    cap: usize,
) -> usize {
    let t = if transfer == 0 {
        Transfer::Copy
    } else {
        Transfer::Move
    };
    let text = preflight::into_itself_text(
        t,
        &string(folder, folder_len),
        &string(dest, dest_len),
        same,
    );
    // SAFETY: `out` as promised.
    unsafe { put(text.as_bytes(), out, cap) }
}

/// The checks before a local copy (`transfer` 0) or move (1): a folder into
/// itself, and room at the destination. `sources` holds the paths separated by
/// NUL bytes. `free_override` is a number of bytes to take as the free space
/// (for tests), or negative to ask the disk. Reads the disk: call it on a
/// worker. Returns 0 when it may go on, 1 when it is refused (the reason, with
/// the numbers, is written to `out`); the length goes to `*len`.
///
/// # Safety
/// The pointers cover their lengths (or are null with length 0); `out` has
/// `cap` writable bytes (or is null); `len` is writable.
#[allow(clippy::too_many_arguments)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_preflight(
    transfer: u32,
    sources: *const u8,
    sources_len: usize,
    dest: *const u8,
    dest_len: usize,
    free_override: i64,
    out: *mut u8,
    cap: usize,
    len: *mut usize,
) -> i32 {
    let t = if transfer == 0 {
        Transfer::Copy
    } else {
        Transfer::Move
    };
    // SAFETY: contract.
    let (sources, dest) = unsafe { (bytes(sources, sources_len), bytes(dest, dest_len)) };
    let sources: Vec<PathBuf> = sources
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| PathBuf::from(std::ffi::OsStr::from_bytes(s)))
        .collect();
    let dest = Path::new(std::ffi::OsStr::from_bytes(dest));
    let free = u64::try_from(free_override).ok();
    let (code, text) = match preflight::check_local(t, &sources, dest, free) {
        Ok(()) => (0, String::new()),
        Err(why) => (1, why),
    };
    if !len.is_null() {
        // SAFETY: `out` and `len` as promised.
        unsafe { *len = put(text.as_bytes(), out, cap) };
    }
    code
}

/// Room for `needed` bytes taken out of an archive into the folder `dest`
/// (which may not exist yet). `what` names what is taken out. Returns 0 when
/// there is room (or the disk doesn't say), 1 when not, with the refusal in
/// plain words in `out`. Reads the disk: for workers.
///
/// # Safety
/// Each pointer pair covers its length (or is null with length 0); `out`
/// points to `cap` writable bytes (or is null); `len` is writable or null.
#[allow(clippy::too_many_arguments)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telamon_room_check(
    needed: u64,
    dest: *const u8,
    dest_len: usize,
    what: *const u8,
    what_len: usize,
    free_override: i64,
    out: *mut u8,
    cap: usize,
    len: *mut usize,
) -> i32 {
    // SAFETY: forwarded from this function's contract.
    let (dest, what) = unsafe { (bytes(dest, dest_len), bytes(what, what_len)) };
    let dest = Path::new(std::ffi::OsStr::from_bytes(dest));
    let what = String::from_utf8_lossy(what).into_owned();
    let free = u64::try_from(free_override).ok();
    let (code, text) = match preflight::check_room_for(dest, needed, &what, free) {
        Ok(()) => (0, String::new()),
        Err(why) => (1, why),
    };
    if !len.is_null() {
        // SAFETY: `out` and `len` as promised.
        unsafe { *len = put(text.as_bytes(), out, cap) };
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out_string(f: impl Fn(*mut u8, usize) -> usize) -> String {
        let mut buf = vec![0u8; 4096];
        let n = f(buf.as_mut_ptr(), buf.len());
        String::from_utf8(buf[..n].to_vec()).unwrap()
    }

    fn actions(h: *mut c_void) -> Vec<(u32, u64)> {
        let mut out = Vec::new();
        let (mut k, mut i) = (0u32, 0u64);
        while unsafe { telamon_ops_next_action(h, &mut k, &mut i) } {
            out.push((k, i));
        }
        out
    }

    #[test]
    fn a_copy_that_meets_a_conflict_and_is_then_undone_and_redone() {
        let h = telamon_ops_new();
        let label = b"Copying 2 items to Backup";
        let id = unsafe { telamon_ops_add(h, 0, label.as_ptr(), label.len()) };
        assert_eq!(actions(h), [(0, id)]);
        // A second transfer waits behind it.
        let id2 = unsafe { telamon_ops_add(h, 1, label.as_ptr(), label.len()) };
        assert!(actions(h).is_empty());
        // The job meets a file conflict: ask; "apply to all: skip" is stored.
        assert_eq!(unsafe { telamon_ops_conflict(h, id, 1) }, 1);
        let mut info = std::mem::MaybeUninit::<TelamonOpInfo>::uninit();
        assert!(unsafe { telamon_ops_info(h, id, info.as_mut_ptr()) });
        let info = unsafe { info.assume_init() };
        assert_eq!((info.state, info.pending), (3, 1));
        // Merge isn't an answer to a file.
        assert!(!unsafe { telamon_ops_answer(h, id, 3, true) });
        assert!(unsafe { telamon_ops_answer(h, id, 1, true) });
        assert_eq!(unsafe { telamon_ops_conflict(h, id, 1) }, 2 + 1);
        unsafe { telamon_ops_progress(h, id, 10, 100, 1, 2) };
        let text = out_string(|o, c| unsafe { telamon_ops_text(h, id, 0, o, c) });
        assert_eq!(text, "Copying 2 items to Backup");
        // Finishing starts the waiting one.
        unsafe { telamon_ops_event(h, id, 4) };
        assert_eq!(actions(h), [(0, id2)]);
        // Its failure is in plain words.
        let why = b"disk full";
        unsafe { telamon_ops_failed(h, id2, why.as_ptr(), why.len()) };
        assert_eq!(
            out_string(|o, c| unsafe { telamon_ops_text(h, id2, 1, o, c) }),
            "disk full"
        );
        assert_eq!(unsafe { telamon_ops_dismiss(h, 0) }, 2);

        // The copy is recorded, undone (trashed) and redone (restored).
        let rec = b"copy\nfile:///b/a\tf\t3\t1\t0\n";
        let title = b"Copy a to Backup";
        assert!(unsafe {
            telamon_hist_record(h, title.as_ptr(), title.len(), rec.as_ptr(), rec.len())
        });
        assert_eq!(
            out_string(|o, c| unsafe { telamon_hist_titles(h, 0, o, c) }),
            "Copy a to Backup"
        );
        let paths = out_string(|o, c| unsafe { telamon_hist_paths(h, 0, false, o, c) });
        assert_eq!(paths, "file:///b/a");
        let states = b"file:///b/a\tf\t3\t1\t0\n";
        let mut len = 0usize;
        let mut buf = vec![0u8; 1024];
        let code = unsafe {
            telamon_hist_plan(
                h,
                0,
                states.as_ptr(),
                states.len(),
                buf.as_mut_ptr(),
                buf.len(),
                &mut len,
            )
        };
        assert_eq!(code, 0);
        assert_eq!(
            String::from_utf8_lossy(&buf[..len]),
            "trash_copy\tfile:///b/a\t"
        );
        let trashes = b"file:///b/a\ttrash:/0-a\n";
        let done = out_string(|o, c| unsafe {
            telamon_hist_complete(h, 0, b"".as_ptr(), 0, trashes.as_ptr(), trashes.len(), o, c)
        });
        assert_eq!(done, "Copy a to Backup");
        assert_eq!(
            out_string(|o, c| unsafe { telamon_hist_titles(h, 1, o, c) }),
            "Copy a to Backup"
        );
        // The file changed: the redo is refused in plain words.
        let moved = b"trash:/0-a\tf\t999\t1\t0\nfile:///b/a\tmissing\n";
        let code = unsafe {
            telamon_hist_plan(
                h,
                1,
                moved.as_ptr(),
                moved.len(),
                buf.as_mut_ptr(),
                buf.len(),
                &mut len,
            )
        };
        assert_eq!(code, 1);
        assert!(String::from_utf8_lossy(&buf[..len]).starts_with("It changed since"));
        unsafe { telamon_ops_free(h) };
    }

    #[test]
    fn words_conflicts_and_preflight_through_the_abi() {
        let names = b"a\nb\nc";
        let to = b"Backup";
        let t = out_string(|o, c| unsafe {
            telamon_op_text(
                0,
                1,
                names.as_ptr(),
                names.len(),
                3,
                to.as_ptr(),
                to.len(),
                std::ptr::null(),
                0,
                o,
                c,
            )
        });
        assert_eq!(t, "Move 3 Items to Backup");
        let r = out_string(|o, c| unsafe {
            telamon_op_text(
                1,
                1,
                names.as_ptr(),
                names.len(),
                3,
                to.as_ptr(),
                to.len(),
                std::ptr::null(),
                0,
                o,
                c,
            )
        });
        assert_eq!(r, "Moving 3 items to Backup");
        assert_eq!(
            out_string(|o, c| unsafe { telamon_format_size(4509715456, o, c) }),
            "4.2 GiB"
        );

        let c = telamon_conflict_choices(true, true, false);
        assert_eq!(c & 0xf, 0b1010); // merge and skip
        assert_eq!((c >> 8) & 0xff, 1); // default: skip
        assert_ne!(c & (1 << 16), 0);
        assert!(!telamon_conflict_allowed(false, false, true, 0));
        assert!(telamon_conflict_allowed(false, false, false, 0));
        assert_eq!(telamon_conflict_newer(true, 9, true, 3), 1);
        assert_eq!(telamon_conflict_newer(true, 3, true, 9), 2);
        assert_eq!(telamon_conflict_newer(false, 0, true, 9), 0);
        assert!(telamon_conflict_endangers(0));
        assert!(!telamon_conflict_endangers(2));

        assert!(unsafe { telamon_is_inside(b"/a/b".as_ptr(), 4, b"/a/b/c".as_ptr(), 6) });
        let kept = out_string(|o, c| unsafe {
            telamon_keep_both(b"".as_ptr(), 0, b"report.txt".as_ptr(), 10, o, c)
        });
        assert_eq!(kept, "report (2).txt");

        // The pre-flight with a mocked full disk, on a real folder.
        let dir = std::env::temp_dir().join(format!("telamon-ops-ffi-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("dest")).unwrap();
        std::fs::write(dir.join("big.bin"), vec![0u8; 8192]).unwrap();
        let src = dir.join("big.bin");
        let dest = dir.join("dest");
        let (mut len, mut buf) = (0usize, vec![0u8; 1024]);
        let code = unsafe {
            telamon_preflight(
                0,
                src.as_os_str().as_bytes().as_ptr(),
                src.as_os_str().as_bytes().len(),
                dest.as_os_str().as_bytes().as_ptr(),
                dest.as_os_str().as_bytes().len(),
                1000,
                buf.as_mut_ptr(),
                buf.len(),
                &mut len,
            )
        };
        assert_eq!(code, 1);
        assert_eq!(
            String::from_utf8_lossy(&buf[..len]),
            "There isn't enough space in \"dest\". Copying \"big.bin\" needs 8 KiB, and only 1000 B is free."
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn room_for_an_extraction_reports_through_the_abi() {
        let dest = b"/nonexistent-telamon-test/new-folder";
        let what = b"\"a.zip\"";
        let mut out = [0u8; 256];
        let mut len = 0usize;
        let mut ask = |needed: u64, free: i64| {
            let rc = unsafe {
                telamon_room_check(
                    needed,
                    dest.as_ptr(),
                    dest.len(),
                    what.as_ptr(),
                    what.len(),
                    free,
                    out.as_mut_ptr(),
                    out.len(),
                    &mut len,
                )
            };
            (
                rc,
                String::from_utf8_lossy(&out[..len.min(out.len())]).into_owned(),
            )
        };
        // The nearest folder that exists is "/", and it has room for a little.
        assert_eq!(ask(10, 100), (0, String::new()));
        let (rc, text) = ask(1000, 100);
        assert_eq!(rc, 1);
        assert!(
            text.contains("new-folder") && text.contains("out of the archive"),
            "{text}"
        );
    }
}
