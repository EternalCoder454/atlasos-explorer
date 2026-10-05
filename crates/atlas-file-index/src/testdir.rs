//! Scratch folders for tests and the benchmark. They live under
//! `$ATLAS_TEST_DIR` (else the system temp dir), never in a real home.

use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

static N: AtomicU32 = AtomicU32::new(0);

/// A random 64-bit value from the kernel; falls back to the clock and the pid.
fn random() -> u64 {
    use std::io::Read;
    let mut b = [0u8; 8];
    if std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .is_ok()
    {
        return u64::from_le_bytes(b);
    }
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    t ^ (u64::from(std::process::id()) << 32)
}

/// A new, empty folder `<base>/atlas-idx-<name>-<pid>-<n>-<random>` (0700).
/// It is created exclusively: a folder that already exists, such as one placed
/// there by someone else, is never reused.
pub fn new(name: &str) -> PathBuf {
    let base = std::env::var_os("ATLAS_TEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    for _ in 0..16 {
        let p = base.join(format!(
            "atlas-idx-{name}-{}-{}-{:08x}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed),
            random() as u32
        ));
        match std::fs::DirBuilder::new().mode(0o700).create(&p) {
            Ok(()) => return p,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => panic!("create test dir {}: {e}", p.display()),
        }
    }
    panic!("no free test dir name under {}", base.display());
}

/// Removes the folder when dropped.
pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new(name: &str) -> Scratch {
        Scratch(new(name))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirs_are_private_and_distinct() {
        use std::os::unix::fs::PermissionsExt;
        let a = Scratch::new("t");
        let b = Scratch::new("t");
        assert_ne!(a.0, b.0);
        let mode = std::fs::metadata(&a.0).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0);
    }
}
