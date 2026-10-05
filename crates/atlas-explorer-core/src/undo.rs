//! The undo record of the last finished operation, and the check that says
//! whether undoing it is still safe. Explorer keeps its own record (not
//! KIO's) so it can tell the user exactly what an undo will do, and refuse
//! when files changed since. A permanent delete has no record at all.
//!
//! Paths here are whatever the app uses to name a file (local paths or
//! URLs); this module only compares them as text and asks `stat`.

use std::fmt;

/// What was seen of a file or folder, recorded at completion and compared
/// at undo time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileState {
    pub is_dir: bool,
    /// Bytes for a file; ignored for a folder.
    pub size: u64,
    /// Modification time in any fixed unit (the app's choice).
    pub mtime: i64,
    /// For a folder, how many entries it holds; 0 for a file.
    pub entries: u64,
}

impl FileState {
    fn same_as(&self, other: &FileState) -> bool {
        self.is_dir == other.is_dir
            && self.mtime == other.mtime
            && if self.is_dir {
                self.entries == other.entries
            } else {
                self.size == other.size
            }
    }
}

/// A file with the state it was left in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub path: String,
    pub state: FileState,
}

/// One item that moved (a move or a rename).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Moved {
    pub from: String,
    pub to: Item,
}

/// One item that went to the trash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trashed {
    /// Where it was.
    pub original: String,
    /// Where it is now, in the trash.
    pub in_trash: Item,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Undo {
    /// Copies made: undo trashes them.
    Copy { created: Vec<Item> },
    /// Moves and renames: undo moves them back.
    Move { moved: Vec<Moved> },
    /// A folder made: undo removes it if still empty.
    NewFolder { path: String },
    /// Trashed: undo restores them.
    Trash { trashed: Vec<Trashed> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepKind {
    TrashCopy,
    MoveBack,
    RemoveEmptyFolder,
    RestoreFromTrash,
}

/// One thing undo will do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub kind: StepKind,
    /// The file acted on.
    pub path: String,
    /// Where it goes (move back, restore); None otherwise.
    pub to: Option<String>,
}

/// Exactly what an undo will do, for the UI to say before and after.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoPlan {
    pub steps: Vec<Step>,
}

impl UndoPlan {
    /// A one-line summary in plain words, naming at most the first path.
    pub fn summary(&self) -> String {
        let n = self.steps.len();
        let Some(first) = self.steps.first() else {
            return "Nothing to undo.".into();
        };
        let what = match first.kind {
            StepKind::TrashCopy => "Move the copies to the Trash",
            StepKind::MoveBack => "Move back to where they were",
            StepKind::RemoveEmptyFolder => "Remove the new folder",
            StepKind::RestoreFromTrash => "Restore from the Trash",
        };
        if n == 1 {
            format!("{what}: {}", first.path)
        } else {
            format!("{what}: {n} items")
        }
    }
}

/// Why an undo is refused, in plain words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoRefusal {
    pub reason: String,
    /// The file that made it unsafe; empty when none.
    pub path: String,
}

impl fmt::Display for UndoRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            f.write_str(&self.reason)
        } else {
            write!(f, "{}: {}", self.reason, self.path)
        }
    }
}

fn refuse<T>(reason: &str, path: &str) -> Result<T, UndoRefusal> {
    Err(UndoRefusal {
        reason: reason.to_string(),
        path: path.to_string(),
    })
}

/// The file must still be there exactly as it was left.
fn unchanged(item: &Item, stat: &impl Fn(&str) -> Option<FileState>) -> Result<(), UndoRefusal> {
    match stat(&item.path) {
        None => refuse("It is gone or was moved", &item.path),
        Some(now) if !now.same_as(&item.state) => refuse("It changed since", &item.path),
        Some(_) => Ok(()),
    }
}

impl Undo {
    /// Decides whether the undo is still safe. `stat` returns a file's
    /// current state, or None when it doesn't exist (or can't be read, which
    /// is refused like a missing file). Does nothing to the files.
    pub fn check(&self, stat: impl Fn(&str) -> Option<FileState>) -> Result<UndoPlan, UndoRefusal> {
        let mut steps = Vec::new();
        match self {
            Undo::Copy { created } => {
                for item in created {
                    unchanged(item, &stat)?;
                    steps.push(Step {
                        kind: StepKind::TrashCopy,
                        path: item.path.clone(),
                        to: None,
                    });
                }
            }
            Undo::Move { moved } => {
                for m in moved {
                    unchanged(&m.to, &stat)?;
                    if stat(&m.from).is_some() {
                        return refuse("Something is already where it came from", &m.from);
                    }
                    steps.push(Step {
                        kind: StepKind::MoveBack,
                        path: m.to.path.clone(),
                        to: Some(m.from.clone()),
                    });
                }
            }
            Undo::NewFolder { path } => match stat(path) {
                None => return refuse("The folder is gone", path),
                Some(s) if !s.is_dir => return refuse("It is no longer a folder", path),
                Some(s) if s.entries != 0 => return refuse("The folder is no longer empty", path),
                Some(_) => steps.push(Step {
                    kind: StepKind::RemoveEmptyFolder,
                    path: path.clone(),
                    to: None,
                }),
            },
            Undo::Trash { trashed } => {
                for t in trashed {
                    unchanged(&t.in_trash, &stat)?;
                    if stat(&t.original).is_some() {
                        return refuse("Something is already where it was", &t.original);
                    }
                    steps.push(Step {
                        kind: StepKind::RestoreFromTrash,
                        path: t.in_trash.path.clone(),
                        to: Some(t.original.clone()),
                    });
                }
            }
        }
        if steps.is_empty() {
            return refuse("There is nothing to undo", "");
        }
        Ok(UndoPlan { steps })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn file(size: u64, mtime: i64) -> FileState {
        FileState {
            is_dir: false,
            size,
            mtime,
            entries: 0,
        }
    }

    fn dir(entries: u64) -> FileState {
        FileState {
            is_dir: true,
            size: 4096,
            mtime: 7,
            entries,
        }
    }

    fn item(path: &str, state: FileState) -> Item {
        Item {
            path: path.into(),
            state,
        }
    }

    fn fs(entries: &[(&str, FileState)]) -> impl Fn(&str) -> Option<FileState> {
        let map: HashMap<String, FileState> = entries
            .iter()
            .map(|(p, s)| (p.to_string(), s.clone()))
            .collect();
        move |p| map.get(p).cloned()
    }

    #[test]
    fn copy_trashes_unchanged_copies() {
        let u = Undo::Copy {
            created: vec![item("/b/a", file(3, 1)), item("/b/d", dir(2))],
        };
        let plan = u
            .check(fs(&[("/b/a", file(3, 1)), ("/b/d", dir(2))]))
            .unwrap();
        assert_eq!(plan.steps.len(), 2);
        assert!(
            plan.steps
                .iter()
                .all(|s| s.kind == StepKind::TrashCopy && s.to.is_none())
        );
        assert_eq!(plan.summary(), "Move the copies to the Trash: 2 items");
        // Changed size, mtime, a folder that gained an entry, or gone: refused.
        for changed in [file(4, 1), file(3, 2)] {
            let e = u
                .check(fs(&[("/b/a", changed), ("/b/d", dir(2))]))
                .unwrap_err();
            assert_eq!(e.path, "/b/a");
        }
        let e = u
            .check(fs(&[("/b/a", file(3, 1)), ("/b/d", dir(3))]))
            .unwrap_err();
        assert_eq!(e.path, "/b/d");
        let e = u.check(fs(&[("/b/a", file(3, 1))])).unwrap_err();
        assert_eq!(e.reason, "It is gone or was moved");
    }

    #[test]
    fn move_back_needs_a_free_origin() {
        let u = Undo::Move {
            moved: vec![Moved {
                from: "/a/x".into(),
                to: item("/b/x", file(1, 1)),
            }],
        };
        let plan = u.check(fs(&[("/b/x", file(1, 1))])).unwrap();
        assert_eq!(plan.steps[0].kind, StepKind::MoveBack);
        assert_eq!(plan.steps[0].to.as_deref(), Some("/a/x"));
        assert_eq!(plan.summary(), "Move back to where they were: /b/x");
        let e = u
            .check(fs(&[("/b/x", file(1, 1)), ("/a/x", file(9, 9))]))
            .unwrap_err();
        assert_eq!(e.path, "/a/x");
        assert!(u.check(fs(&[])).is_err());
        assert!(u.check(fs(&[("/b/x", file(2, 1))])).is_err());
    }

    #[test]
    fn new_folder_only_if_empty() {
        let u = Undo::NewFolder {
            path: "/a/new".into(),
        };
        assert_eq!(
            u.check(fs(&[("/a/new", dir(0))])).unwrap().steps[0].kind,
            StepKind::RemoveEmptyFolder
        );
        assert!(u.check(fs(&[("/a/new", dir(1))])).is_err());
        assert!(u.check(fs(&[("/a/new", file(0, 0))])).is_err());
        assert!(u.check(fs(&[])).is_err());
    }

    #[test]
    fn trash_restores_to_a_free_place() {
        let u = Undo::Trash {
            trashed: vec![Trashed {
                original: "/h/f".into(),
                in_trash: item("trash:/f", file(5, 5)),
            }],
        };
        let plan = u.check(fs(&[("trash:/f", file(5, 5))])).unwrap();
        assert_eq!(plan.steps[0].kind, StepKind::RestoreFromTrash);
        assert_eq!(plan.steps[0].to.as_deref(), Some("/h/f"));
        assert!(
            u.check(fs(&[("trash:/f", file(5, 5)), ("/h/f", file(1, 1))]))
                .is_err()
        );
        assert!(u.check(fs(&[])).is_err());
    }

    #[test]
    fn empty_records_are_refused_and_refusals_read_plainly() {
        let e = Undo::Copy { created: vec![] }.check(fs(&[])).unwrap_err();
        assert_eq!(e.to_string(), "There is nothing to undo");
        let e = Undo::NewFolder { path: "/x".into() }
            .check(fs(&[]))
            .unwrap_err();
        assert_eq!(e.to_string(), "The folder is gone: /x");
    }
}
