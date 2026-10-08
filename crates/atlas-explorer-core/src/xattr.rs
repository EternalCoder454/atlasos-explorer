//! Extended attributes of files on this computer: the `user.*` ones Files
//! keeps tags and the star rating in. Plain `lgetxattr`/`lsetxattr` calls
//! that never follow a link, with the errors sorted into the few things the
//! window says in words. Blocking: call from a worker.

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

/// The freedesktop tags: a comma-separated list of names (Baloo and Dolphin
/// read and write the same attribute).
pub const TAGS: &str = "user.xdg.tags";
/// Baloo's star rating, a decimal number 0 to 10 written as text.
pub const RATING: &str = "user.baloo.rating";
/// The longest value read; a longer one is refused, not cut.
pub const MAX_VALUE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The file system keeps no extended attributes (FAT, many network shares).
    Unsupported,
    /// The file is a link: user attributes can't be put on one.
    Link,
    /// Not allowed (a read-only file or file system, not yours).
    Denied,
    /// The value does not fit.
    TooBig,
    /// The file is gone.
    Gone,
    Other(i32),
}

impl Error {
    fn from_errno(e: i32) -> Error {
        match e {
            libc::ENOTSUP => Error::Unsupported,
            libc::EACCES | libc::EPERM | libc::EROFS => Error::Denied,
            libc::ENOSPC | libc::E2BIG | libc::ERANGE | libc::EDQUOT => Error::TooBig,
            libc::ENOENT | libc::ENOTDIR => Error::Gone,
            other => Error::Other(other),
        }
    }

    /// A sentence for the window.
    pub fn text(self) -> &'static str {
        match self {
            Error::Unsupported => "This location can't keep tags.",
            Error::Link => "Links can't have tags.",
            Error::Denied => "You aren't allowed to change this item.",
            Error::TooBig => "There is no room to keep more tags on this item.",
            Error::Gone => "The item is gone.",
            Error::Other(_) => "The change couldn't be saved.",
        }
    }
}

fn cstr(path: &Path, name: &str) -> Result<(CString, CString), Error> {
    let p = CString::new(path.as_os_str().as_bytes()).map_err(|_| Error::Gone)?;
    let n = CString::new(name).map_err(|_| Error::Other(libc::EINVAL))?;
    Ok((p, n))
}

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// The value of attribute `name`, `None` when the file has none.
pub fn get(path: &Path, name: &str) -> Result<Option<Vec<u8>>, Error> {
    let (p, n) = cstr(path, name)?;
    let mut buf = vec![0u8; 256];
    loop {
        // SAFETY: `p` and `n` are NUL-terminated and `buf` is writable for its length.
        let r =
            unsafe { libc::lgetxattr(p.as_ptr(), n.as_ptr(), buf.as_mut_ptr().cast(), buf.len()) };
        if r >= 0 {
            buf.truncate(r as usize);
            return Ok(Some(buf));
        }
        match errno() {
            libc::ENODATA => return Ok(None),
            libc::ERANGE if buf.len() < MAX_VALUE => buf = vec![0u8; MAX_VALUE],
            // A value bigger than we read: not ours to cut.
            libc::ERANGE => return Err(Error::TooBig),
            e => return Err(Error::from_errno(e)),
        }
    }
}

/// Whether `path` itself is a link (not followed).
pub fn is_link(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
}

/// Sets attribute `name` to `value`; an empty `value` removes it.
pub fn set(path: &Path, name: &str, value: &[u8]) -> Result<(), Error> {
    if is_link(path) {
        return Err(Error::Link);
    }
    let (p, n) = cstr(path, name)?;
    if value.is_empty() {
        // SAFETY: both strings are NUL-terminated.
        let r = unsafe { libc::lremovexattr(p.as_ptr(), n.as_ptr()) };
        if r == 0 || errno() == libc::ENODATA {
            return Ok(());
        }
        return Err(Error::from_errno(errno()));
    }
    if value.len() > MAX_VALUE {
        return Err(Error::TooBig);
    }
    // SAFETY: the strings are NUL-terminated and `value` is readable for its length.
    let r = unsafe {
        libc::lsetxattr(
            p.as_ptr(),
            n.as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
        )
    };
    if r == 0 {
        Ok(())
    } else {
        Err(Error::from_errno(errno()))
    }
}

/// Whether the file system holding `path` keeps `user.*` attributes: `Ok`
/// when asking works (the answer "no such attribute" counts), the reason
/// when it does not. Changes nothing.
pub fn probe(path: &Path) -> Result<(), Error> {
    get(path, TAGS).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A temporary folder, or `None` (the test is skipped) when its file
    /// system keeps no user attributes.
    fn scratch(tag: &str) -> Option<std::path::PathBuf> {
        let dir = std::env::temp_dir().join(format!("telamon-xattr-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        if probe(&dir).is_err() {
            eprintln!("skipped: {} keeps no user attributes", dir.display());
            return None;
        }
        Some(dir)
    }

    #[test]
    fn set_get_and_remove() {
        let Some(dir) = scratch("sgr") else { return };
        let f = dir.join("a.txt");
        std::fs::write(&f, b"x").unwrap();
        assert_eq!(get(&f, TAGS), Ok(None));
        set(&f, TAGS, b"Red,Work").unwrap();
        assert_eq!(get(&f, TAGS), Ok(Some(b"Red,Work".to_vec())));
        set(&f, TAGS, b"").unwrap();
        assert_eq!(get(&f, TAGS), Ok(None));
        // Removing what is not there is fine.
        set(&f, TAGS, b"").unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn links_and_missing_files() {
        let Some(dir) = scratch("link") else { return };
        let f = dir.join("a.txt");
        std::fs::write(&f, b"x").unwrap();
        let l = dir.join("l");
        std::os::unix::fs::symlink(&f, &l).unwrap();
        assert_eq!(set(&l, TAGS, b"Red"), Err(Error::Link));
        assert_eq!(get(&dir.join("nope"), TAGS), Err(Error::Gone));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn long_values_are_refused_not_cut() {
        let Some(dir) = scratch("long") else { return };
        let f = dir.join("a.txt");
        std::fs::write(&f, b"x").unwrap();
        assert_eq!(
            set(&f, TAGS, &vec![b'a'; MAX_VALUE + 1]),
            Err(Error::TooBig)
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn error_text_is_plain() {
        assert_eq!(Error::Unsupported.text(), "This location can't keep tags.");
        assert_eq!(Error::from_errno(libc::EOPNOTSUPP), Error::Unsupported);
        assert_eq!(Error::from_errno(libc::EACCES), Error::Denied);
    }
}
