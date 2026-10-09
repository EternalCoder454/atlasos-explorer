#![no_main]
//! The records the window sends before KIO copies out of an archive, and the
//! rules for one entry's path and link.
use atlas_explorer_core::archive;
use libfuzzer_sys::fuzz_target;

/// Whether `path` (read with either separator) climbs out of the folder it is
/// taken out into, by a reference model that shares no code with the guard.
fn escapes(path: &[u8]) -> bool {
    if path.contains(&0) || matches!(path.first(), Some(b'/' | b'\\')) {
        return true;
    }
    if path.len() >= 2 && path[0].is_ascii_alphabetic() && path[1] == b':' {
        let rest = &path[2..];
        if rest.is_empty() || matches!(rest[0], b'/' | b'\\') {
            return true;
        }
    }
    path.split(|&b| b == b'/' || b == b'\\').any(|c| c == b"..")
}

fuzz_target!(|data: &[u8]| {
    // Any bytes: never a panic, and anything that does not parse is refused
    // rather than skipped.
    let entries = archive::parse_records(data);
    let report = archive::check_entries(entries.iter().copied());
    assert_eq!(report.checked, entries.len());
    let _ = archive::refusal_text(&report, true);
    for e in &entries {
        let ok = archive::check_path(e.path).is_ok();
        if ok {
            assert!(!escapes(e.path), "accepted a path that escapes");
            assert!(e.path.len() <= archive::MAX_PATH_BYTES);
        }
        if let Some(l) = e.link
            && archive::check_link(l).is_ok()
        {
            assert!(!escapes(l) || l.first().is_some_and(|b| b.is_ascii_alphabetic()) && l.get(1) == Some(&b':'));
        }
    }
    // The zip trailer readers take the same bytes.
    let _ = archive::zip_end(data, data.len() as u64);
    let _ = archive::zip_directory_encrypted(data);
    let _ = archive::zip64_directory(data, 0);
    let _ = archive::looks_like_archive(data);
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = archive::locate(s);
        let _ = archive::parent(s);
        let _ = archive::archive_name(s);
        let _ = archive::folder_name_for(s);
    }
});
