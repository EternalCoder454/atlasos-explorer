//! Who may do what with a file, in plain words: the owner's, the group's and
//! everyone else's read, write and run bits of a mode, the sentence that
//! says them, and who may change them. The changes themselves are `attrs`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Who {
    Owner,
    Group,
    Others,
}

impl Who {
    pub const ALL: [Who; 3] = [Who::Owner, Who::Group, Who::Others];

    pub fn from_index(i: u32) -> Option<Who> {
        Who::ALL.get(i as usize).copied()
    }

    fn shift(self) -> u32 {
        match self {
            Who::Owner => 6,
            Who::Group => 3,
            Who::Others => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Access {
    pub read: bool,
    pub write: bool,
    /// Run a file as a program; enter a folder.
    pub run: bool,
}

/// The bits of the permissions of a file: owner, group and others.
pub const RWX: u32 = 0o777;
/// Everything a mode holds that this module changes or keeps: the nine bits,
/// setuid, setgid and sticky.
pub const ALL_BITS: u32 = 0o7777;
/// The run bits.
pub const RUN: u32 = 0o111;

pub fn access(mode: u32, who: Who) -> Access {
    let m = (mode >> who.shift()) & 0o7;
    Access {
        read: m & 4 != 0,
        write: m & 2 != 0,
        run: m & 1 != 0,
    }
}

/// `mode` with the bits of one class set to `a`; the rest (including setuid,
/// setgid and sticky) is kept.
pub fn with_access(mode: u32, who: Who, a: Access) -> u32 {
    let bits = (u32::from(a.read) * 4 + u32::from(a.write) * 2 + u32::from(a.run)) << who.shift();
    (mode & !(0o7 << who.shift())) | bits
}

/// The bits to turn on and off to go from `before` to `after` (only the nine
/// bits): `(set, clear)`.
pub fn difference(before: u32, after: u32) -> (u32, u32) {
    (after & !before & RWX, before & !after & RWX)
}

/// The mode after `set` and `clear`.
pub fn edited(mode: u32, set: u32, clear: u32) -> u32 {
    ((mode | (set & RWX)) & !(clear & RWX)) & ALL_BITS
}

/// What one class may do, in words: "Read and write", "Read only", "No access".
pub fn words(a: Access, is_dir: bool) -> String {
    let parts: Vec<&str> = if is_dir {
        // A folder: reading lists what is in it, writing adds and removes,
        // running (searching) lets you open what is in it.
        match (a.read, a.write, a.run) {
            (true, true, true) => return "View, change and open items".into(),
            (true, false, true) => return "View and open items".into(),
            // Without the run bit a folder can't be entered, so writing is no use.
            (true, _, false) => return "See the names only".into(),
            (false, false, true) => return "Open items whose names are known".into(),
            (false, true, true) => return "Add and remove items, but not see them".into(),
            (false, _, false) => return "No access".into(),
        }
    } else {
        let mut v = Vec::new();
        if a.read {
            v.push("read");
        }
        if a.write {
            v.push("write");
        }
        if a.run {
            v.push("run");
        }
        v
    };
    match parts.as_slice() {
        [] => "No access".into(),
        [one] => {
            if *one == "read" {
                "Read only".into()
            } else {
                capital(&format!("{one} only"))
            }
        }
        [a, b] => capital(&format!("{a} and {b}")),
        [a, b, c] => capital(&format!("{a}, {b} and {c}")),
        _ => String::new(),
    }
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// The whole mode in one sentence: "You can read and write. Your group can
/// read. Everyone else has no access."
pub fn sentence(mode: u32, is_dir: bool, owner_is_you: bool) -> String {
    let one = |who: Who, you: &str| {
        let a = access(mode, who);
        let what = words(a, is_dir);
        format!("{you}: {what}")
    };
    format!(
        "{}. {}. {}.",
        one(Who::Owner, if owner_is_you { "You" } else { "The owner" }),
        one(Who::Group, "Group"),
        one(Who::Others, "Everyone else")
    )
}

/// The mode as the usual three or four octal digits ("644").
pub fn octal(mode: u32) -> String {
    format!("{:o}", mode & ALL_BITS)
}

/// The mode as `rwxr-xr--`.
pub fn symbolic(mode: u32) -> String {
    let mut s = String::new();
    for who in Who::ALL {
        let a = access(mode, who);
        s.push(if a.read { 'r' } else { '-' });
        s.push(if a.write { 'w' } else { '-' });
        s.push(if a.run { 'x' } else { '-' });
    }
    s
}

/// Whether the permissions of an item owned by `owner_uid` can be changed by
/// `uid`: only the owner can (root is never used by Files).
pub fn can_change(owner_uid: u32, uid: u32) -> bool {
    owner_uid == uid
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_classes() {
        let m = 0o640;
        assert_eq!(
            access(m, Who::Owner),
            Access {
                read: true,
                write: true,
                run: false
            }
        );
        assert_eq!(
            access(m, Who::Group),
            Access {
                read: true,
                write: false,
                run: false
            }
        );
        assert_eq!(access(m, Who::Others), Access::default());
    }

    #[test]
    fn changes_one_class_and_keeps_the_rest() {
        let m = with_access(
            0o4640,
            Who::Group,
            Access {
                read: true,
                write: true,
                run: true,
            },
        );
        assert_eq!(m, 0o4670);
        assert_eq!(with_access(0o777, Who::Others, Access::default()), 0o770);
    }

    #[test]
    fn differences_and_edits() {
        assert_eq!(difference(0o644, 0o664), (0o020, 0));
        assert_eq!(difference(0o755, 0o700), (0, 0o055));
        assert_eq!(difference(0o644, 0o755), (0o111, 0o000 | 0o000));
        assert_eq!(edited(0o644, 0o020, 0), 0o664);
        assert_eq!(edited(0o755, 0, 0o055), 0o700);
        // setuid is neither turned on nor off by an edit
        assert_eq!(edited(0o4755, 0, 0o001), 0o4754);
        assert_eq!(edited(0o644, 0o7000, 0), 0o644);
    }

    #[test]
    fn words_for_files() {
        let a = |r, w, x| Access {
            read: r,
            write: w,
            run: x,
        };
        assert_eq!(words(a(true, true, false), false), "Read and write");
        assert_eq!(words(a(true, false, false), false), "Read only");
        assert_eq!(words(a(false, false, false), false), "No access");
        assert_eq!(words(a(true, true, true), false), "Read, write and run");
        assert_eq!(words(a(true, false, true), false), "Read and run");
        assert_eq!(words(a(false, true, false), false), "Write only");
    }

    #[test]
    fn words_for_folders() {
        let a = |r, w, x| Access {
            read: r,
            write: w,
            run: x,
        };
        assert_eq!(
            words(a(true, true, true), true),
            "View, change and open items"
        );
        assert_eq!(words(a(true, false, true), true), "View and open items");
        assert_eq!(words(a(false, false, false), true), "No access");
        assert_eq!(words(a(true, false, false), true), "See the names only");
    }

    #[test]
    fn the_sentence() {
        assert_eq!(
            sentence(0o640, false, true),
            "You: Read and write. Group: Read only. Everyone else: No access."
        );
        assert!(sentence(0o755, true, false).starts_with("The owner: View, change"));
    }

    #[test]
    fn text_forms() {
        assert_eq!(octal(0o644), "644");
        assert_eq!(octal(0o4755), "4755");
        assert_eq!(symbolic(0o754), "rwxr-xr--");
        assert!(can_change(1000, 1000));
        assert!(!can_change(0, 1000));
    }
}
