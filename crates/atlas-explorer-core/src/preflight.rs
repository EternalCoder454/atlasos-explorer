//! Checks made before a copy or move starts, so a doomed one is refused up
//! front with the numbers in plain words: a folder into itself, and a
//! destination without the room. Local files only (a server has no honest
//! free-space answer); everything else is left to the job.
//!
//! These read the disk (`stat`, `statvfs`, a walk of the sources), so the app
//! runs them on a worker.

use crate::optext::{format_size, short_name};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// Entries looked at when adding up the size of the sources; past this the
/// total so far is used.
pub const MAX_WALK_ENTRIES: u64 = 2_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transfer {
    Copy,
    Move,
}

impl Transfer {
    fn verb(self) -> &'static str {
        match self {
            Transfer::Copy => "copy",
            Transfer::Move => "move",
        }
    }
    fn gerund(self) -> &'static str {
        match self {
            Transfer::Copy => "Copying",
            Transfer::Move => "Moving",
        }
    }
}

/// Whether `dest` is `source` or inside it, comparing text: both are
/// absolute and clean (no `.`/`..`, no trailing slash), as the app makes
/// them. Works for local paths and for URLs (scheme and host first).
pub fn is_inside(source: &str, dest: &str) -> bool {
    let source = source.trim_end_matches('/');
    let dest = dest.trim_end_matches('/');
    dest == source
        || (dest.len() > source.len()
            && dest.starts_with(source)
            && dest.as_bytes()[source.len()] == b'/')
}

/// The message for a folder that would go into itself.
pub fn into_itself_text(transfer: Transfer, folder_name: &str, dest_name: &str) -> String {
    format!(
        "Can't {} \"{}\" into itself. The folder you chose, \"{}\", is the same folder or inside it.",
        transfer.verb(),
        short_name(folder_name),
        short_name(dest_name)
    )
}

/// The message for a destination without room, with the numbers.
pub fn no_room_text(
    transfer: Transfer,
    what: &str,
    dest_name: &str,
    needed: u64,
    free: u64,
) -> String {
    format!(
        "There isn't enough space in \"{}\". {} {} needs {}, and only {} is free.",
        short_name(dest_name),
        transfer.gerund(),
        what,
        format_size(needed),
        format_size(free)
    )
}

/// The size of everything under `paths` (symbolic links count as themselves,
/// not what they point to), and whether the walk was complete.
pub fn tree_size(paths: &[PathBuf]) -> (u64, bool) {
    let mut total = 0u64;
    let mut seen = 0u64;
    let mut stack: Vec<PathBuf> = paths.to_vec();
    while let Some(p) = stack.pop() {
        seen += 1;
        if seen > MAX_WALK_ENTRIES {
            return (total, false);
        }
        let Ok(meta) = std::fs::symlink_metadata(&p) else {
            continue;
        };
        if meta.is_dir() {
            if let Ok(rd) = std::fs::read_dir(&p) {
                stack.extend(rd.flatten().map(|e| e.path()));
            }
        } else {
            total = total.saturating_add(meta.len());
        }
    }
    (total, true)
}

/// Free bytes for an unprivileged user on the filesystem holding `path`;
/// None when it can't be asked or the filesystem doesn't say (no blocks).
pub fn free_space(path: &Path) -> Option<u64> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let c = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut st = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
    // SAFETY: `c` is a valid C string and `st` is writable for one statvfs.
    let rc = unsafe { libc::statvfs(c.as_ptr(), st.as_mut_ptr()) };
    if rc != 0 {
        return None;
    }
    // SAFETY: statvfs returned 0, so it filled the struct.
    let st = unsafe { st.assume_init() };
    if st.f_blocks == 0 {
        return None;
    }
    Some((st.f_bavail as u64).saturating_mul(st.f_frsize as u64))
}

fn name_of(p: &Path) -> String {
    p.file_name()
        .map(|n| crate::display_name(n.as_encoded_bytes()))
        .unwrap_or_else(|| crate::display_name(p.as_os_str().as_encoded_bytes()))
}

/// Looks at a local copy or move before it starts. `free_override` stands in
/// for the filesystem's answer (tests, and a mocked full disk). Err is the
/// refusal in plain words.
pub fn check_local(
    transfer: Transfer,
    sources: &[PathBuf],
    dest: &Path,
    free_override: Option<u64>,
) -> Result<(), String> {
    let Ok(real_dest) = std::fs::canonicalize(dest) else {
        // Not there: the job will say so.
        return Ok(());
    };
    let dest_name = name_of(&real_dest);
    // A folder into itself, through links too.
    for s in sources {
        let Ok(meta) = std::fs::symlink_metadata(s) else {
            continue;
        };
        if !meta.is_dir() {
            continue;
        }
        if let Ok(real) = std::fs::canonicalize(s)
            && real_dest.starts_with(&real)
        {
            return Err(into_itself_text(transfer, &name_of(s), &dest_name));
        }
    }
    // Room: a move on the same disk is a rename and needs none.
    let dest_dev = std::fs::metadata(&real_dest).map(|m| m.dev()).ok();
    let moving_on_disk = |s: &PathBuf| {
        transfer == Transfer::Move
            && std::fs::symlink_metadata(s).map(|m| m.dev()).ok() == dest_dev
            && dest_dev.is_some()
    };
    let to_copy: Vec<PathBuf> = sources
        .iter()
        .filter(|s| !moving_on_disk(s))
        .cloned()
        .collect();
    if to_copy.is_empty() {
        return Ok(());
    }
    let Some(free) = free_override.or_else(|| free_space(&real_dest)) else {
        return Ok(());
    };
    let (needed, _complete) = tree_size(&to_copy);
    if needed > free {
        let what = if to_copy.len() == 1 {
            format!("\"{}\"", short_name(&name_of(&to_copy[0])))
        } else {
            format!("{} items", to_copy.len())
        };
        return Err(no_room_text(transfer, &what, &dest_name, needed, free));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct Dir(PathBuf);
    impl Dir {
        fn new(tag: &str) -> Dir {
            let p = std::env::temp_dir().join(format!(
                "telamon-preflight-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = fs::remove_dir_all(&p);
            fs::create_dir_all(&p).unwrap();
            Dir(p)
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn inside_is_by_whole_path_parts() {
        assert!(is_inside("/a/b", "/a/b"));
        assert!(is_inside("/a/b", "/a/b/c/d"));
        assert!(is_inside("/a/b/", "/a/b/c"));
        assert!(!is_inside("/a/b", "/a/bc"));
        assert!(!is_inside("/a/b", "/a"));
        assert!(!is_inside("/a/b", "/x/a/b"));
        assert!(is_inside("smb://host/share/a", "smb://host/share/a/b"));
        assert!(!is_inside("smb://host/share/a", "smb://other/share/a/b"));
    }

    #[test]
    fn a_folder_into_itself_is_refused_with_its_name() {
        let d = Dir::new("itself");
        let a = d.0.join("Projects");
        let old = a.join("Old");
        fs::create_dir_all(&old).unwrap();
        for dest in [&a, &old] {
            let e = check_local(Transfer::Move, std::slice::from_ref(&a), dest, None).unwrap_err();
            assert!(e.contains("\"Projects\""), "{e}");
            assert!(e.starts_with("Can't move"), "{e}");
        }
        let e = check_local(Transfer::Copy, std::slice::from_ref(&a), &old, None).unwrap_err();
        assert!(e.starts_with("Can't copy"), "{e}");
        // A sibling is fine, and so is a file next to a folder of the same prefix.
        let sib = d.0.join("Projects2");
        fs::create_dir_all(&sib).unwrap();
        assert!(check_local(Transfer::Move, std::slice::from_ref(&a), &sib, None).is_ok());
    }

    #[test]
    fn a_link_to_the_folder_does_not_hide_it() {
        let d = Dir::new("link");
        let a = d.0.join("A");
        fs::create_dir_all(a.join("sub")).unwrap();
        let link = d.0.join("alias");
        std::os::unix::fs::symlink(&a, &link).unwrap();
        assert!(
            check_local(
                Transfer::Move,
                std::slice::from_ref(&a),
                &link.join("sub"),
                None
            )
            .is_err()
        );
        // A symbolic link moved into the folder it points to is just a link.
        assert!(check_local(Transfer::Move, std::slice::from_ref(&link), &a, None).is_ok());
    }

    #[test]
    fn no_room_gives_the_numbers() {
        let d = Dir::new("room");
        let src = d.0.join("big.bin");
        fs::write(&src, vec![0u8; 3 * 1024 * 1024]).unwrap();
        let dest = d.0.join("Backup");
        fs::create_dir_all(&dest).unwrap();
        let e = check_local(
            Transfer::Copy,
            std::slice::from_ref(&src),
            &dest,
            Some(1024 * 1024),
        )
        .unwrap_err();
        assert_eq!(
            e,
            "There isn't enough space in \"Backup\". Copying \"big.bin\" needs 3 MiB, and only 1 MiB is free."
        );
        // Enough room passes; so does exactly enough.
        assert!(
            check_local(
                Transfer::Copy,
                std::slice::from_ref(&src),
                &dest,
                Some(3 * 1024 * 1024)
            )
            .is_ok()
        );
        // Many items are counted, not named.
        let two = d.0.join("two.bin");
        fs::write(&two, vec![0u8; 1024]).unwrap();
        let e = check_local(Transfer::Copy, &[src.clone(), two], &dest, Some(10)).unwrap_err();
        assert!(e.contains("Copying 2 items needs"), "{e}");
    }

    #[test]
    fn a_move_on_the_same_disk_needs_no_room() {
        let d = Dir::new("rename");
        let src = d.0.join("big.bin");
        fs::write(&src, vec![0u8; 4096]).unwrap();
        let dest = d.0.join("Backup");
        fs::create_dir_all(&dest).unwrap();
        assert!(check_local(Transfer::Move, std::slice::from_ref(&src), &dest, Some(0)).is_ok());
        assert!(check_local(Transfer::Copy, std::slice::from_ref(&src), &dest, Some(0)).is_err());
    }

    #[test]
    fn a_real_disk_is_asked_and_a_missing_destination_is_left_to_the_job() {
        let d = Dir::new("real");
        assert!(free_space(&d.0).is_some());
        assert!(free_space(Path::new("/nonexistent-telamon")).is_none());
        let src = d.0.join("f");
        fs::write(&src, b"x").unwrap();
        assert!(check_local(Transfer::Copy, &[src], &d.0.join("missing"), Some(0)).is_ok());
    }

    #[test]
    fn tree_size_adds_files_and_counts_links_as_links() {
        let d = Dir::new("tree");
        fs::create_dir_all(d.0.join("a/b")).unwrap();
        fs::write(d.0.join("a/x"), vec![0u8; 100]).unwrap();
        fs::write(d.0.join("a/b/y"), vec![0u8; 50]).unwrap();
        std::os::unix::fs::symlink("/dev/zero", d.0.join("a/zero")).unwrap();
        let (n, complete) = tree_size(&[d.0.join("a")]);
        assert!(complete);
        // Two files and one link of 9 bytes ("/dev/zero").
        assert_eq!(n, 100 + 50 + 9);
    }
}
