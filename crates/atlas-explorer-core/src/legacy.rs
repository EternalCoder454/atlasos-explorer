//! What Explorer was called before it was Telamon Explorer (`atlas-explorer`),
//! and the once-only moves of the files it kept under that name.
//!
//! A move is a `rename(2)` of a whole folder or one file, never over something
//! that is there, so a file moves whole or not at all and a user's newer file
//! is never replaced. Kept for one release; the app runs it at start
//! (`telamon_adopt_legacy`), and the index service, which cannot write outside
//! its cache folder, only *reads* the old `indexrc` until the app has moved it.

use std::io;
use std::path::{Path, PathBuf};

/// The folder name under the config and cache homes before the rename.
pub const OLD_DIR: &str = "atlas-explorer";
/// ... and after it.
pub const NEW_DIR: &str = "telamon-explorer";

/// Where `new` (a path below a `telamon-explorer` folder) was before the
/// rename: the same path with that folder named `atlas-explorer`. `None` when
/// `new` has no such folder.
pub fn legacy_of(new: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    let mut swapped = false;
    for part in new.components() {
        if !swapped && part.as_os_str() == NEW_DIR {
            out.push(OLD_DIR);
            swapped = true;
        } else {
            out.push(part);
        }
    }
    swapped.then_some(out)
}

fn exists(path: &Path) -> bool {
    path.symlink_metadata().is_ok()
}

/// Moves `from` to `to` unless something is at `to`. A file is linked then
/// unlinked (a link never replaces); a folder is renamed, which only ever
/// replaces an empty folder. True when it moved.
pub fn move_no_replace(from: &Path, to: &Path) -> io::Result<bool> {
    let meta = match from.symlink_metadata() {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    if meta.is_dir() {
        return match std::fs::rename(from, to) {
            Ok(()) => Ok(true),
            // `to` is a folder with something in it (or a file): leave both.
            Err(e) if e.kind() == io::ErrorKind::DirectoryNotEmpty || exists(to) => Ok(false),
            Err(e) => Err(e),
        };
    }
    match std::fs::hard_link(from, to) {
        Ok(()) => {
            std::fs::remove_file(from)?;
            Ok(true)
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        // A file system without links: look first, then rename.
        Err(_) if !exists(to) => std::fs::rename(from, to).map(|()| true),
        Err(_) => Ok(false),
    }
}

/// `<config_home>/atlas-explorer` becomes `<config_home>/telamon-explorer`
/// (it holds `indexrc`). When the new folder exists already, only an `indexrc`
/// the new one lacks is moved into it. True when something moved.
pub fn adopt_config(config_home: &Path) -> io::Result<bool> {
    let old = config_home.join(OLD_DIR);
    let new = config_home.join(NEW_DIR);
    if !exists(&old) {
        return Ok(false);
    }
    if move_no_replace(&old, &new)? {
        return Ok(true);
    }
    if !new.is_dir() {
        return Ok(false);
    }
    let moved = move_no_replace(&old.join("indexrc"), &new.join("indexrc"))?;
    if moved {
        // Only if nothing else is in it.
        let _ = std::fs::remove_dir(&old);
    }
    Ok(moved)
}

/// `<cache_home>/atlas-explorer` (the index snapshot, `index/v1.idx`) becomes
/// `<cache_home>/telamon-explorer`, so the first query after the upgrade is
/// answered from it. When the service has made the new folder first (systemd
/// makes it when the unit starts), the old snapshot is moved into it if the
/// new one has none. Anything else is left as it is: it is a cache, and what
/// stays costs a rescan at worst. True when something moved.
pub fn adopt_cache(cache_home: &Path) -> io::Result<bool> {
    let old = cache_home.join(OLD_DIR);
    let new = cache_home.join(NEW_DIR);
    if !exists(&old) {
        return Ok(false);
    }
    if move_no_replace(&old, &new)? {
        return Ok(true);
    }
    let (from, to) = (old.join("index/v1.idx"), new.join("index/v1.idx"));
    if !new.is_dir() || !exists(&from) || exists(&to) {
        return Ok(false);
    }
    // The service makes its folders 0700 and checks them before it reads.
    let parent = new.join("index");
    if !parent.is_dir() {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().mode(0o700).create(&parent)?;
    }
    let moved = move_no_replace(&from, &to)?;
    if moved {
        let _ = std::fs::remove_dir(old.join("index"));
        let _ = std::fs::remove_dir(&old);
    }
    Ok(moved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(name: &str) -> PathBuf {
        let base = std::env::var_os("ATLAS_TEST_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(format!("legacy-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        base
    }

    #[test]
    fn legacy_path_swaps_the_folder() {
        let p = Path::new("/h/.config/telamon-explorer/indexrc");
        assert_eq!(
            legacy_of(p).unwrap(),
            Path::new("/h/.config/atlas-explorer/indexrc")
        );
        assert_eq!(legacy_of(Path::new("/h/.config/other/indexrc")), None);
    }

    #[test]
    fn config_folder_moves_whole_and_once() {
        let base = scratch("config");
        fs::create_dir_all(base.join("atlas-explorer")).unwrap();
        fs::write(base.join("atlas-explorer/indexrc"), "[Index]\nRoots=/x\n").unwrap();
        assert!(adopt_config(&base).unwrap());
        assert!(!base.join("atlas-explorer").exists());
        assert_eq!(
            fs::read_to_string(base.join("telamon-explorer/indexrc")).unwrap(),
            "[Index]\nRoots=/x\n"
        );
        assert!(!adopt_config(&base).unwrap());
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn config_never_replaces_a_new_indexrc() {
        let base = scratch("config-both");
        fs::create_dir_all(base.join("atlas-explorer")).unwrap();
        fs::create_dir_all(base.join("telamon-explorer")).unwrap();
        fs::write(base.join("atlas-explorer/indexrc"), "old").unwrap();
        fs::write(base.join("telamon-explorer/indexrc"), "new").unwrap();
        assert!(!adopt_config(&base).unwrap());
        assert_eq!(
            fs::read_to_string(base.join("telamon-explorer/indexrc")).unwrap(),
            "new"
        );
        assert_eq!(
            fs::read_to_string(base.join("atlas-explorer/indexrc")).unwrap(),
            "old"
        );
        // A new folder without one takes the old file.
        fs::remove_file(base.join("telamon-explorer/indexrc")).unwrap();
        assert!(adopt_config(&base).unwrap());
        assert_eq!(
            fs::read_to_string(base.join("telamon-explorer/indexrc")).unwrap(),
            "old"
        );
        assert!(!base.join("atlas-explorer").exists());
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn cache_folder_moves_with_its_snapshot() {
        let base = scratch("cache");
        fs::create_dir_all(base.join("atlas-explorer/index")).unwrap();
        fs::write(base.join("atlas-explorer/index/v1.idx"), b"snapshot").unwrap();
        assert!(adopt_cache(&base).unwrap());
        assert_eq!(
            fs::read(base.join("telamon-explorer/index/v1.idx")).unwrap(),
            b"snapshot"
        );
        assert!(!base.join("atlas-explorer").exists());
        assert!(!adopt_cache(&base).unwrap());
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn cache_folder_the_service_made_first() {
        use std::os::unix::fs::PermissionsExt;
        let base = scratch("cache-both");
        fs::create_dir_all(base.join("atlas-explorer/index")).unwrap();
        fs::write(base.join("atlas-explorer/index/v1.idx"), b"old").unwrap();
        // systemd made the new folder (empty, 0700): the old one replaces it.
        fs::create_dir_all(base.join("telamon-explorer")).unwrap();
        assert!(adopt_cache(&base).unwrap());
        assert_eq!(
            fs::read(base.join("telamon-explorer/index/v1.idx")).unwrap(),
            b"old"
        );
        // A folder with something else in it: only the snapshot moves, when
        // there is none, and never over a new one.
        let base2 = scratch("cache-busy");
        fs::create_dir_all(base2.join("atlas-explorer/index")).unwrap();
        fs::write(base2.join("atlas-explorer/index/v1.idx"), b"old").unwrap();
        fs::create_dir_all(base2.join("telamon-explorer")).unwrap();
        fs::write(base2.join("telamon-explorer/other"), b"x").unwrap();
        assert!(adopt_cache(&base2).unwrap());
        assert_eq!(
            fs::read(base2.join("telamon-explorer/index/v1.idx")).unwrap(),
            b"old"
        );
        assert_eq!(
            fs::metadata(base2.join("telamon-explorer/index"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        fs::create_dir_all(base2.join("atlas-explorer/index")).unwrap();
        fs::write(base2.join("atlas-explorer/index/v1.idx"), b"older").unwrap();
        assert!(!adopt_cache(&base2).unwrap());
        assert_eq!(
            fs::read(base2.join("telamon-explorer/index/v1.idx")).unwrap(),
            b"old"
        );
        assert_eq!(
            fs::read(base2.join("atlas-explorer/index/v1.idx")).unwrap(),
            b"older"
        );
        fs::remove_dir_all(&base).unwrap();
        fs::remove_dir_all(&base2).unwrap();
    }
}
