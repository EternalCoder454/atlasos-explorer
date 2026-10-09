#![no_main]
//! A `.trashinfo` file, its date, and the mount table: all text a drive or
//! the kernel's neighbours can shape.
use atlas_explorer_core::trash;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(info) = trash::parse_info(data) {
        assert!(!info.original.is_empty() && !info.original.contains(&0));
    }
    if let Ok(s) = std::str::from_utf8(data) {
        if let Some(t) = trash::parse_date(s) {
            assert!(t >= 0);
        }
        for m in trash::parse_mountinfo(s) {
            assert!(m.point.is_absolute());
        }
    }
    // the restore decision never panics, and a home Trash is always believed
    assert!(trash::restore_allowed(true, data, b"/home/u"));
    let _ = trash::restore_allowed(false, data, b"/home/u");
});
