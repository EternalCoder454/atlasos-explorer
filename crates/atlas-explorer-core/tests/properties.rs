//! Property tests for the parsers and checks that read untrusted input: what
//! they accept holds the invariants the rest of Files relies on, and nothing
//! panics. The same invariants run under cargo-fuzz (`fuzz/`), for longer, on
//! bytes chosen by coverage; these run on every `cargo test`.

use atlas_explorer_core::batch::{self, CaseMode, Edge, Item, Op};
use atlas_explorer_core::{address, archive, display, launch, names, servers, trash};
use proptest::prelude::*;
use std::path::Path;

/// Paths made of the pieces that matter: separators of both kinds, dots,
/// drive letters, a NUL, controls and some ordinary names.
fn pathish() -> impl Strategy<Value = Vec<u8>> {
    proptest::collection::vec(
        prop_oneof![
            Just(b"/".to_vec()),
            Just(b"\\".to_vec()),
            Just(b".".to_vec()),
            Just(b"..".to_vec()),
            Just(b"a".to_vec()),
            Just(b"C:".to_vec()),
            Just(b"\0".to_vec()),
            Just(b"\n".to_vec()),
            Just("é".as_bytes().to_vec()),
            Just(b"name".to_vec()),
        ],
        0..14,
    )
    .prop_map(|parts| parts.concat())
}

/// A reference for "this path climbs out of the folder it is taken out into",
/// sharing no code with the guard.
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

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(2000),
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn an_archive_path_is_taken_out_exactly_when_it_stays_below_the_folder(path in pathish()) {
        let ok = archive::check_path(&path).is_ok();
        // (length and depth limits only ever make it stricter)
        if ok {
            prop_assert!(!escapes(&path));
        }
        if !escapes(&path) && path.len() <= archive::MAX_PATH_BYTES {
            let depth = path.split(|&b| b == b'/' || b == b'\\').filter(|c| !c.is_empty() && *c != b".").count();
            prop_assert_eq!(ok, depth <= archive::MAX_COMPONENTS);
        }
    }

    #[test]
    fn a_link_target_is_never_absolute_or_climbing(target in pathish()) {
        if archive::check_link(&target).is_ok() {
            prop_assert!(!target.contains(&0));
            prop_assert!(!matches!(target.first(), Some(b'/' | b'\\')));
            prop_assert!(!target.split(|&b| b == b'/' || b == b'\\').any(|c| c == b".."));
        }
    }

    #[test]
    fn records_from_the_window_never_panic_and_bad_ones_are_refused(buf in proptest::collection::vec(any::<u8>(), 0..200)) {
        let entries = archive::parse_records(&buf);
        let report = archive::check_entries(entries.iter().copied());
        prop_assert_eq!(report.checked, entries.len());
        // a record that does not parse is a problem, never skipped
        let well_formed = entries.iter().all(|e| !e.path.contains(&0) && e.link.is_none_or(|l| !l.contains(&0)));
        if !well_formed {
            prop_assert!(!report.ok());
        }
        let _ = archive::zip_end(&buf, buf.len() as u64);
        let _ = archive::zip_directory_encrypted(&buf);
        let _ = archive::zip64_directory(&buf, 0);
    }

    #[test]
    fn trashinfo_text_never_panics_and_gives_a_usable_path(buf in proptest::collection::vec(any::<u8>(), 0..300)) {
        if let Ok(info) = trash::parse_info(&buf) {
            prop_assert!(!info.original.is_empty() && !info.original.contains(&0));
        }
    }

    #[test]
    fn a_trashinfo_round_trips_any_path_without_nul(path in proptest::collection::vec(1u8..=255, 1..60)) {
        // percent-encode everything, as a writer may
        let enc: String = path.iter().map(|b| format!("%{b:02X}")).collect();
        let text = format!("[Trash Info]\nPath={enc}\nDeletionDate=2026-10-07T10:00:00\n");
        let info = trash::parse_info(text.as_bytes()).expect("a well formed info");
        prop_assert_eq!(info.original, path);
    }

    #[test]
    fn dates_are_real_dates_or_nothing(s in "[0-9T:\\- ]{0,24}") {
        if let Some(t) = trash::parse_date(&s) {
            prop_assert!(t >= 0);
        }
    }

    #[test]
    fn a_hostile_drives_trash_never_restores_into_hidden_home_places(
        rest in "[a-zA-Z.]{0,6}(/[a-zA-Z.]{0,6}){0,3}"
    ) {
        let target = format!("/home/u/{rest}");
        let allowed = trash::restore_allowed(false, target.as_bytes(), b"/home/u");
        if rest.starts_with('.') || rest.split('/').any(|c| c == "..") {
            prop_assert!(!allowed, "{target}");
        }
        prop_assert!(trash::restore_allowed(true, target.as_bytes(), b"/home/u"));
    }

    #[test]
    fn launch_arguments_never_give_hidden_characters_or_passwords(
        args in proptest::collection::vec("[ -~\u{202e}\u{0}\u{a}%]{0,40}", 0..6),
        bus in any::<bool>(),
    ) {
        let mut a = args;
        if bus {
            a.insert(0, "--bus".into());
        }
        let l = launch::parse(&a, Path::new("/home/u"));
        for loc in &l.locations {
            prop_assert!(!loc.chars().any(launch::is_hidden_char));
        }
    }

    #[test]
    fn typed_addresses_never_keep_a_password_or_a_dash_host(
        user in "[a-z:%2D-]{0,8}", pw in "[a-z0-9]{0,6}", host in "[a-z0-9.%-]{0,12}", rest in "[a-z/]{0,8}"
    ) {
        for scheme in ["sftp", "smb", "ftp", "fish", "webdavs", "nfs"] {
            let text = format!("{scheme}://{user}:{pw}@{host}/{rest}");
            if let Ok(url) = address::parse(&text, "file:///home/u", Path::new("/home/u")) {
                let auth = url.split_once("://").map(|(_, r)| r.split('/').next().unwrap_or("")).unwrap_or("");
                let userinfo = auth.rsplit_once('@').map(|(u, _)| u).unwrap_or("");
                prop_assert!(!userinfo.contains(':'), "{url}");
                prop_assert!(!userinfo.starts_with('-'), "{url}");
                let h = auth.rsplit_once('@').map_or(auth, |(_, h)| h);
                prop_assert!(!h.starts_with('-'), "{url}");
            }
        }
    }

    #[test]
    fn connect_to_server_builds_only_addresses_it_reads_back(
        server in "\\PC{0,30}", folder in "\\PC{0,30}", user in "\\PC{0,12}", p in 0u32..6
    ) {
        let proto = servers::Protocol::from_code(p).unwrap();
        if let Ok(url) = servers::build(proto, &server, &folder, &user) {
            prop_assert!(!url.chars().any(launch::is_hidden_char));
            let parts = servers::parse_url(&url).expect("an address it built is one it reads");
            let again = servers::build(parts.protocol, &parts.server, &parts.folder, &parts.user).unwrap();
            prop_assert_eq!(again, url);
        }
    }

    #[test]
    fn shown_names_hold_no_controls_or_direction_characters(raw in proptest::collection::vec(any::<u8>(), 0..400)) {
        let s = display::display_name(&raw[..]);
        prop_assert!(s.chars().count() <= display::MAX_DISPLAY_CHARS + 1);
        prop_assert!(!s.chars().any(|c| c.is_control()));
        prop_assert!(!s.chars().any(launch::is_hidden_char));
    }

    #[test]
    fn a_name_that_passes_the_check_is_a_single_plain_name(name in "\\PC{0,300}") {
        if names::validate(&name).is_ok() {
            prop_assert!(!name.is_empty() && !name.contains(['/', '\0']));
            prop_assert!(name != "." && name != "..");
            prop_assert!(name.len() <= names::MAX_NAME_BYTES);
        }
    }

    #[test]
    fn batch_rename_never_makes_a_path_out_of_a_name(
        files in proptest::collection::vec(("\\PC{1,40}", any::<bool>()), 1..8),
        find in "\\PC{0,8}", with in "\\PC{0,20}", op in 0u8..4, flag in any::<bool>(), n in any::<u64>(),
    ) {
        let items: Vec<Item> = files.iter().map(|(n, d)| Item { name: n, is_dir: *d }).collect();
        let op = match op {
            0 => Op::Replace { find: &find, with: &with, match_case: flag, regex: n % 2 == 0 },
            1 => Op::Number { start: n, step: n >> 3, padding: (n % 14) as usize, at: if flag { Edge::End } else { Edge::Start }, separator: &with },
            2 => Op::Case(CaseMode::Title),
            _ => Op::AddText { text: &with, at: if flag { Edge::End } else { Edge::Start } },
        };
        let plan = batch::plan(&items, &op, |_| false);
        for row in &plan.rows {
            // (an item that does not change is left alone, whatever it is called)
            if !row.check.blocks() && row.check != batch::Check::Unchanged {
                prop_assert!(!row.new.is_empty() && row.new != "." && row.new != "..");
                prop_assert!(!row.new.contains(['/', '\0']));
                prop_assert!(row.new.len() <= names::MAX_NAME_BYTES);
            }
        }
    }
}
