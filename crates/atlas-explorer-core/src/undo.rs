//! The undo record of the last finished operation, and the check that says
//! whether undoing it is still safe. Explorer keeps its own record (not
//! KIO's) so it can tell the user exactly what an undo will do, and refuse
//! when files changed since. A permanent delete has no record at all.
//!
//! Paths here are whatever the app uses to name a file (local paths or
//! URLs); this module only compares them as text and asks `stat`.
//!
//! A record says how to undo one finished operation. Running it produces the
//! record that reverses it (`inverse`), which is what Redo runs: copies are
//! trashed and restored, moves go back and forth, a new folder is removed and
//! made again. Nothing here ever deletes for good: the only removals are a
//! trash (recoverable) and an `rmdir` of a folder that is still empty.

use std::fmt;

/// `FileState::entries` of a folder whose entries were not counted (a server,
/// the Trash): never compared.
pub const UNKNOWN_ENTRIES: u64 = u64::MAX;

/// What was seen of a file or folder, recorded at completion and compared
/// at undo time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileState {
    pub is_dir: bool,
    /// Bytes for a file; ignored for a folder.
    pub size: u64,
    /// Modification time in any fixed unit (the app's choice).
    pub mtime: i64,
    /// For a folder, how many entries it holds (`UNKNOWN_ENTRIES` when not
    /// counted); 0 for a file.
    pub entries: u64,
}

impl FileState {
    fn same_as(&self, other: &FileState) -> bool {
        self.is_dir == other.is_dir
            && self.mtime == other.mtime
            && if self.is_dir {
                self.entries == other.entries
                    || self.entries == UNKNOWN_ENTRIES
                    || other.entries == UNKNOWN_ENTRIES
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
    /// A new folder that was removed again: undo (a redo) makes it.
    MakeFolder { path: String },
    /// Trashed: undo restores them.
    Trash { trashed: Vec<Trashed> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepKind {
    TrashCopy,
    MoveBack,
    RemoveEmptyFolder,
    RestoreFromTrash,
    MakeFolder,
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
            StepKind::MakeFolder => "Make the folder again",
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
                Some(s) if s.entries != 0 && s.entries != UNKNOWN_ENTRIES => {
                    return refuse("The folder is no longer empty", path);
                }
                Some(_) => steps.push(Step {
                    kind: StepKind::RemoveEmptyFolder,
                    path: path.clone(),
                    to: None,
                }),
            },
            Undo::MakeFolder { path } => match stat(path) {
                Some(_) => return refuse("Something is already there", path),
                None => steps.push(Step {
                    kind: StepKind::MakeFolder,
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

impl Undo {
    /// Every path `check` asks `stat` about, so the app can look at all of
    /// them (on workers, or through KIO) before calling it.
    pub fn paths(&self) -> Vec<String> {
        let mut out = Vec::new();
        match self {
            Undo::Copy { created } => out.extend(created.iter().map(|i| i.path.clone())),
            Undo::Move { moved } => {
                for m in moved {
                    out.push(m.to.path.clone());
                    out.push(m.from.clone());
                }
            }
            Undo::NewFolder { path } | Undo::MakeFolder { path } => out.push(path.clone()),
            Undo::Trash { trashed } => {
                for t in trashed {
                    out.push(t.in_trash.path.clone());
                    out.push(t.original.clone());
                }
            }
        }
        out
    }

    /// The paths `inverse` asks `stat` about once the undo has run.
    pub fn paths_after(&self) -> Vec<String> {
        match self {
            Undo::Move { moved } => moved.iter().map(|m| m.from.clone()).collect(),
            Undo::Trash { trashed } => trashed.iter().map(|t| t.original.clone()).collect(),
            _ => Vec::new(),
        }
    }

    /// The record that reverses this one, for Redo (and for Undo again after
    /// a Redo). `stat` is how things are now, after running this record;
    /// `trash_url` names the URL a trashed copy was given. None when
    /// something needed is missing: then there is no redo.
    pub fn inverse(
        &self,
        stat: impl Fn(&str) -> Option<FileState>,
        trash_url: impl Fn(&str) -> Option<String>,
    ) -> Option<Undo> {
        match self {
            Undo::Copy { created } => {
                let mut trashed = Vec::new();
                for item in created {
                    trashed.push(Trashed {
                        original: item.path.clone(),
                        in_trash: Item {
                            path: trash_url(&item.path)?,
                            state: item.state.clone(),
                        },
                    });
                }
                (!trashed.is_empty()).then_some(Undo::Trash { trashed })
            }
            Undo::Move { moved } => {
                let mut back = Vec::new();
                for m in moved {
                    back.push(Moved {
                        from: m.to.path.clone(),
                        to: Item {
                            path: m.from.clone(),
                            state: stat(&m.from)?,
                        },
                    });
                }
                (!back.is_empty()).then_some(Undo::Move { moved: back })
            }
            Undo::NewFolder { path } => Some(Undo::MakeFolder { path: path.clone() }),
            Undo::MakeFolder { path } => Some(Undo::NewFolder { path: path.clone() }),
            Undo::Trash { trashed } => {
                let mut created = Vec::new();
                for t in trashed {
                    created.push(Item {
                        path: t.original.clone(),
                        state: stat(&t.original)?,
                    });
                }
                (!created.is_empty()).then_some(Undo::Copy { created })
            }
        }
    }

    /// The record as lines of tab-separated fields (the C ABI's form): the
    /// first line names the kind. None when a path holds a control character
    /// (URLs are percent-encoded, so a real one never does).
    pub fn to_text(&self) -> Option<String> {
        let mut out = String::new();
        // One line: the paths, then the state when there is one.
        let mut push = |paths: &[&str], state: Option<&FileState>| {
            if paths.iter().any(|f| f.chars().any(char::is_control)) {
                return false;
            }
            out.push_str(&paths.join("\t"));
            if let Some(s) = state {
                out.push('\t');
                out.push_str(&state_text(s));
            }
            out.push('\n');
            true
        };
        match self {
            Undo::Copy { created } => {
                push(&["copy"], None);
                for i in created {
                    if !push(&[&i.path], Some(&i.state)) {
                        return None;
                    }
                }
            }
            Undo::Move { moved } => {
                push(&["move"], None);
                for m in moved {
                    if !push(&[&m.from, &m.to.path], Some(&m.to.state)) {
                        return None;
                    }
                }
            }
            Undo::NewFolder { path } => {
                push(&["newfolder"], None);
                if !push(&[path], None) {
                    return None;
                }
            }
            Undo::MakeFolder { path } => {
                push(&["makefolder"], None);
                if !push(&[path], None) {
                    return None;
                }
            }
            Undo::Trash { trashed } => {
                push(&["trash"], None);
                for t in trashed {
                    if !push(&[&t.original, &t.in_trash.path], Some(&t.in_trash.state)) {
                        return None;
                    }
                }
            }
        }
        Some(out)
    }

    /// Reads what `to_text` wrote (and what the app builds): None for
    /// anything malformed, a record with no items included.
    pub fn from_text(text: &str) -> Option<Undo> {
        let mut lines = text.lines();
        let kind = lines.next()?;
        let rows: Vec<Vec<&str>> = lines.map(|l| l.split('\t').collect()).collect();
        let rec = match kind {
            "copy" => Undo::Copy {
                created: rows
                    .iter()
                    .map(|r| {
                        Some(Item {
                            path: nonempty(r.first()?)?,
                            state: parse_state(r.get(1..)?)?,
                        })
                    })
                    .collect::<Option<_>>()?,
            },
            "move" => Undo::Move {
                moved: rows
                    .iter()
                    .map(|r| {
                        Some(Moved {
                            from: nonempty(r.first()?)?,
                            to: Item {
                                path: nonempty(r.get(1)?)?,
                                state: parse_state(r.get(2..)?)?,
                            },
                        })
                    })
                    .collect::<Option<_>>()?,
            },
            "trash" => Undo::Trash {
                trashed: rows
                    .iter()
                    .map(|r| {
                        Some(Trashed {
                            original: nonempty(r.first()?)?,
                            in_trash: Item {
                                path: nonempty(r.get(1)?)?,
                                state: parse_state(r.get(2..)?)?,
                            },
                        })
                    })
                    .collect::<Option<_>>()?,
            },
            "newfolder" | "makefolder" => {
                let [row] = rows.as_slice() else { return None };
                let path = nonempty(row.first()?)?;
                if kind == "newfolder" {
                    Undo::NewFolder { path }
                } else {
                    Undo::MakeFolder { path }
                }
            }
            _ => return None,
        };
        Some(rec)
    }
}

fn nonempty(s: &str) -> Option<String> {
    (!s.is_empty()).then(|| s.to_string())
}

/// `d` or `f`, size, mtime, entries (`-` when not counted), tab-separated.
pub fn state_text(s: &FileState) -> String {
    let entries = if s.entries == UNKNOWN_ENTRIES {
        "-".to_string()
    } else {
        s.entries.to_string()
    };
    format!(
        "{}\t{}\t{}\t{}",
        if s.is_dir { "d" } else { "f" },
        s.size,
        s.mtime,
        entries
    )
}

/// Reads the four fields `state_text` writes.
pub fn parse_state(fields: &[&str]) -> Option<FileState> {
    let [kind, size, mtime, entries] = fields else {
        return None;
    };
    Some(FileState {
        is_dir: match *kind {
            "d" => true,
            "f" => false,
            _ => return None,
        },
        size: size.parse().ok()?,
        mtime: mtime.parse().ok()?,
        entries: if *entries == "-" {
            UNKNOWN_ENTRIES
        } else {
            entries.parse().ok()?
        },
    })
}

/// What the app saw: one line per path, `path<TAB>missing` or
/// `path<TAB>` followed by the four state fields. A path listed twice keeps
/// its last line; lines that don't parse count as missing.
pub fn parse_states(text: &str) -> std::collections::HashMap<String, Option<FileState>> {
    let mut map = std::collections::HashMap::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let Some((path, rest)) = fields.split_first() else {
            continue;
        };
        map.insert(path.to_string(), parse_state(rest));
    }
    map
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
    #[test]
    fn make_folder_needs_a_free_place() {
        let u = Undo::MakeFolder {
            path: "/a/new".into(),
        };
        assert_eq!(
            u.check(fs(&[])).unwrap().steps[0].kind,
            StepKind::MakeFolder
        );
        assert!(u.check(fs(&[("/a/new", dir(0))])).is_err());
    }

    #[test]
    fn unknown_entry_counts_are_not_compared() {
        let unknown = FileState {
            entries: UNKNOWN_ENTRIES,
            ..dir(0)
        };
        let u = Undo::Copy {
            created: vec![item("/b/d", unknown.clone())],
        };
        assert!(u.check(fs(&[("/b/d", dir(5))])).is_ok());
        // Still refused when the folder is not one, or changed in time.
        assert!(u.check(fs(&[("/b/d", file(1, 7))])).is_err());
        let moved = FileState { mtime: 8, ..dir(5) };
        assert!(u.check(fs(&[("/b/d", moved)])).is_err());
        // A new folder of unknown contents is removed by rmdir, which refuses
        // one that is not empty.
        let n = Undo::NewFolder { path: "/n".into() };
        assert!(n.check(fs(&[("/n", unknown)])).is_ok());
    }

    #[test]
    fn inverses_swap_directions() {
        let none = |_: &str| None::<String>;
        // A copy undone is trashed; the inverse restores it from there.
        let copy = Undo::Copy {
            created: vec![item("/b/a", file(3, 1))],
        };
        let inv = copy
            .inverse(|_| None, |p| (p == "/b/a").then(|| "trash:/0-a".into()))
            .unwrap();
        assert_eq!(
            inv,
            Undo::Trash {
                trashed: vec![Trashed {
                    original: "/b/a".into(),
                    in_trash: item("trash:/0-a", file(3, 1)),
                }]
            }
        );
        // ... and the inverse of that is a record of the restored copy.
        let back = inv.inverse(fs(&[("/b/a", file(3, 1))]), none).unwrap();
        assert_eq!(
            back,
            Undo::Copy {
                created: vec![item("/b/a", file(3, 1))]
            }
        );
        // No trash URL known, no redo.
        assert!(copy.inverse(|_| None, none).is_none());
        // A move goes back and forth.
        let mv = Undo::Move {
            moved: vec![Moved {
                from: "/a/x".into(),
                to: item("/b/x", file(1, 1)),
            }],
        };
        let inv = mv.inverse(fs(&[("/a/x", file(1, 1))]), none).unwrap();
        assert_eq!(
            inv,
            Undo::Move {
                moved: vec![Moved {
                    from: "/b/x".into(),
                    to: item("/a/x", file(1, 1)),
                }]
            }
        );
        assert_eq!(inv.inverse(fs(&[("/b/x", file(1, 1))]), none).unwrap(), mv);
        assert!(mv.inverse(fs(&[]), none).is_none());
        // Folders swap.
        let nf = Undo::NewFolder { path: "/n".into() };
        let mf = nf.inverse(fs(&[]), none).unwrap();
        assert_eq!(mf, Undo::MakeFolder { path: "/n".into() });
        assert_eq!(mf.inverse(fs(&[]), none).unwrap(), nf);
    }

    #[test]
    fn paths_cover_what_check_asks() {
        let u = Undo::Move {
            moved: vec![Moved {
                from: "/a/x".into(),
                to: item("/b/x", file(1, 1)),
            }],
        };
        assert_eq!(u.paths(), ["/b/x", "/a/x"]);
        assert_eq!(u.paths_after(), ["/a/x"]);
        let t = Undo::Trash {
            trashed: vec![Trashed {
                original: "/h/f".into(),
                in_trash: item("trash:/f", file(5, 5)),
            }],
        };
        assert_eq!(t.paths(), ["trash:/f", "/h/f"]);
        assert_eq!(t.paths_after(), ["/h/f"]);
        assert!(Undo::Copy { created: vec![] }.paths_after().is_empty());
    }

    #[test]
    fn text_round_trips_and_rejects_junk() {
        let records = [
            Undo::Copy {
                created: vec![
                    item("file:///b/a%20b", file(3, 1)),
                    item("file:///b/d", dir(2)),
                ],
            },
            Undo::Move {
                moved: vec![Moved {
                    from: "file:///a/x".into(),
                    to: item("file:///b/x", file(1, 1)),
                }],
            },
            Undo::NewFolder {
                path: "file:///n".into(),
            },
            Undo::MakeFolder {
                path: "file:///n".into(),
            },
            Undo::Trash {
                trashed: vec![Trashed {
                    original: "file:///h/f".into(),
                    in_trash: item(
                        "trash:/0-f",
                        FileState {
                            entries: UNKNOWN_ENTRIES,
                            ..dir(0)
                        },
                    ),
                }],
            },
        ];
        for r in records {
            let text = r.to_text().unwrap();
            assert_eq!(Undo::from_text(&text), Some(r));
        }
        // A path with a control character is not written at all.
        let bad = Undo::NewFolder {
            path: "/a\tb".into(),
        };
        assert_eq!(bad.to_text(), None);
        for junk in [
            "",
            "nonsense\n",
            "copy\n/a\tx\t1\t1\t0\n",
            "copy\n/a\tf\tnope\t1\t0\n",
            "move\n/a\n",
            "newfolder\n",
            "newfolder\n/a\n/b\n",
            "trash\n\ttrash:/f\tf\t1\t1\t0\n",
        ] {
            assert_eq!(Undo::from_text(junk), None, "{junk:?}");
        }
    }

    #[test]
    fn states_text_reads_missing_and_present() {
        let map = parse_states("/a\tf\t3\t4\t0\n/b\tmissing\n/c\td\t0\t9\t-\n");
        assert_eq!(map["/a"], Some(file(3, 4)));
        assert_eq!(map["/b"], None);
        assert_eq!(map["/c"].as_ref().unwrap().entries, UNKNOWN_ENTRIES);
    }
}
