//! Undo and Redo: the last 20 operations that can be reversed, each with the
//! title the user sees ("Move 3 Items to Backup"). No I/O: the app looks at
//! the files, hands the states in, and runs the plan this returns.
//!
//! Recording a new operation ends the redo list. An operation that cannot be
//! undone safely (it replaced or merged files; see `barrier`) empties both
//! lists, so Ctrl+Z never reaches past it and undoes something older while
//! the user thinks it undoes that.

use crate::undo::{FileState, Undo, UndoPlan, UndoRefusal};
use std::collections::HashMap;

/// Entries kept on each side.
pub const MAX_HISTORY: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Undo,
    Redo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// What the operation was, as the user sees it.
    pub title: String,
    /// How to reverse it.
    pub rec: Undo,
}

#[derive(Debug, Default)]
pub struct History {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
}

fn push_bounded(list: &mut Vec<Entry>, entry: Entry) {
    list.push(entry);
    if list.len() > MAX_HISTORY {
        list.remove(0);
    }
}

impl History {
    pub fn new() -> Self {
        History::default()
    }

    fn list(&self, side: Side) -> &Vec<Entry> {
        match side {
            Side::Undo => &self.undo,
            Side::Redo => &self.redo,
        }
    }

    fn list_mut(&mut self, side: Side) -> &mut Vec<Entry> {
        match side {
            Side::Undo => &mut self.undo,
            Side::Redo => &mut self.redo,
        }
    }

    /// A finished operation that can be undone. Ends the redo list.
    pub fn record(&mut self, title: &str, rec: Undo) {
        self.redo.clear();
        push_bounded(
            &mut self.undo,
            Entry {
                title: title.to_string(),
                rec,
            },
        );
    }

    /// A finished operation that cannot be undone safely: nothing before it
    /// can be undone through it either.
    pub fn barrier(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    /// The entry the next Undo (or Redo) runs.
    pub fn top(&self, side: Side) -> Option<&Entry> {
        self.list(side).last()
    }

    /// Titles, newest (next to run) first.
    pub fn titles(&self, side: Side) -> Vec<&str> {
        self.list(side)
            .iter()
            .rev()
            .map(|e| e.title.as_str())
            .collect()
    }

    /// The paths to look at before `plan`.
    pub fn paths(&self, side: Side) -> Vec<String> {
        self.top(side).map(|e| e.rec.paths()).unwrap_or_default()
    }

    /// What the next Undo (or Redo) would do, or why not. `states` holds how
    /// each path of `paths` is now (absent counts as missing).
    pub fn plan(
        &self,
        side: Side,
        states: &HashMap<String, Option<FileState>>,
    ) -> Result<UndoPlan, UndoRefusal> {
        let Some(entry) = self.top(side) else {
            return Err(UndoRefusal {
                reason: match side {
                    Side::Undo => "There is nothing to undo".into(),
                    Side::Redo => "There is nothing to redo".into(),
                },
                path: String::new(),
            });
        };
        entry.rec.check(|p| states.get(p).cloned().flatten())
    }

    /// The paths `complete` will want the states of once the plan has run.
    pub fn paths_after(&self, side: Side) -> Vec<String> {
        self.top(side)
            .map(|e| e.rec.paths_after())
            .unwrap_or_default()
    }

    /// The plan ran: the entry moves to the other side as its inverse.
    /// `states` is how the `paths_after` are now, `trash_urls` where each
    /// trashed path went. When the inverse can't be made the entry is just
    /// dropped (there is no way back). Returns the title.
    pub fn complete(
        &mut self,
        side: Side,
        states: &HashMap<String, Option<FileState>>,
        trash_urls: &HashMap<String, String>,
    ) -> Option<String> {
        let entry = self.list_mut(side).pop()?;
        let inverse = entry.rec.inverse(
            |p| states.get(p).cloned().flatten(),
            |p| trash_urls.get(p).cloned(),
        );
        if let Some(rec) = inverse {
            let other = match side {
                Side::Undo => Side::Redo,
                Side::Redo => Side::Undo,
            };
            push_bounded(
                self.list_mut(other),
                Entry {
                    title: entry.title.clone(),
                    rec,
                },
            );
        }
        Some(entry.title)
    }

    /// The plan can't be run any more (it failed part way, or the files
    /// changed): the entry is dropped so the next press tries the one before.
    pub fn drop_top(&mut self, side: Side) {
        self.list_mut(side).pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::undo::{Item, Moved, StepKind};

    fn file(size: u64, mtime: i64) -> FileState {
        FileState {
            is_dir: false,
            size,
            mtime,
            entries: 0,
        }
    }

    fn mv(from: &str, to: &str) -> Undo {
        Undo::Move {
            moved: vec![Moved {
                from: from.into(),
                to: Item {
                    path: to.into(),
                    state: file(1, 1),
                },
            }],
        }
    }

    fn states(list: &[(&str, Option<FileState>)]) -> HashMap<String, Option<FileState>> {
        list.iter()
            .map(|(p, s)| (p.to_string(), s.clone()))
            .collect()
    }

    #[test]
    fn undo_then_redo_swaps_the_entry_between_sides() {
        let mut h = History::new();
        h.record("Move x to Backup", mv("/a/x", "/b/x"));
        assert_eq!(h.titles(Side::Undo), ["Move x to Backup"]);
        assert!(h.titles(Side::Redo).is_empty());
        assert_eq!(h.paths(Side::Undo), ["/b/x", "/a/x"]);

        // Undo: x is in /b/x, /a/x is free.
        let plan = h
            .plan(
                Side::Undo,
                &states(&[("/b/x", Some(file(1, 1))), ("/a/x", None)]),
            )
            .unwrap();
        assert_eq!(plan.steps[0].kind, StepKind::MoveBack);
        assert_eq!(h.paths_after(Side::Undo), ["/a/x"]);
        let title = h.complete(
            Side::Undo,
            &states(&[("/a/x", Some(file(1, 1)))]),
            &HashMap::new(),
        );
        assert_eq!(title.as_deref(), Some("Move x to Backup"));
        assert!(h.top(Side::Undo).is_none());
        assert_eq!(h.titles(Side::Redo), ["Move x to Backup"]);

        // Redo runs the move again and puts the entry back for Undo.
        let plan = h
            .plan(
                Side::Redo,
                &states(&[("/a/x", Some(file(1, 1))), ("/b/x", None)]),
            )
            .unwrap();
        assert_eq!(plan.steps[0].path, "/a/x");
        assert_eq!(plan.steps[0].to.as_deref(), Some("/b/x"));
        h.complete(
            Side::Redo,
            &states(&[("/b/x", Some(file(1, 1)))]),
            &HashMap::new(),
        );
        assert_eq!(h.titles(Side::Undo), ["Move x to Backup"]);
        assert!(h.titles(Side::Redo).is_empty());
    }

    #[test]
    fn a_new_operation_ends_redo_and_the_list_is_bounded() {
        let mut h = History::new();
        h.record("one", Undo::NewFolder { path: "/1".into() });
        h.complete(Side::Undo, &HashMap::new(), &HashMap::new());
        assert_eq!(h.titles(Side::Redo), ["one"]);
        h.record("two", Undo::NewFolder { path: "/2".into() });
        assert!(h.titles(Side::Redo).is_empty());
        for i in 0..40 {
            h.record(
                &format!("n{i}"),
                Undo::NewFolder {
                    path: format!("/{i}"),
                },
            );
        }
        let titles = h.titles(Side::Undo);
        assert_eq!(titles.len(), MAX_HISTORY);
        assert_eq!(titles[0], "n39");
        assert_eq!(titles[MAX_HISTORY - 1], "n20");
    }

    #[test]
    fn a_barrier_empties_both_sides() {
        let mut h = History::new();
        h.record("a", Undo::NewFolder { path: "/a".into() });
        h.record("b", Undo::NewFolder { path: "/b".into() });
        h.complete(Side::Undo, &HashMap::new(), &HashMap::new());
        h.barrier();
        assert!(h.top(Side::Undo).is_none());
        assert!(h.top(Side::Redo).is_none());
    }

    #[test]
    fn nothing_to_do_and_refusals_read_plainly() {
        let mut h = History::new();
        let e = h.plan(Side::Undo, &HashMap::new()).unwrap_err();
        assert_eq!(e.to_string(), "There is nothing to undo");
        assert_eq!(
            h.plan(Side::Redo, &HashMap::new()).unwrap_err().to_string(),
            "There is nothing to redo"
        );
        h.record("x", mv("/a/x", "/b/x"));
        // The file changed since: refused, and the entry stays until dropped.
        let e = h
            .plan(Side::Undo, &states(&[("/b/x", Some(file(9, 1)))]))
            .unwrap_err();
        assert_eq!(e.reason, "It changed since");
        assert_eq!(e.path, "/b/x");
        assert!(h.top(Side::Undo).is_some());
        h.drop_top(Side::Undo);
        assert!(h.top(Side::Undo).is_none());
    }

    #[test]
    fn an_undo_with_no_way_back_leaves_no_redo() {
        let mut h = History::new();
        // A copy is trashed by its undo; without a trash URL there is no redo.
        h.record(
            "Copy x",
            Undo::Copy {
                created: vec![Item {
                    path: "/b/x".into(),
                    state: file(1, 1),
                }],
            },
        );
        h.complete(Side::Undo, &HashMap::new(), &HashMap::new());
        assert!(h.top(Side::Undo).is_none());
        assert!(h.top(Side::Redo).is_none());
        // With one, the redo restores it from the Trash.
        h.record(
            "Copy x",
            Undo::Copy {
                created: vec![Item {
                    path: "/b/x".into(),
                    state: file(1, 1),
                }],
            },
        );
        let urls: HashMap<String, String> = [("/b/x".to_string(), "trash:/0-x".to_string())].into();
        h.complete(Side::Undo, &HashMap::new(), &urls);
        let plan = h
            .plan(
                Side::Redo,
                &states(&[("/b/x", None), ("trash:/0-x", Some(file(1, 1)))]),
            )
            .unwrap();
        assert_eq!(plan.steps[0].kind, StepKind::RestoreFromTrash);
        assert_eq!(plan.steps[0].to.as_deref(), Some("/b/x"));
    }
}
