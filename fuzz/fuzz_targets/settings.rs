#![no_main]
//! Text read back from the settings files and the index's own files: all of
//! it can be edited by anything running as the user, and by a copy of the
//! settings from elsewhere.
use atlas_explorer_core::{actions, home, menuprefs, saved, views};
use libfuzzer_sys::fuzz_target;
use std::path::Path;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data).into_owned();
    // lists that bound themselves and write back what they read
    let a = actions::ActionList::parse(&text);
    assert!(a.items().len() <= actions::MAX_ACTIONS);
    let again = actions::ActionList::parse(&a.to_text());
    assert_eq!(again.items().len(), a.items().len());
    let s = saved::SavedList::parse(&text);
    assert!(s.items().len() <= saved::MAX_SAVED);
    assert_eq!(saved::SavedList::parse(&s.to_text()).items().len(), s.items().len());
    let h = menuprefs::Hidden::parse(&text);
    let _ = h.to_text();
    let v = views::FolderViews::parse(&text);
    assert!(v.len() <= views::MAX_FOLDERS);
    let f = home::Frequent::parse(&text);
    assert!(f.len() <= home::MAX_FOLDERS);
    // argument templates and what they make of a file name
    let _ = actions::split_words(&text);
    if actions::parse_args(&text).is_ok() {
        let item = actions::Item {
            path: Some(text.clone()),
            url: format!("file://{text}"),
        };
        if let Ok(runs) = actions::expand(&text, std::slice::from_ref(&item)) {
            assert!(runs.len() <= actions::MAX_RUNS);
        }
    }
    let _ = actions::resolve_program(&text, "/usr/bin:/bin");
    // the index service's files
    let _ = atlas_file_index::config::Config::parse(&text, Path::new("/home/u"));
    let _ = atlas_file_index::config::parse_hidden_file(data);
    let _ = atlas_file_index::recent::parse(&text);
    let _ = atlas_file_index::uri::uri_to_path(&text);
    let _ = atlas_file_index::tags::clean(data);
    // search words and patterns
    let m = atlas_file_index::query::NameMatcher::new(&text);
    let _ = m.class(data);
    let _ = atlas_explorer_core::pattern::Pattern::new(&text).map(|p| p.is_match(data));
});
