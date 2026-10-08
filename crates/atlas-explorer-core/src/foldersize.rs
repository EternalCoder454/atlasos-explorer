//! The size of a folder and what is in it, worked out only when someone asks
//! (Properties), on a worker, and stoppable. Links are not followed, other
//! file systems are not entered, and a file with several names is counted
//! once.

use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Totals {
    pub files: u64,
    pub folders: u64,
    /// The sum of the files' sizes.
    pub bytes: u64,
    /// What they take on the disk (blocks).
    pub on_disk: u64,
    /// Folders that could not be read.
    pub unreadable: u64,
    /// Folders on other file systems, left out.
    pub skipped: u64,
}

impl Totals {
    pub fn add(&mut self, o: &Totals) {
        self.files += o.files;
        self.folders += o.folders;
        self.bytes += o.bytes;
        self.on_disk += o.on_disk;
        self.unreadable += o.unreadable;
        self.skipped += o.skipped;
    }
}

/// Counts `root` (a file, a link or a folder). `progress` gets the totals so
/// far every few hundred entries; it returns nothing. `Err(totals so far)`
/// when `cancel` was set.
pub fn measure(
    root: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(&Totals),
) -> Result<Totals, Totals> {
    let mut t = Totals::default();
    // A link to a folder, asked about itself, is counted as the folder it
    // leads to (what it shows as); links inside are never followed.
    let root = &fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let Ok(md) = fs::symlink_metadata(root) else {
        t.unreadable += 1;
        return Ok(t);
    };
    let dev = md.dev();
    let mut seen: HashSet<(u64, u64)> = HashSet::new();
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    let mut tick = 0u32;
    while let Some(path) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            return Err(t);
        }
        let Ok(md) = fs::symlink_metadata(&path) else {
            t.unreadable += 1;
            continue;
        };
        tick += 1;
        if tick.is_multiple_of(500) {
            progress(&t);
        }
        if md.is_dir() {
            if md.dev() != dev {
                t.skipped += 1;
                continue;
            }
            // The folder asked about is not counted in its own "folders".
            if path != *root {
                t.folders += 1;
            }
            t.on_disk += md.blocks() * 512;
            match fs::read_dir(&path) {
                Ok(rd) => stack.extend(rd.flatten().map(|e| e.path())),
                Err(_) => t.unreadable += 1,
            }
        } else {
            // A file with more than one name counts once.
            if md.nlink() > 1 && !seen.insert((md.dev(), md.ino())) {
                continue;
            }
            t.files += 1;
            if md.is_file() {
                t.bytes += md.len();
            }
            t.on_disk += md.blocks() * 512;
        }
    }
    progress(&t);
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_files_folders_and_bytes() {
        let dir = std::env::temp_dir().join(format!("telamon-fsize-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("a/b")).unwrap();
        fs::write(dir.join("one"), vec![0u8; 1000]).unwrap();
        fs::write(dir.join("a/two"), vec![0u8; 24]).unwrap();
        fs::write(dir.join("a/b/three"), b"").unwrap();
        std::os::unix::fs::symlink("/usr", dir.join("link")).unwrap();
        fs::hard_link(dir.join("one"), dir.join("a/one-again")).unwrap();
        let no = AtomicBool::new(false);
        let t = measure(&dir, &no, &mut |_| {}).unwrap();
        // The link counts as a file of its own and is not followed.
        assert_eq!(t.folders, 2);
        assert_eq!(t.files, 4);
        assert!(t.bytes >= 1024 && t.bytes < 1100, "{}", t.bytes);
        assert_eq!(t.unreadable, 0);
        assert!(t.on_disk >= 4096);
        // A file alone.
        let one = measure(&dir.join("a/two"), &no, &mut |_| {}).unwrap();
        assert_eq!((one.files, one.folders, one.bytes), (1, 0, 24));
        let yes = AtomicBool::new(true);
        assert!(measure(&dir, &yes, &mut |_| {}).is_err());
        // A link to a folder asked about itself counts the folder.
        let to_a = dir.join("to-a");
        std::os::unix::fs::symlink(dir.join("a"), &to_a).unwrap();
        let via = measure(&to_a, &no, &mut |_| {}).unwrap();
        assert_eq!((via.files, via.folders), (3, 1));
        let gone = measure(&dir.join("nope"), &no, &mut |_| {}).unwrap();
        assert_eq!(gone.unreadable, 1);
        fs::remove_dir_all(&dir).unwrap();
    }
}
