//! What the context menus offer: which entries a menu has for the items under
//! the pointer (or for the folder's background) and which are enabled, the
//! templates of "New", and the `.hidden` file "Hide" writes. The window draws
//! the entries; this decides them from facts, so it is tested without a
//! display. The menu is decided once, before it is shown, and not again while
//! it is open (the facts are a snapshot).

use crate::names::{self, split_ext};

// Facts about the selection and the folder, as bits.

/// The folder shown can be written to (and is not a list of search results).
pub const F_WRITABLE: u32 = 1;
/// The rows are search results.
pub const F_SEARCHING: u32 = 1 << 1;
/// The folder is "Recent".
pub const F_RECENT: u32 = 1 << 2;
/// The folder is the Trash.
pub const F_IN_TRASH: u32 = 1 << 3;
/// Every item is on this computer.
pub const F_LOCAL: u32 = 1 << 4;
/// The clipboard holds something that can be pasted.
pub const F_CAN_PASTE: u32 = 1 << 5;
/// Telamon Archive is installed.
pub const F_ARCHIVE: u32 = 1 << 6;
/// A single selected folder may be pinned to the sidebar.
pub const F_PINNABLE: u32 = 1 << 7;
/// A single selected folder can be written to.
pub const F_FOLDER_WRITABLE: u32 = 1 << 8;
/// KFileItemActions offers at least one "Open With" entry.
pub const F_OPEN_WITH: u32 = 1 << 9;
/// No selected item is hidden (a name with a dot first, or listed in `.hidden`).
pub const F_HIDEABLE: u32 = 1 << 10;
/// Every selected item is hidden by a `.hidden` file, not by a dot.
pub const F_UNHIDEABLE: u32 = 1 << 11;
/// A terminal can be opened for the item (a folder on this computer).
pub const F_TERMINAL: u32 = 1 << 12;
/// There is something to undo.
pub const F_CAN_UNDO: u32 = 1 << 13;
/// There is something to redo.
pub const F_CAN_REDO: u32 = 1 << 14;
/// Every selected item is an archive Telamon Archive can extract.
pub const F_ARCHIVE_ITEMS: u32 = 1 << 15;
/// The items are at the top of the Trash (only those can be restored).
pub const F_TRASH_TOP: u32 = 1 << 16;

/// One entry of a menu. `key` names it for the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmd {
    // The menu of items.
    Open,
    Restore,
    OpenWith,
    Cut,
    Copy,
    Paste,
    Rename,
    Trash,
    ExtractHere,
    ExtractTo,
    CompressZip,
    Compress,
    Properties,
    MoreActions,
    // In "More Actions".
    OpenInNewTab,
    OpenFileLocation,
    OpenTerminal,
    PinToSidebar,
    CopyPath,
    Hide,
    Unhide,
    DeleteForGood,
    // The menu of the background (Paste, Properties and Open Terminal Here too).
    New,
    Undo,
    Redo,
    Sort,
    View,
    PinFolder,
}

impl Cmd {
    pub fn key(self) -> &'static str {
        match self {
            Cmd::Open => "open",
            Cmd::Restore => "restore",
            Cmd::OpenWith => "openWith",
            Cmd::Cut => "cut",
            Cmd::Copy => "copy",
            Cmd::Paste => "paste",
            Cmd::Rename => "rename",
            Cmd::Trash => "trash",
            Cmd::ExtractHere => "extractHere",
            Cmd::ExtractTo => "extractTo",
            Cmd::CompressZip => "compressZip",
            Cmd::Compress => "compress",
            Cmd::Properties => "properties",
            Cmd::MoreActions => "moreActions",
            Cmd::OpenInNewTab => "openInNewTab",
            Cmd::OpenFileLocation => "openFileLocation",
            Cmd::OpenTerminal => "openTerminal",
            Cmd::PinToSidebar => "pinToSidebar",
            Cmd::CopyPath => "copyPath",
            Cmd::Hide => "hide",
            Cmd::Unhide => "unhide",
            Cmd::DeleteForGood => "deleteForGood",
            Cmd::New => "new",
            Cmd::Undo => "undo",
            Cmd::Redo => "redo",
            Cmd::Sort => "sort",
            Cmd::View => "view",
            Cmd::PinFolder => "pinFolder",
        }
    }
}

/// Where "Paste" in the menu of items puts the clipboard: into the one folder
/// selected, else into the folder shown.
pub fn paste_into_selected_folder(count: usize, folders: usize) -> bool {
    count == 1 && folders == 1
}

/// The entries of the menu of `count` items, `folders` of them folders, and
/// whether each is enabled. Entries that don't apply are left out. The order
/// is not the menu's (the window fixes that); each entry appears once.
pub fn item_menu(count: usize, folders: usize, flags: u32) -> Vec<(Cmd, bool)> {
    if count == 0 {
        return Vec::new();
    }
    let has = |f: u32| flags & f != 0;
    let writable = has(F_WRITABLE);
    let single = count == 1;
    // "Open With" is always in the menu; it is disabled when no application
    // is known for the items.
    let mut out = vec![(Cmd::Open, true), (Cmd::OpenWith, has(F_OPEN_WITH))];
    if has(F_IN_TRASH) {
        // Only what was trashed itself goes back; what is inside a trashed
        // folder goes back with it.
        out.push((Cmd::Restore, has(F_TRASH_TOP)));
    }
    out.push((Cmd::Cut, writable));
    out.push((Cmd::Copy, true));
    let paste_target_ok = if paste_into_selected_folder(count, folders) {
        has(F_FOLDER_WRITABLE)
    } else {
        // Results are from many folders: there is no "here" to paste into.
        writable && !has(F_SEARCHING)
    };
    out.push((Cmd::Paste, has(F_CAN_PASTE) && paste_target_ok));
    // Items in the Trash keep the names they were deleted with. Several items
    // are renamed together (Batch Rename), which needs them in one folder:
    // search results are from many.
    let renamable = single || (count > 1 && !has(F_SEARCHING));
    out.push((Cmd::Rename, renamable && writable && !has(F_IN_TRASH)));
    out.push((Cmd::Trash, writable && !has(F_IN_TRASH)));
    if has(F_ARCHIVE) {
        // Telamon Archive reads and writes files on this computer only.
        let here = has(F_LOCAL) && !has(F_IN_TRASH);
        if has(F_ARCHIVE_ITEMS) {
            out.push((Cmd::ExtractHere, here));
            out.push((Cmd::ExtractTo, here));
        }
        out.push((Cmd::CompressZip, here));
        out.push((Cmd::Compress, here));
    }
    out.push((Cmd::Properties, true));
    out.push((Cmd::MoreActions, true));
    if folders > 0 {
        out.push((Cmd::OpenInNewTab, true));
    }
    if has(F_SEARCHING) || has(F_RECENT) {
        out.push((Cmd::OpenFileLocation, true));
    }
    out.push((Cmd::OpenTerminal, has(F_TERMINAL)));
    if single && folders == 1 {
        out.push((Cmd::PinToSidebar, has(F_PINNABLE)));
    }
    out.push((Cmd::CopyPath, true));
    let hide_here = writable && has(F_LOCAL) && !has(F_SEARCHING) && !has(F_RECENT);
    if has(F_UNHIDEABLE) {
        out.push((Cmd::Unhide, hide_here));
    } else if has(F_HIDEABLE) {
        out.push((Cmd::Hide, hide_here));
    }
    out.push((Cmd::DeleteForGood, writable || has(F_IN_TRASH)));
    out
}

/// The entries of the menu of the folder's background.
pub fn background_menu(flags: u32) -> Vec<(Cmd, bool)> {
    let has = |f: u32| flags & f != 0;
    let writable = has(F_WRITABLE);
    let searching = has(F_SEARCHING);
    let mut out = vec![
        (Cmd::New, writable),
        (Cmd::Paste, writable && has(F_CAN_PASTE)),
        (Cmd::Undo, has(F_CAN_UNDO)),
        (Cmd::Redo, has(F_CAN_REDO)),
        (Cmd::Sort, true),
        (Cmd::View, true),
        (Cmd::OpenTerminal, has(F_TERMINAL)),
    ];
    if !searching {
        out.push((Cmd::PinFolder, has(F_PINNABLE)));
    }
    out.push((Cmd::Properties, !searching));
    out
}

/// The entries as lines for the C++ side: the key, then `+` for enabled or
/// `-` for disabled.
pub fn state_text(entries: &[(Cmd, bool)]) -> String {
    let mut s = String::new();
    for (c, on) in entries {
        s.push_str(c.key());
        s.push(if *on { '+' } else { '-' });
        s.push('\n');
    }
    s
}

// ---- Telamon Archive ----

/// The desktop file IDs under which Telamon Archive is installed: the new
/// name, then the old one (kept for one release, see Archive's DESIGN).
pub const ARCHIVE_DESKTOP_IDS: [&str; 2] = [
    "net.eterneon.telamon.archive.desktop",
    "net.eterneon.atlas.archive.desktop",
];

// ---- New: Text File and the templates ----

/// The most templates "New" lists.
pub const MAX_TEMPLATES: usize = 40;

/// Whether a file in the Templates folder is offered: a regular name, not a
/// dot file, a backup (`~`) or a leftover of a download.
pub fn template_ok(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    !file_name.is_empty()
        && !file_name.starts_with('.')
        && !file_name.ends_with('~')
        && !lower.ends_with(".part")
        && !lower.ends_with(".crdownload")
        && !lower.ends_with(".tmp")
        && !file_name.contains(['/', '\0', '\n'])
}

/// What a template is called in the menu: its name without the extension.
pub fn template_label(file_name: &str) -> String {
    let (stem, _) = split_ext(file_name);
    if stem.is_empty() {
        file_name.to_string()
    } else {
        stem.to_string()
    }
}

/// The name proposed for a new file made from a template, or for a new text
/// file: "New Spreadsheet.ods". A name that already starts with "New" stays.
pub fn new_file_name(template_file_name: &str) -> String {
    let lower = template_file_name.to_lowercase();
    if lower.starts_with("new ") || lower == "new" {
        template_file_name.to_string()
    } else {
        format!("New {template_file_name}")
    }
}

/// `wanted` if `exists` says it is free, else "wanted (2)", "wanted (3)" and so on.
pub fn free_name(wanted: &str, exists: impl Fn(&str) -> bool) -> String {
    if exists(wanted) {
        names::keep_both_name(wanted, exists)
    } else {
        wanted.to_string()
    }
}

/// The name proposed for a new text file.
pub const NEW_TEXT_FILE: &str = "New Text File.txt";
/// The name proposed for a new folder.
pub const NEW_FOLDER: &str = "New Folder";

/// The file names to list, in order: filtered, ordered by label (case
/// ignored, numbers in order), at most `MAX_TEMPLATES`. Two files with the
/// same label both stay (their extensions tell them apart in the prompt).
pub fn pick_templates(file_names: &[String]) -> Vec<String> {
    let mut v: Vec<(Vec<u8>, &String)> = file_names
        .iter()
        .filter(|n| template_ok(n))
        .map(|n| (crate::sort::name_key(template_label(n).as_bytes()), n))
        .collect();
    v.sort();
    v.into_iter()
        .take(MAX_TEMPLATES)
        .map(|(_, n)| n.clone())
        .collect()
}

// ---- Hide: the `.hidden` file ----

/// The most bytes of a `.hidden` file that are read; a bigger one is left alone.
pub const HIDDEN_FILE_MAX: usize = 1 << 20;

/// Why a name can't go into a `.hidden` file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HideError {
    /// A name with a line break would be two names.
    LineBreak,
    /// Not a name (empty, with a slash or NUL, "." or "..").
    NotAName,
    /// A file this big is not a list of names; it is not touched.
    TooBig,
}

impl HideError {
    pub fn describe(self) -> &'static str {
        match self {
            HideError::LineBreak => "A name with a line break can't be hidden this way.",
            HideError::NotAName => "That is not a name that can be hidden.",
            HideError::TooBig => "The .hidden file in this folder is too big to change.",
        }
    }
}

fn hide_name_ok(name: &str) -> Result<(), HideError> {
    if name.contains(['\n', '\r']) {
        return Err(HideError::LineBreak);
    }
    if name.is_empty() || name.contains(['/', '\0']) || name == "." || name == ".." {
        return Err(HideError::NotAName);
    }
    Ok(())
}

/// The names in a `.hidden` file's text.
pub fn hidden_names(content: &str) -> Vec<&str> {
    content
        .lines()
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .filter(|l| !l.is_empty())
        .collect()
}

/// The `.hidden` text with `names` added, or `None` when all were listed
/// already. Other lines are kept as they are.
pub fn hidden_add(content: &str, names: &[String]) -> Result<Option<String>, HideError> {
    if content.len() > HIDDEN_FILE_MAX {
        return Err(HideError::TooBig);
    }
    let listed = hidden_names(content);
    let mut out = content.to_string();
    let mut changed = false;
    for n in names {
        hide_name_ok(n)?;
        if listed.contains(&n.as_str()) || out.lines().any(|l| l == n) {
            continue;
        }
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(n);
        out.push('\n');
        changed = true;
    }
    Ok(changed.then_some(out))
}

/// The `.hidden` text without `names`, or `None` when none was listed.
pub fn hidden_remove(content: &str, names: &[String]) -> Result<Option<String>, HideError> {
    if content.len() > HIDDEN_FILE_MAX {
        return Err(HideError::TooBig);
    }
    let mut out = String::with_capacity(content.len());
    let mut changed = false;
    for line in content.split_inclusive('\n') {
        let bare = line.trim_end_matches('\n');
        let bare = bare.strip_suffix('\r').unwrap_or(bare);
        if names.iter().any(|n| n == bare) {
            changed = true;
        } else {
            out.push_str(line);
        }
    }
    Ok(changed.then_some(out))
}

/// Whether `name` is a good name for the user's input (the same check as
/// Rename). Re-exported so the prompts and the menu share one rule.
pub fn name_problem(name: &str) -> Result<Vec<names::Warning>, names::Invalid> {
    names::validate(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(v: &[(Cmd, bool)]) -> Vec<&'static str> {
        v.iter().map(|(c, _)| c.key()).collect()
    }
    fn on(v: &[(Cmd, bool)], c: Cmd) -> Option<bool> {
        v.iter().find(|(k, _)| *k == c).map(|(_, e)| *e)
    }

    const BASE: u32 = F_WRITABLE | F_LOCAL | F_TERMINAL | F_HIDEABLE;

    #[test]
    fn a_file_gets_the_fixed_set() {
        let m = item_menu(1, 0, BASE);
        for c in [
            Cmd::Open,
            Cmd::Cut,
            Cmd::Copy,
            Cmd::Paste,
            Cmd::Rename,
            Cmd::Trash,
            Cmd::OpenWith,
            Cmd::Properties,
            Cmd::MoreActions,
            Cmd::OpenTerminal,
            Cmd::CopyPath,
            Cmd::Hide,
            Cmd::DeleteForGood,
        ] {
            assert!(on(&m, c).is_some(), "{:?} missing: {:?}", c, keys(&m));
        }
        // Not for a file: tabs, pins; nothing about Open With or Compress
        // unless asked.
        for c in [
            Cmd::OpenInNewTab,
            Cmd::PinToSidebar,
            Cmd::Compress,
            Cmd::OpenFileLocation,
            Cmd::Unhide,
        ] {
            assert!(on(&m, c).is_none(), "{:?} should be absent", c);
        }
    }

    #[test]
    fn every_entry_once() {
        let all = F_WRITABLE
            | F_SEARCHING
            | F_RECENT
            | F_LOCAL
            | F_CAN_PASTE
            | F_ARCHIVE
            | F_ARCHIVE_ITEMS
            | F_PINNABLE
            | F_FOLDER_WRITABLE
            | F_OPEN_WITH
            | F_HIDEABLE
            | F_TERMINAL;
        for (count, folders) in [(1, 0), (1, 1), (3, 0), (3, 2), (2, 2)] {
            let m = item_menu(count, folders, all);
            let mut k = keys(&m);
            let n = k.len();
            k.sort();
            k.dedup();
            assert_eq!(k.len(), n, "a key twice for {count} {folders}");
        }
    }

    #[test]
    fn nothing_selected_has_no_item_menu() {
        assert!(item_menu(0, 0, !0).is_empty());
    }

    #[test]
    fn read_only_folder_disables_changes_but_not_copy() {
        let m = item_menu(1, 0, F_LOCAL | F_CAN_PASTE | F_TERMINAL | F_HIDEABLE);
        assert_eq!(on(&m, Cmd::Cut), Some(false));
        assert_eq!(on(&m, Cmd::Rename), Some(false));
        assert_eq!(on(&m, Cmd::Trash), Some(false));
        assert_eq!(on(&m, Cmd::Paste), Some(false));
        assert_eq!(on(&m, Cmd::Hide), Some(false));
        assert_eq!(on(&m, Cmd::DeleteForGood), Some(false));
        assert_eq!(on(&m, Cmd::Copy), Some(true));
        assert_eq!(on(&m, Cmd::Open), Some(true));
        assert_eq!(on(&m, Cmd::Properties), Some(true));
        assert_eq!(on(&m, Cmd::CopyPath), Some(true));
    }

    #[test]
    fn rename_takes_one_item_or_a_set_in_one_folder() {
        assert_eq!(on(&item_menu(1, 0, BASE), Cmd::Rename), Some(true));
        // Several items open Batch Rename.
        assert_eq!(on(&item_menu(2, 0, BASE), Cmd::Rename), Some(true));
        assert_eq!(on(&item_menu(2, 1, BASE), Cmd::Rename), Some(true));
        // Results come from many folders: one at a time.
        assert_eq!(
            on(&item_menu(1, 0, BASE | F_SEARCHING), Cmd::Rename),
            Some(true)
        );
        assert_eq!(
            on(&item_menu(2, 0, BASE | F_SEARCHING), Cmd::Rename),
            Some(false)
        );
        assert_eq!(
            on(&item_menu(2, 0, BASE | F_IN_TRASH), Cmd::Rename),
            Some(false)
        );
        assert_eq!(
            on(&item_menu(2, 0, BASE & !F_WRITABLE), Cmd::Rename),
            Some(false)
        );
    }

    #[test]
    fn paste_goes_into_the_one_folder_selected() {
        // A folder you can't write to: no paste, even if the folder shown is writable.
        let m = item_menu(1, 1, BASE | F_CAN_PASTE);
        assert_eq!(on(&m, Cmd::Paste), Some(false));
        let m = item_menu(1, 1, BASE | F_CAN_PASTE | F_FOLDER_WRITABLE);
        assert_eq!(on(&m, Cmd::Paste), Some(true));
        // Anything else pastes into the folder shown.
        assert_eq!(
            on(&item_menu(1, 0, BASE | F_CAN_PASTE), Cmd::Paste),
            Some(true)
        );
        assert_eq!(
            on(&item_menu(2, 1, BASE | F_CAN_PASTE), Cmd::Paste),
            Some(true)
        );
        // Nothing on the clipboard.
        assert_eq!(on(&item_menu(1, 0, BASE), Cmd::Paste), Some(false));
        assert!(paste_into_selected_folder(1, 1));
        assert!(!paste_into_selected_folder(2, 2));
        assert!(!paste_into_selected_folder(1, 0));
    }

    #[test]
    fn nothing_is_pasted_into_search_results() {
        let flags = BASE | F_CAN_PASTE | F_SEARCHING;
        assert_eq!(on(&item_menu(1, 0, flags), Cmd::Paste), Some(false));
        assert_eq!(on(&item_menu(3, 0, flags), Cmd::Paste), Some(false));
        // A folder among the results still takes it.
        assert_eq!(
            on(&item_menu(1, 1, flags | F_FOLDER_WRITABLE), Cmd::Paste),
            Some(true)
        );
    }

    #[test]
    fn folders_get_tab_and_pin() {
        let m = item_menu(1, 1, BASE | F_PINNABLE);
        assert_eq!(on(&m, Cmd::OpenInNewTab), Some(true));
        assert_eq!(on(&m, Cmd::PinToSidebar), Some(true));
        // A folder where pinning is not possible (the Trash): listed, disabled.
        assert_eq!(on(&item_menu(1, 1, BASE), Cmd::PinToSidebar), Some(false));
        // Several folders: tabs, but no single pin.
        let m = item_menu(2, 2, BASE | F_PINNABLE);
        assert_eq!(on(&m, Cmd::OpenInNewTab), Some(true));
        assert_eq!(on(&m, Cmd::PinToSidebar), None);
        // A folder among files: a tab for it, no pin.
        let m = item_menu(2, 1, BASE | F_PINNABLE);
        assert_eq!(on(&m, Cmd::OpenInNewTab), Some(true));
        assert_eq!(on(&m, Cmd::PinToSidebar), None);
    }

    #[test]
    fn compress_only_with_archive_and_local_files() {
        assert_eq!(on(&item_menu(1, 0, BASE), Cmd::Compress), None);
        assert_eq!(
            on(&item_menu(1, 0, BASE | F_ARCHIVE), Cmd::Compress),
            Some(true)
        );
        assert_eq!(
            on(
                &item_menu(1, 0, (BASE & !F_LOCAL) | F_ARCHIVE),
                Cmd::Compress
            ),
            Some(false)
        );
        assert_eq!(
            on(
                &item_menu(1, 0, BASE | F_ARCHIVE | F_IN_TRASH),
                Cmd::Compress
            ),
            Some(false)
        );
    }

    #[test]
    fn extract_only_for_archives_with_archive_installed() {
        let ext = |flags| {
            let m = item_menu(1, 0, flags);
            (
                on(&m, Cmd::ExtractHere),
                on(&m, Cmd::ExtractTo),
                on(&m, Cmd::CompressZip),
            )
        };
        // Archive missing: no archive entry at all.
        assert_eq!(ext(BASE | F_ARCHIVE_ITEMS), (None, None, None));
        // Archive there, a plain file: Compress only.
        assert_eq!(ext(BASE | F_ARCHIVE), (None, None, Some(true)));
        // An archive: Extract Here and Extract To as well.
        assert_eq!(
            ext(BASE | F_ARCHIVE | F_ARCHIVE_ITEMS),
            (Some(true), Some(true), Some(true))
        );
        // Not on this computer, or in the Trash: shown off (Archive takes file:// only).
        assert_eq!(
            ext((BASE & !F_LOCAL) | F_ARCHIVE | F_ARCHIVE_ITEMS),
            (Some(false), Some(false), Some(false))
        );
        assert_eq!(
            ext(BASE | F_ARCHIVE | F_ARCHIVE_ITEMS | F_IN_TRASH),
            (Some(false), Some(false), Some(false))
        );
        // Extracting needs no write access to the folder the archive is in
        // (Archive says if it can't write), nor does compressing.
        assert_eq!(
            ext((BASE & !F_WRITABLE) | F_ARCHIVE | F_ARCHIVE_ITEMS),
            (Some(true), Some(true), Some(true))
        );
    }

    #[test]
    fn open_with_is_there_but_off_without_apps() {
        assert_eq!(on(&item_menu(1, 0, BASE), Cmd::OpenWith), Some(false));
        assert_eq!(
            on(&item_menu(1, 0, BASE | F_OPEN_WITH), Cmd::OpenWith),
            Some(true)
        );
    }

    #[test]
    fn open_file_location_for_results_and_recent() {
        assert_eq!(on(&item_menu(1, 0, BASE), Cmd::OpenFileLocation), None);
        assert_eq!(
            on(&item_menu(1, 0, BASE | F_SEARCHING), Cmd::OpenFileLocation),
            Some(true)
        );
        assert_eq!(
            on(&item_menu(1, 0, BASE | F_RECENT), Cmd::OpenFileLocation),
            Some(true)
        );
    }

    #[test]
    fn hide_and_unhide_are_exclusive_and_local() {
        let m = item_menu(1, 0, BASE);
        assert_eq!(on(&m, Cmd::Hide), Some(true));
        assert_eq!(on(&m, Cmd::Unhide), None);
        let m = item_menu(1, 0, (BASE & !F_HIDEABLE) | F_UNHIDEABLE);
        assert_eq!(on(&m, Cmd::Unhide), Some(true));
        assert_eq!(on(&m, Cmd::Hide), None);
        // A dot file (hidden already, not by `.hidden`): neither.
        let m = item_menu(1, 0, BASE & !F_HIDEABLE);
        assert_eq!(on(&m, Cmd::Hide), None);
        assert_eq!(on(&m, Cmd::Unhide), None);
        // Search results come from many folders: not hidden from here.
        let m = item_menu(1, 0, BASE | F_SEARCHING);
        assert_eq!(on(&m, Cmd::Hide), Some(false));
        // Not on a server.
        let m = item_menu(1, 0, BASE & !F_LOCAL);
        assert_eq!(on(&m, Cmd::Hide), Some(false));
    }

    #[test]
    fn restore_is_offered_in_the_trash_for_what_was_trashed() {
        let m = item_menu(2, 0, F_IN_TRASH | F_TRASH_TOP | F_LOCAL | F_WRITABLE);
        assert_eq!(on(&m, Cmd::Restore), Some(true));
        // Inside a trashed folder: listed, off.
        let m = item_menu(1, 0, F_IN_TRASH | F_LOCAL | F_WRITABLE);
        assert_eq!(on(&m, Cmd::Restore), Some(false));
        // Not offered anywhere else.
        assert_eq!(on(&item_menu(1, 0, BASE), Cmd::Restore), None);
    }

    #[test]
    fn the_trash_cannot_be_trashed_but_can_be_emptied_for_good() {
        let m = item_menu(1, 0, F_IN_TRASH | F_LOCAL | F_WRITABLE);
        assert_eq!(on(&m, Cmd::Trash), Some(false));
        assert_eq!(on(&m, Cmd::Rename), Some(false));
        assert_eq!(on(&m, Cmd::DeleteForGood), Some(true));
    }

    #[test]
    fn terminal_follows_the_fact() {
        assert_eq!(on(&item_menu(1, 0, BASE), Cmd::OpenTerminal), Some(true));
        assert_eq!(
            on(&item_menu(1, 0, BASE & !F_TERMINAL), Cmd::OpenTerminal),
            Some(false)
        );
    }

    #[test]
    fn background_menu_has_the_fixed_set() {
        let m = background_menu(F_WRITABLE | F_LOCAL | F_TERMINAL | F_PINNABLE);
        for c in [
            Cmd::New,
            Cmd::Paste,
            Cmd::Undo,
            Cmd::Redo,
            Cmd::Sort,
            Cmd::View,
            Cmd::OpenTerminal,
            Cmd::PinFolder,
            Cmd::Properties,
        ] {
            assert!(on(&m, c).is_some(), "{:?}", c);
        }
        assert_eq!(on(&m, Cmd::New), Some(true));
        assert_eq!(on(&m, Cmd::Paste), Some(false), "nothing to paste");
        assert_eq!(on(&m, Cmd::Undo), Some(false));
        assert_eq!(on(&m, Cmd::Sort), Some(true));
        assert_eq!(on(&m, Cmd::View), Some(true));
        assert_eq!(on(&m, Cmd::OpenTerminal), Some(true));
    }

    #[test]
    fn background_undo_redo_follow_the_history() {
        let m = background_menu(F_WRITABLE | F_CAN_UNDO);
        assert_eq!(on(&m, Cmd::Undo), Some(true));
        assert_eq!(on(&m, Cmd::Redo), Some(false));
        let m = background_menu(F_WRITABLE | F_CAN_REDO);
        assert_eq!(on(&m, Cmd::Redo), Some(true));
    }

    #[test]
    fn background_of_a_read_only_folder_or_results() {
        let m = background_menu(F_CAN_PASTE | F_TERMINAL);
        assert_eq!(on(&m, Cmd::New), Some(false));
        assert_eq!(on(&m, Cmd::Paste), Some(false));
        assert_eq!(on(&m, Cmd::Sort), Some(true));
        // Results: nothing is made or pasted "here", and the folder's own
        // properties and pin are not what is shown.
        let m = background_menu(F_SEARCHING | F_CAN_PASTE);
        assert_eq!(on(&m, Cmd::New), Some(false));
        assert_eq!(on(&m, Cmd::PinFolder), None);
        assert_eq!(on(&m, Cmd::Properties), Some(false));
    }

    #[test]
    fn state_text_has_one_line_each() {
        let t = state_text(&[(Cmd::Open, true), (Cmd::Cut, false)]);
        assert_eq!(t, "open+\ncut-\n");
        assert_eq!(state_text(&[]), "");
    }

    #[test]
    fn templates_are_filtered_and_ordered() {
        let names: Vec<String> = [
            "Spreadsheet.ods",
            ".hidden",
            "backup~",
            "document 10.odt",
            "Document 2.odt",
            "half.part",
            "readme",
            "a.tmp",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            pick_templates(&names),
            [
                "Document 2.odt",
                "document 10.odt",
                "readme",
                "Spreadsheet.ods"
            ]
        );
    }

    #[test]
    fn templates_are_capped() {
        let names: Vec<String> = (0..100).map(|i| format!("t{i:03}.txt")).collect();
        assert_eq!(pick_templates(&names).len(), MAX_TEMPLATES);
    }

    #[test]
    fn template_filter_refuses_odd_names() {
        assert!(!template_ok(""));
        assert!(!template_ok("a/b"));
        assert!(!template_ok("a\nb"));
        assert!(template_ok("Report.docx"));
        assert!(template_ok("Report (copy).docx"));
    }

    #[test]
    fn free_names_count_up() {
        let taken = ["New Folder", "New Folder (2)"];
        assert_eq!(
            free_name("New Folder", |n| taken.contains(&n)),
            "New Folder (3)"
        );
        assert_eq!(free_name("Other", |n| taken.contains(&n)), "Other");
        assert_eq!(
            free_name("New Text File.txt", |n| n == "New Text File.txt"),
            "New Text File (2).txt"
        );
    }

    #[test]
    fn labels_and_proposed_names() {
        assert_eq!(template_label("Spreadsheet.ods"), "Spreadsheet");
        assert_eq!(template_label("notes.tar.gz"), "notes");
        assert_eq!(template_label("Makefile"), "Makefile");
        assert_eq!(template_label(".bashrc"), ".bashrc");
        assert_eq!(new_file_name("Spreadsheet.ods"), "New Spreadsheet.ods");
        assert_eq!(new_file_name("New Note.txt"), "New Note.txt");
        assert_eq!(NEW_TEXT_FILE, "New Text File.txt");
        assert!(name_problem(NEW_TEXT_FILE).is_ok());
        assert!(name_problem(&new_file_name("x")).is_ok());
    }

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn hide_appends_and_keeps_the_rest() {
        assert_eq!(
            hidden_add("", &strings(&["a"])),
            Ok(Some("a\n".to_string()))
        );
        assert_eq!(
            hidden_add("x\ny", &strings(&["a", "b"])),
            Ok(Some("x\ny\na\nb\n".to_string()))
        );
        assert_eq!(hidden_add("a\n", &strings(&["a"])), Ok(None));
        assert_eq!(hidden_add("a\r\nb\r\n", &strings(&["b"])), Ok(None));
        // Comment-like lines are names like any other.
        assert_eq!(
            hidden_add("# x\n", &strings(&["#"])),
            Ok(Some("# x\n#\n".to_string()))
        );
    }

    #[test]
    fn hide_refuses_what_is_not_a_name() {
        assert_eq!(
            hidden_add("", &strings(&["a\nb"])),
            Err(HideError::LineBreak)
        );
        assert_eq!(hidden_add("", &strings(&["a/b"])), Err(HideError::NotAName));
        assert_eq!(hidden_add("", &strings(&[".."])), Err(HideError::NotAName));
        assert_eq!(hidden_add("", &strings(&[""])), Err(HideError::NotAName));
        // Nothing is half done: a bad name anywhere refuses the whole.
        assert!(hidden_add("", &strings(&["ok", "bad/name"])).is_err());
        let big = "x\n".repeat(HIDDEN_FILE_MAX);
        assert_eq!(hidden_add(&big, &strings(&["a"])), Err(HideError::TooBig));
        assert_eq!(
            hidden_remove(&big, &strings(&["a"])),
            Err(HideError::TooBig)
        );
    }

    #[test]
    fn unhide_removes_only_that_name() {
        assert_eq!(
            hidden_remove("a\nb\nc\n", &strings(&["b"])),
            Ok(Some("a\nc\n".to_string()))
        );
        assert_eq!(
            hidden_remove("a\r\nb\r\n", &strings(&["a"])),
            Ok(Some("b\r\n".to_string()))
        );
        assert_eq!(hidden_remove("a\n", &strings(&["z"])), Ok(None));
        assert_eq!(
            hidden_remove("a\nb", &strings(&["b"])),
            Ok(Some("a\n".to_string()))
        );
        // A name that merely contains another is kept.
        assert_eq!(hidden_remove("ab\n", &strings(&["a"])), Ok(None));
    }

    #[test]
    fn hidden_names_skips_blank_lines() {
        assert_eq!(hidden_names("a\n\nb\r\n"), ["a", "b"]);
        assert!(hidden_names("").is_empty());
    }
}
