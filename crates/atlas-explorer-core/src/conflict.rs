//! A name already taken at the destination: which answers the dialog may
//! offer, which of the two files is newer, and what the answer means to KIO.
//! Replacing a folder with a file (or the reverse) is never offered, and a
//! file is never "merged".

use crate::queue::{Answer, ConflictKind};

/// What is being copied or moved, and what is in the way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Case {
    pub source_is_dir: bool,
    pub dest_is_dir: bool,
    /// The two are the same file (a paste into the folder it came from).
    pub same_file: bool,
}

impl Case {
    /// The question the queue's "apply to all" remembers: one answer for
    /// every file conflict, another for every folder conflict.
    pub fn kind(self) -> ConflictKind {
        if self.source_is_dir && self.dest_is_dir {
            ConflictKind::Folder
        } else {
            ConflictKind::File
        }
    }
}

/// The buttons the dialog shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Choices {
    pub replace: bool,
    pub merge: bool,
    pub keep_both: bool,
    pub skip: bool,
    /// The one that starts with the focus: never the one that destroys.
    pub default: Answer,
}

pub fn choices(case: Case) -> Choices {
    match (case.source_is_dir, case.dest_is_dir) {
        // Folder onto folder: merge them, or leave the folder alone. Merging
        // a folder into itself is meaningless: a copy of it keeps both.
        (true, true) => Choices {
            replace: false,
            merge: !case.same_file,
            keep_both: case.same_file,
            skip: true,
            default: if case.same_file {
                Answer::KeepBoth
            } else {
                Answer::Skip
            },
        },
        // File onto file: replace, skip, or keep both. A file can't replace
        // itself.
        (false, false) => Choices {
            replace: !case.same_file,
            merge: false,
            keep_both: true,
            skip: true,
            default: Answer::KeepBoth,
        },
        // A file where a folder is (or the reverse): replacing would throw a
        // whole folder away, or fill it with one file; only these are safe.
        _ => Choices {
            replace: false,
            merge: false,
            keep_both: true,
            skip: true,
            default: Answer::KeepBoth,
        },
    }
}

/// Whether `answer` is one the case offers.
pub fn allowed(case: Case, answer: Answer) -> bool {
    let c = choices(case);
    match answer {
        Answer::Replace => c.replace,
        Answer::Merge => c.merge,
        Answer::KeepBoth => c.keep_both,
        Answer::Skip => c.skip,
    }
}

/// Which of the two files was changed last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Newer {
    Source,
    Destination,
    /// Same time to the second, or one of them is not known.
    Neither,
}

pub fn newer(source_mtime: Option<i64>, dest_mtime: Option<i64>) -> Newer {
    match (source_mtime, dest_mtime) {
        (Some(s), Some(d)) if s > d => Newer::Source,
        (Some(s), Some(d)) if d > s => Newer::Destination,
        _ => Newer::Neither,
    }
}

/// Whether a replace or a merge happened: the result then holds files that
/// were there before the operation, so undoing it by trashing what it
/// "created" could take user data along. Only an answer that leaves the
/// existing file alone is safe.
pub fn endangers_undo(answer: Answer) -> bool {
    matches!(answer, Answer::Replace | Answer::Merge)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case(source_is_dir: bool, dest_is_dir: bool, same_file: bool) -> Case {
        Case {
            source_is_dir,
            dest_is_dir,
            same_file,
        }
    }

    #[test]
    fn files_offer_replace_skip_and_keep_both() {
        let c = choices(case(false, false, false));
        assert!(c.replace && c.skip && c.keep_both && !c.merge);
        assert_eq!(c.default, Answer::KeepBoth);
        assert_eq!(case(false, false, false).kind(), ConflictKind::File);
    }

    #[test]
    fn folders_offer_merge_and_skip_with_skip_first() {
        let c = choices(case(true, true, false));
        assert!(c.merge && c.skip && !c.replace && !c.keep_both);
        assert_eq!(c.default, Answer::Skip);
        assert_eq!(case(true, true, false).kind(), ConflictKind::Folder);
        assert!(!allowed(case(true, true, false), Answer::Replace));
    }

    #[test]
    fn a_file_never_replaces_itself_or_a_folder() {
        for c in [
            case(false, false, true),
            case(false, true, false),
            case(true, false, false),
        ] {
            let ch = choices(c);
            assert!(!ch.replace && !ch.merge, "{c:?}");
            assert!(ch.skip && ch.keep_both);
            assert_eq!(ch.default, Answer::KeepBoth);
        }
        // A folder into itself can't merge.
        let ch = choices(case(true, true, true));
        assert!(!ch.merge && ch.keep_both);
    }

    #[test]
    fn newer_compares_whole_seconds() {
        assert_eq!(newer(Some(10), Some(5)), Newer::Source);
        assert_eq!(newer(Some(5), Some(10)), Newer::Destination);
        assert_eq!(newer(Some(5), Some(5)), Newer::Neither);
        assert_eq!(newer(None, Some(5)), Newer::Neither);
        assert_eq!(newer(Some(5), None), Newer::Neither);
    }

    #[test]
    fn only_replace_and_merge_endanger_an_undo() {
        assert!(endangers_undo(Answer::Replace));
        assert!(endangers_undo(Answer::Merge));
        assert!(!endangers_undo(Answer::Skip));
        assert!(!endangers_undo(Answer::KeepBoth));
    }
}
