//! The operation queue's state machine. No I/O and no clock: every event
//! carries `now` (a `Duration` since any fixed start), and every event that
//! can change what runs returns the [`Action`]s the app must perform on its
//! KIO jobs. See docs/DESIGN.md, "Operations".
//!
//! Scheduling: one transfer is active at a time (a transfer that is running,
//! paused or waiting for an answer holds the slot, so pausing never starts a
//! second disk job by surprise); quick operations start at once beside it;
//! `run_now` starts a waiting transfer in parallel.

use std::collections::VecDeque;
use std::time::Duration;

pub type OpId = u64;

/// Finished operations (done, failed or cancelled) kept in the list.
pub const MAX_FINISHED: usize = 50;
/// The window speed is measured over.
pub const SPEED_WINDOW: Duration = Duration::from_secs(5);
/// Under this much measured time, the speed is still unknown.
const MIN_SPAN: Duration = Duration::from_millis(500);
/// Progress reports closer together than this replace the last sample.
const SAMPLE_GAP: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Copy,
    Move,
    Link,
    Trash,
    Delete,
    Rename,
    NewFolder,
    Restore,
    EmptyTrash,
    /// A job another app runs (Archive's), shown and paced here.
    External,
}

impl Kind {
    /// Transfers move or remove bulk data and take turns; the rest are quick.
    pub fn is_transfer(self) -> bool {
        matches!(
            self,
            Kind::Copy | Kind::Move | Kind::Delete | Kind::EmptyTrash | Kind::External
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Waiting,
    Running,
    Paused,
    NeedsAnswer,
    Done,
    Failed(String),
    Cancelled,
}

impl State {
    /// Holds a running slot: started and not finished.
    pub fn is_active(&self) -> bool {
        matches!(self, State::Running | State::Paused | State::NeedsAnswer)
    }
    pub fn is_finished(&self) -> bool {
        matches!(self, State::Done | State::Failed(_) | State::Cancelled)
    }
}

/// What the app must do to a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Start(OpId),
    Suspend(OpId),
    Resume(OpId),
    Kill(OpId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictKind {
    File,
    Folder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Replace,
    Skip,
    KeepBoth,
    /// Folders only.
    Merge,
}

/// What to do about a conflict that was just reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply {
    /// Answered from an earlier "apply to all": tell the job so.
    Auto(Answer),
    /// Ask the user; the op waits in `NeedsAnswer`.
    Ask,
}

#[derive(Debug, Clone)]
pub struct Op {
    pub id: OpId,
    pub kind: Kind,
    pub label: String,
    pub state: State,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub items_done: u64,
    pub items_total: u64,
    /// The conflict being asked about.
    pub pending: Option<ConflictKind>,
    file_policy: Option<Answer>,
    folder_policy: Option<Answer>,
    samples: VecDeque<(Duration, u64)>,
    /// Order of finishing, for dropping the oldest.
    finished_seq: u64,
}

impl Op {
    fn begin(&mut self, now: Duration) {
        self.state = State::Running;
        self.restart_samples(now);
    }

    fn restart_samples(&mut self, now: Duration) {
        self.samples.clear();
        self.samples.push_back((now, self.bytes_done));
    }

    /// Bytes per second over the last 5 s of running time. None while
    /// unknown (not running, or under half a second measured); 0 when
    /// nothing moved.
    pub fn speed(&self, now: Duration) -> Option<f64> {
        if self.state != State::Running {
            return None;
        }
        let from = now.saturating_sub(SPEED_WINDOW);
        let first = self.samples.iter().find(|s| s.0 >= from);
        let Some(&(t0, b0)) = first else {
            // Samples exist but all are older than the window: stalled.
            return (!self.samples.is_empty()).then_some(0.0);
        };
        let span = now.saturating_sub(t0);
        if span < MIN_SPAN {
            return None;
        }
        let last = self.samples.back().map_or(b0, |s| s.1);
        Some(last.saturating_sub(b0) as f64 / span.as_secs_f64())
    }

    /// Time left at the current speed; None while the speed is unknown or
    /// zero, or the total is unknown.
    pub fn time_left(&self, now: Duration) -> Option<Duration> {
        let speed = self.speed(now)?;
        if speed <= 0.0 || self.bytes_total == 0 {
            return None;
        }
        let secs = self.bytes_total.saturating_sub(self.bytes_done) as f64 / speed;
        (secs.is_finite() && secs < 1e9).then(|| Duration::from_secs_f64(secs))
    }
}

#[derive(Debug, Default)]
pub struct Queue {
    ops: Vec<Op>,
    next_id: OpId,
    seq: u64,
}

impl Queue {
    pub fn new() -> Self {
        Queue {
            ops: Vec::new(),
            next_id: 1,
            seq: 0,
        }
    }

    /// Every listed op, in the order added.
    pub fn ops(&self) -> &[Op] {
        &self.ops
    }

    pub fn get(&self, id: OpId) -> Option<&Op> {
        self.ops.iter().find(|o| o.id == id)
    }

    /// The op whose result an undo would reverse: the latest one that ended
    /// as `Done`. It stays listed until a later one replaces it.
    pub fn last_done(&self) -> Option<OpId> {
        self.ops
            .iter()
            .filter(|o| o.state == State::Done)
            .max_by_key(|o| o.finished_seq)
            .map(|o| o.id)
    }

    fn op_mut(&mut self, id: OpId) -> Option<&mut Op> {
        self.ops.iter_mut().find(|o| o.id == id)
    }

    /// Adds an op. Returns its id and what to start now.
    pub fn add(&mut self, kind: Kind, label: &str, now: Duration) -> (OpId, Vec<Action>) {
        let id = self.next_id;
        self.next_id += 1;
        self.ops.push(Op {
            id,
            kind,
            label: label.to_string(),
            state: State::Waiting,
            bytes_done: 0,
            bytes_total: 0,
            items_done: 0,
            items_total: 0,
            pending: None,
            file_policy: None,
            folder_policy: None,
            samples: VecDeque::new(),
            finished_seq: 0,
        });
        (id, self.schedule(now))
    }

    /// Starts what may start: quick ops always, the first waiting transfer
    /// when none is active. Then drops finished ops beyond the last 50.
    fn schedule(&mut self, now: Duration) -> Vec<Action> {
        let mut actions = Vec::new();
        let mut busy = self
            .ops
            .iter()
            .any(|o| o.kind.is_transfer() && o.state.is_active());
        for o in &mut self.ops {
            if o.state != State::Waiting {
                continue;
            }
            if o.kind.is_transfer() {
                if busy {
                    continue;
                }
                busy = true;
            }
            o.begin(now);
            actions.push(Action::Start(o.id));
        }
        self.trim();
        actions
    }

    fn trim(&mut self) {
        while self.ops.iter().filter(|o| o.state.is_finished()).count() > MAX_FINISHED {
            let oldest = self
                .ops
                .iter()
                .enumerate()
                .filter(|(_, o)| o.state.is_finished())
                .min_by_key(|(_, o)| o.finished_seq)
                .map(|(i, _)| i);
            match oldest {
                Some(i) => {
                    self.ops.remove(i);
                }
                None => break,
            }
        }
    }

    /// The job really started (its first sign of life): speed is measured
    /// from here. Harmless to send late.
    pub fn started(&mut self, id: OpId, now: Duration) {
        if let Some(o) = self.op_mut(id)
            && o.state == State::Running
        {
            o.restart_samples(now);
        }
    }

    /// "Run now" on a waiting transfer: starts it beside the active one.
    pub fn run_now(&mut self, id: OpId, now: Duration) -> Vec<Action> {
        match self.op_mut(id) {
            Some(o) if o.state == State::Waiting => {
                o.begin(now);
                vec![Action::Start(id)]
            }
            _ => Vec::new(),
        }
    }

    /// Progress of a running op; reports for any other state are ignored.
    pub fn progress(
        &mut self,
        id: OpId,
        bytes_done: u64,
        bytes_total: u64,
        items_done: u64,
        items_total: u64,
        now: Duration,
    ) {
        let Some(o) = self.op_mut(id) else { return };
        if o.state != State::Running {
            return;
        }
        if bytes_done < o.bytes_done {
            // A job that starts over (a retried file): measure afresh.
            o.samples.clear();
        }
        o.bytes_done = bytes_done;
        o.bytes_total = bytes_total;
        o.items_done = items_done;
        o.items_total = items_total;
        while o
            .samples
            .front()
            .is_some_and(|s| s.0 < now.saturating_sub(SPEED_WINDOW))
            && o.samples.len() > 1
        {
            o.samples.pop_front();
        }
        let coalesce = o.samples.len() >= 2
            && o.samples
                .back()
                .is_some_and(|l| now.saturating_sub(l.0) < SAMPLE_GAP);
        if coalesce {
            o.samples.pop_back();
        }
        o.samples.push_back((now, bytes_done));
    }

    /// The user paused a running op.
    pub fn pause(&mut self, id: OpId) -> Vec<Action> {
        match self.op_mut(id) {
            Some(o) if o.state == State::Running => {
                o.state = State::Paused;
                o.samples.clear();
                vec![Action::Suspend(id)]
            }
            _ => Vec::new(),
        }
    }

    pub fn resume(&mut self, id: OpId, now: Duration) -> Vec<Action> {
        match self.op_mut(id) {
            Some(o) if o.state == State::Paused => {
                o.begin(now);
                vec![Action::Resume(id)]
            }
            _ => Vec::new(),
        }
    }

    /// The job reports a conflict. With a stored "apply to all" answer for
    /// this kind the reply is `Auto`; otherwise the op waits for the user.
    pub fn conflict_asked(&mut self, id: OpId, kind: ConflictKind) -> Option<Reply> {
        let o = self.op_mut(id)?;
        if o.state != State::Running {
            return None;
        }
        let stored = match kind {
            ConflictKind::File => o.file_policy,
            ConflictKind::Folder => o.folder_policy,
        };
        if let Some(answer) = stored {
            return Some(Reply::Auto(answer));
        }
        o.state = State::NeedsAnswer;
        o.pending = Some(kind);
        o.samples.clear();
        Some(Reply::Ask)
    }

    /// The user answered. With `apply_to_all`, later conflicts of the same
    /// kind in this op get the same answer. `Merge` is for folders only.
    pub fn answered(
        &mut self,
        id: OpId,
        answer: Answer,
        apply_to_all: bool,
        now: Duration,
    ) -> Result<(), &'static str> {
        let o = self.op_mut(id).ok_or("no such operation")?;
        let kind = match (&o.state, o.pending) {
            (State::NeedsAnswer, Some(k)) => k,
            _ => return Err("the operation is not waiting for an answer"),
        };
        if answer == Answer::Merge && kind == ConflictKind::File {
            return Err("only folders can be merged");
        }
        if apply_to_all {
            match kind {
                ConflictKind::File => o.file_policy = Some(answer),
                ConflictKind::Folder => o.folder_policy = Some(answer),
            }
        }
        o.pending = None;
        o.begin(now);
        Ok(())
    }

    fn end(&mut self, id: OpId, state: State, now: Duration) -> Vec<Action> {
        self.seq += 1;
        let seq = self.seq;
        if let Some(o) = self.op_mut(id)
            && !o.state.is_finished()
        {
            o.state = state;
            o.pending = None;
            o.samples.clear();
            o.finished_seq = seq;
        }
        self.schedule(now)
    }

    /// The job finished. Ignored unless the op was started.
    pub fn finished(&mut self, id: OpId, now: Duration) -> Vec<Action> {
        if !self.get(id).is_some_and(|o| o.state.is_active()) {
            return Vec::new();
        }
        if let Some(o) = self.op_mut(id) {
            if o.bytes_total > 0 {
                o.bytes_done = o.bytes_total;
            }
            o.items_done = o.items_done.max(o.items_total);
        }
        self.end(id, State::Done, now)
    }

    /// The job failed, with a reason in plain words.
    pub fn failed(&mut self, id: OpId, reason: &str, now: Duration) -> Vec<Action> {
        if !self.get(id).is_some_and(|o| o.state.is_active()) {
            return Vec::new();
        }
        self.end(id, State::Failed(reason.to_string()), now)
    }

    /// The user cancelled. A started job is killed; a waiting one just goes.
    pub fn cancel(&mut self, id: OpId, now: Duration) -> Vec<Action> {
        let Some(o) = self.get(id) else {
            return Vec::new();
        };
        let mut actions = Vec::new();
        if o.state.is_active() {
            actions.push(Action::Kill(id));
        } else if o.state != State::Waiting {
            return actions;
        }
        actions.extend(self.end(id, State::Cancelled, now));
        actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    fn state(q: &Queue, id: OpId) -> State {
        q.get(id).unwrap().state.clone()
    }

    #[test]
    fn one_transfer_at_a_time_quick_ops_beside_it() {
        let mut q = Queue::new();
        let (a, act) = q.add(Kind::Copy, "a", t(0));
        assert_eq!(act, [Action::Start(a)]);
        let (b, act) = q.add(Kind::Move, "b", t(0));
        assert!(act.is_empty());
        assert_eq!(state(&q, b), State::Waiting);
        let (r, act) = q.add(Kind::Rename, "r", t(0));
        assert_eq!(act, [Action::Start(r)]);
        let (c, act) = q.add(Kind::External, "c", t(0));
        assert!(act.is_empty());
        // Finishing the first starts the next in order, only one.
        assert_eq!(q.finished(a, t(1)), [Action::Start(b)]);
        assert_eq!(state(&q, c), State::Waiting);
        assert_eq!(state(&q, a), State::Done);
    }

    #[test]
    fn run_now_starts_in_parallel() {
        let mut q = Queue::new();
        let (a, _) = q.add(Kind::Copy, "a", t(0));
        let (b, _) = q.add(Kind::Copy, "b", t(0));
        assert_eq!(q.run_now(b, t(1)), [Action::Start(b)]);
        assert_eq!(state(&q, b), State::Running);
        assert!(q.run_now(b, t(1)).is_empty());
        assert!(q.run_now(a, t(1)).is_empty());
    }

    #[test]
    fn pause_holds_the_slot_and_resume_continues() {
        let mut q = Queue::new();
        let (a, _) = q.add(Kind::Copy, "a", t(0));
        let (b, _) = q.add(Kind::Copy, "b", t(0));
        assert_eq!(q.pause(a), [Action::Suspend(a)]);
        assert!(q.pause(a).is_empty());
        assert_eq!(state(&q, b), State::Waiting);
        assert_eq!(q.resume(a, t(5)), [Action::Resume(a)]);
        assert!(q.resume(a, t(5)).is_empty());
        assert_eq!(state(&q, a), State::Running);
    }

    #[test]
    fn cancel_and_fail_free_the_slot() {
        let mut q = Queue::new();
        let (a, _) = q.add(Kind::Delete, "a", t(0));
        let (b, _) = q.add(Kind::Copy, "b", t(0));
        let (c, _) = q.add(Kind::Copy, "c", t(0));
        assert_eq!(q.cancel(a, t(1)), [Action::Kill(a), Action::Start(b)]);
        assert_eq!(state(&q, a), State::Cancelled);
        assert_eq!(q.failed(b, "disk full", t(2)), [Action::Start(c)]);
        assert_eq!(state(&q, b), State::Failed("disk full".into()));
        // A waiting op is cancelled with no Kill; late events change nothing.
        let (d, _) = q.add(Kind::Copy, "d", t(2));
        assert!(q.cancel(d, t(2)).is_empty());
        assert_eq!(state(&q, d), State::Cancelled);
        assert!(q.finished(a, t(3)).is_empty());
        assert!(q.failed(d, "x", t(3)).is_empty());
        assert!(q.cancel(a, t(3)).is_empty());
        assert_eq!(state(&q, a), State::Cancelled);
        assert_eq!(state(&q, d), State::Cancelled);
    }

    #[test]
    fn speed_over_a_sliding_window_and_time_left() {
        let mut q = Queue::new();
        let (a, _) = q.add(Kind::Copy, "a", t(0));
        assert_eq!(q.get(a).unwrap().speed(t(0)), None);
        for s in 1..=10u64 {
            q.progress(a, s * 1000, 100_000, s, 100, t(s));
        }
        let o = q.get(a).unwrap();
        let sp = o.speed(t(10)).unwrap();
        assert!((sp - 1000.0).abs() < 1.0, "{sp}");
        assert_eq!(o.time_left(t(10)), Some(t(90)));
        // The speed follows the last 5 s: it doubles, the old rate fades.
        for s in 11..=20u64 {
            q.progress(a, 10_000 + (s - 10) * 2000, 100_000, s, 100, t(s));
        }
        let sp = q.get(a).unwrap().speed(t(20)).unwrap();
        assert!((sp - 2000.0).abs() < 1.0, "{sp}");
        // Nothing reported for over 5 s: stalled, no time left.
        let o = q.get(a).unwrap();
        assert_eq!(o.speed(t(40)), Some(0.0));
        assert_eq!(o.time_left(t(40)), None);
    }

    #[test]
    fn unknown_total_and_pause_give_no_estimate() {
        let mut q = Queue::new();
        let (a, _) = q.add(Kind::Copy, "a", t(0));
        q.progress(a, 5000, 0, 1, 0, t(5));
        assert!(q.get(a).unwrap().speed(t(5)).unwrap() > 0.0);
        assert_eq!(q.get(a).unwrap().time_left(t(5)), None);
        q.progress(a, 6000, 100_000, 1, 10, t(6));
        q.pause(a);
        let o = q.get(a).unwrap();
        assert_eq!(o.speed(t(7)), None);
        assert_eq!(o.time_left(t(7)), None);
        // Time spent paused does not dilute the speed after resuming.
        q.resume(a, t(100));
        q.progress(a, 7000, 100_000, 1, 10, t(101));
        q.progress(a, 8000, 100_000, 1, 10, t(102));
        let sp = q.get(a).unwrap().speed(t(102)).unwrap();
        assert!((sp - 1000.0).abs() < 1.0, "{sp}");
        // Progress while paused is ignored.
        q.pause(a);
        q.progress(a, 99_000, 100_000, 5, 10, t(103));
        assert_eq!(q.get(a).unwrap().bytes_done, 8000);
    }

    #[test]
    fn samples_stay_bounded() {
        let mut q = Queue::new();
        let (a, _) = q.add(Kind::Copy, "a", t(0));
        for i in 0..100_000u64 {
            q.progress(a, i, 1_000_000, 0, 0, Duration::from_millis(i));
        }
        assert!(
            q.get(a).unwrap().samples.len() <= 100,
            "{}",
            q.get(a).unwrap().samples.len()
        );
    }

    #[test]
    fn conflict_apply_to_all_per_kind() {
        let mut q = Queue::new();
        let (a, _) = q.add(Kind::Copy, "a", t(0));
        assert_eq!(q.conflict_asked(a, ConflictKind::File), Some(Reply::Ask));
        assert_eq!(state(&q, a), State::NeedsAnswer);
        assert_eq!(q.get(a).unwrap().pending, Some(ConflictKind::File));
        assert!(q.answered(a, Answer::Merge, false, t(1)).is_err());
        assert!(q.answered(a, Answer::Skip, true, t(1)).is_ok());
        assert_eq!(state(&q, a), State::Running);
        assert_eq!(
            q.conflict_asked(a, ConflictKind::File),
            Some(Reply::Auto(Answer::Skip))
        );
        assert_eq!(state(&q, a), State::Running);
        // Folders are a separate question.
        assert_eq!(q.conflict_asked(a, ConflictKind::Folder), Some(Reply::Ask));
        assert!(q.answered(a, Answer::Merge, true, t(2)).is_ok());
        assert_eq!(
            q.conflict_asked(a, ConflictKind::Folder),
            Some(Reply::Auto(Answer::Merge))
        );
        // Without "apply to all" nothing is stored; other ops don't share.
        let (b, _) = q.add(Kind::Rename, "b", t(2));
        assert_eq!(q.conflict_asked(b, ConflictKind::File), Some(Reply::Ask));
        q.answered(b, Answer::Replace, false, t(3)).unwrap();
        assert_eq!(q.conflict_asked(b, ConflictKind::File), Some(Reply::Ask));
        assert!(q.answered(a, Answer::Skip, false, t(3)).is_err());
    }

    #[test]
    fn needs_answer_holds_the_slot_and_can_be_cancelled() {
        let mut q = Queue::new();
        let (a, _) = q.add(Kind::Copy, "a", t(0));
        let (b, _) = q.add(Kind::Copy, "b", t(0));
        q.conflict_asked(a, ConflictKind::File);
        assert_eq!(state(&q, b), State::Waiting);
        assert!(q.pause(a).is_empty());
        assert_eq!(q.cancel(a, t(1)), [Action::Kill(a), Action::Start(b)]);
    }

    #[test]
    fn last_done_is_replaced_by_the_next() {
        let mut q = Queue::new();
        let (a, _) = q.add(Kind::Copy, "a", t(0));
        assert_eq!(q.last_done(), None);
        q.finished(a, t(1));
        assert_eq!(q.last_done(), Some(a));
        let (b, _) = q.add(Kind::Rename, "b", t(2));
        q.finished(b, t(2));
        assert_eq!(q.last_done(), Some(b));
        let (c, _) = q.add(Kind::Copy, "c", t(3));
        q.failed(c, "x", t(3));
        assert_eq!(q.last_done(), Some(b));
    }

    #[test]
    fn finished_ops_are_bounded() {
        let mut q = Queue::new();
        let (keep, _) = q.add(Kind::Copy, "running", t(0));
        for i in 0..200u64 {
            let (id, _) = q.add(Kind::Rename, "r", t(i));
            q.finished(id, t(i));
        }
        let finished = q.ops().iter().filter(|o| o.state.is_finished()).count();
        assert_eq!(finished, MAX_FINISHED);
        assert_eq!(q.get(keep).unwrap().state, State::Running);
        // The newest survive.
        assert_eq!(q.ops().last().unwrap().id, 201);
        assert!(q.get(2).is_none());
    }

    #[test]
    fn finishing_fills_progress() {
        let mut q = Queue::new();
        let (a, _) = q.add(Kind::Copy, "a", t(0));
        q.progress(a, 10, 100, 1, 4, t(1));
        q.finished(a, t(2));
        let o = q.get(a).unwrap();
        assert_eq!((o.bytes_done, o.items_done), (100, 4));
    }

    #[test]
    fn unknown_ids_are_ignored() {
        let mut q = Queue::new();
        assert!(q.pause(9).is_empty());
        assert!(q.cancel(9, t(0)).is_empty());
        assert!(q.finished(9, t(0)).is_empty());
        assert_eq!(q.conflict_asked(9, ConflictKind::File), None);
        assert!(q.answered(9, Answer::Skip, false, t(0)).is_err());
        q.progress(9, 1, 1, 1, 1, t(0));
    }
}
