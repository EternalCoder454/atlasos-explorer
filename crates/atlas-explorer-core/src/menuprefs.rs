//! Which entries of the context menus the user has hidden (Settings > Context
//! Menu and Actions). Every built-in entry and every service-menu action or
//! plugin can be hidden with a switch; this module keeps the set (as text in
//! Files' settings file, `telamon-explorerrc`, group `[Menu]`, key `Hidden`),
//! reads it as untrusted input, and filters the menu model of `menu` by it.
//!
//! A hidden entry is left out when a menu is made, so the arrow keys skip it
//! and a screen reader never hears it; nothing else about the menu changes.
//! The Settings list always shows every entry, hidden or not, so a switch
//! can bring one back, and the window never hides its own way back: Settings
//! opens from the View menu, the tab menu and Ctrl+, whatever is hidden here.
//!
//! Keys: a built-in entry is its `menu::Cmd` key (or one of the extra keys
//! below, for entries the model does not decide), a service-menu action is
//! `svc:` and its action name, and a plugin of KDE's file-item actions is
//! `plugin:` and its id.

use crate::menu::Cmd;

/// Most hidden entries kept, and the longest key.
pub const MAX_HIDDEN: usize = 300;
pub const MAX_KEY_BYTES: usize = 200;

/// Built-in entries that are not `menu::Cmd` (they come with a context, not
/// from the model): the quick actions on pictures and the split view's.
pub const EXTRA_KEYS: [&str; 8] = [
    "rotateLeft",
    "rotateRight",
    "convertPng",
    "convertJpeg",
    "convertWebp",
    "combinePdf",
    "copyToOtherPane",
    "moveToOtherPane",
];

/// Every `menu::Cmd`, so the Settings list has them all.
pub const ALL_CMDS: [Cmd; 29] = [
    Cmd::Open,
    Cmd::Restore,
    Cmd::OpenWith,
    Cmd::Cut,
    Cmd::Copy,
    Cmd::Paste,
    Cmd::Rename,
    Cmd::Trash,
    Cmd::ExtractHere,
    Cmd::ExtractTo,
    Cmd::CompressZip,
    Cmd::Compress,
    Cmd::Tags,
    Cmd::Properties,
    Cmd::MoreActions,
    Cmd::OpenInNewTab,
    Cmd::OpenFileLocation,
    Cmd::OpenTerminal,
    Cmd::PinToSidebar,
    Cmd::CopyPath,
    Cmd::Hide,
    Cmd::Unhide,
    Cmd::DeleteForGood,
    Cmd::New,
    Cmd::Undo,
    Cmd::Redo,
    Cmd::Sort,
    Cmd::View,
    Cmd::PinFolder,
];

/// The keys of the built-in entries, in the order the Settings list shows them.
pub fn builtin_keys() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = ALL_CMDS.iter().map(|c| c.key()).collect();
    v.extend(EXTRA_KEYS);
    v
}

pub fn service_key(action_name: &str) -> String {
    format!("svc:{action_name}")
}

pub fn plugin_key(plugin_id: &str) -> String {
    format!("plugin:{plugin_id}")
}

/// Whether `key` is a key this module keeps: short, plain, one line.
fn key_ok(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= MAX_KEY_BYTES
        && !key
            .chars()
            .any(|c| c.is_control() || c == '\u{2028}' || c == '\u{2029}')
}

/// The hidden entries.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Hidden {
    keys: Vec<String>,
}

impl Hidden {
    /// Reads the settings text (one key a line). A line that is no key is dropped.
    pub fn parse(text: &str) -> Hidden {
        let mut h = Hidden::default();
        for line in text.lines() {
            if h.keys.len() >= MAX_HIDDEN {
                break;
            }
            let k = line.trim();
            if key_ok(k) && !h.keys.iter().any(|x| x == k) {
                h.keys.push(k.to_string());
            }
        }
        h
    }

    pub fn to_text(&self) -> String {
        let mut s = String::new();
        for k in &self.keys {
            s.push_str(k);
            s.push('\n');
        }
        s
    }

    pub fn is_hidden(&self, key: &str) -> bool {
        self.keys.iter().any(|k| k == key)
    }

    pub fn keys(&self) -> &[String] {
        &self.keys
    }

    /// Hides or shows `key`. False when it can't be kept (not a key, or too many).
    pub fn set(&mut self, key: &str, hidden: bool) -> bool {
        if !key_ok(key) {
            return false;
        }
        let pos = self.keys.iter().position(|k| k == key);
        match (hidden, pos) {
            (true, None) => {
                if self.keys.len() >= MAX_HIDDEN {
                    return false;
                }
                self.keys.push(key.to_string());
            }
            (false, Some(i)) => {
                self.keys.remove(i);
            }
            _ => {}
        }
        true
    }

    /// The entries of a menu model without the hidden ones.
    pub fn filter(&self, entries: Vec<(Cmd, bool)>) -> Vec<(Cmd, bool)> {
        entries
            .into_iter()
            .filter(|(c, _)| !self.is_hidden(c.key()))
            .collect()
    }

    /// The keys of service-menu actions and plugins that are hidden, without
    /// their prefixes, for `KFileItemActions::addActionsTo`'s exclude list.
    pub fn excluded_services(&self) -> Vec<String> {
        self.keys
            .iter()
            .filter_map(|k| {
                k.strip_prefix("svc:")
                    .or_else(|| k.strip_prefix("plugin:"))
                    .map(str::to_string)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::{F_LOCAL, F_WRITABLE, background_menu, item_menu};

    #[test]
    fn every_entry_the_model_can_make_can_be_hidden() {
        let keys = builtin_keys();
        // No key twice.
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), keys.len());
        // Every key the item and background menus can offer is in the list,
        // whatever the facts are.
        for flags in [
            0u32,
            u32::MAX,
            F_WRITABLE,
            F_WRITABLE | F_LOCAL,
            0x5555,
            0xAAAA,
        ] {
            for (count, folders) in [(1usize, 0usize), (1, 1), (3, 1), (3, 3)] {
                for (c, _) in item_menu(count, folders, flags) {
                    assert!(keys.contains(&c.key()), "{} is not hideable", c.key());
                }
            }
            for (c, _) in background_menu(flags) {
                assert!(keys.contains(&c.key()), "{} is not hideable", c.key());
            }
        }
        // And the list holds only keys the window knows.
        for k in EXTRA_KEYS {
            assert!(!ALL_CMDS.iter().any(|c| c.key() == k));
        }
    }

    #[test]
    fn hidden_entries_are_left_out_of_the_menu_model() {
        let mut h = Hidden::default();
        assert!(h.set("copyPath", true));
        assert!(h.set("openTerminal", true));
        let all = item_menu(1, 0, F_WRITABLE | F_LOCAL);
        let kept = h.filter(all.clone());
        assert_eq!(kept.len(), all.len() - 2);
        assert!(
            !kept
                .iter()
                .any(|(c, _)| c.key() == "copyPath" || c.key() == "openTerminal")
        );
        // What is left keeps its order and its enabled state.
        let want: Vec<(Cmd, bool)> = all
            .into_iter()
            .filter(|(c, _)| c.key() != "copyPath" && c.key() != "openTerminal")
            .collect();
        assert_eq!(kept, want);
        // Showing it again brings it back.
        assert!(h.set("copyPath", false));
        assert!(!h.is_hidden("copyPath"));
        assert_eq!(
            h.filter(item_menu(1, 0, F_WRITABLE)).len(),
            item_menu(1, 0, F_WRITABLE).len() - 1
        );
        // Nothing hidden: nothing changes.
        assert_eq!(
            Hidden::default().filter(background_menu(F_WRITABLE)),
            background_menu(F_WRITABLE)
        );
        // Everything hidden: nothing is left, and no entry survives by another route.
        let mut all_hidden = Hidden::default();
        for k in builtin_keys() {
            assert!(all_hidden.set(k, true));
        }
        assert!(all_hidden.filter(item_menu(2, 1, u32::MAX)).is_empty());
        assert!(all_hidden.filter(background_menu(u32::MAX)).is_empty());
    }

    #[test]
    fn the_text_round_trips_and_a_damaged_file_gives_what_is_fine() {
        let mut h = Hidden::default();
        for k in [
            "open",
            "svc:resize_images",
            "plugin:forgetfileitemaction",
            "svc:it's \"x\"",
        ] {
            assert!(h.set(k, true));
        }
        assert!(h.set("open", true), "twice is fine");
        assert_eq!(h.keys().len(), 4);
        assert_eq!(Hidden::parse(&h.to_text()), h);
        let h2 = Hidden::parse("open\n\n  \nnew\nopen\nbad\u{7}key\n\u{2028}\n  spaced  \n");
        assert_eq!(h2.keys(), &["open", "new", "spaced"]);
        assert!(!h.clone().set("", true));
        assert!(!h.clone().set("line\nbreak", true));
        assert!(!h.clone().set(&"k".repeat(MAX_KEY_BYTES + 1), true));
        // Too many are cut.
        let many: String = (0..MAX_HIDDEN + 50).map(|i| format!("k{i}\n")).collect();
        assert_eq!(Hidden::parse(&many).keys().len(), MAX_HIDDEN);
        let mut full = Hidden::parse(&many);
        assert!(!full.set("one more", true));
        assert!(full.set("k0", false));
        assert!(full.set("one more", true));
    }

    #[test]
    fn service_menus_and_plugins_are_told_apart() {
        let mut h = Hidden::default();
        h.set(&service_key("resize"), true);
        h.set(&plugin_key("forgetfileitemaction"), true);
        h.set("copyPath", true);
        assert_eq!(
            h.excluded_services(),
            vec!["resize", "forgetfileitemaction"]
        );
        assert!(h.is_hidden("svc:resize"));
        // A service called like a built-in entry is not that entry.
        h.set(&service_key("copyPath"), true);
        assert!(h.is_hidden("copyPath") && h.is_hidden("svc:copyPath"));
        h.set("copyPath", false);
        assert!(h.is_hidden("svc:copyPath") && !h.is_hidden("copyPath"));
    }
}
