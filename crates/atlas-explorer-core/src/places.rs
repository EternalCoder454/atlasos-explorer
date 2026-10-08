//! The sidebar's decisions, with no Qt and no KF6: which section a place
//! belongs to, what kind of place it is, which actions its menu offers, and
//! the plain-word texts (the Trash count, the Empty Trash question, "Safe to
//! remove"). The window gets its places from KIO's `KFilePlacesModel` (so
//! pins are shared through `user-places.xbel`); this module only decides
//! what to do with them. Pure data in, data out, so it is tested without a
//! display. See docs/DESIGN.md, "Window" (Sidebar).

use crate::display::display_name;
use crate::location;

/// Longest name a place can be given, in characters.
pub const MAX_LABEL_CHARS: usize = 80;
/// The Trash shows a count up to this, then "999+".
pub const MAX_TRASH_COUNT_SHOWN: usize = 999;

/// Where a place is listed. The numbers are the C ABI's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Section {
    /// Home, the user's folders, Recent and pinned folders.
    Favourites = 0,
    /// Disks, USB drives, phones and cameras.
    Drives = 1,
    /// Network and saved servers.
    Network = 2,
    /// The Trash, pinned under the list.
    Trash = 3,
    /// Not listed (searches and the dated lists that need Baloo, tags until
    /// they have their own wave).
    Unlisted = 4,
}

/// `KFilePlacesModel::GroupType`, by its numbers.
const GROUP_PLACES: i32 = 0;
const GROUP_REMOTE: i32 = 1;
const GROUP_RECENT: i32 = 2;
const GROUP_DEVICES: i32 = 4;
const GROUP_REMOVABLE: i32 = 5;

fn lower(s: &str) -> String {
    s.to_ascii_lowercase()
}

/// The section of a place from KFilePlacesModel's group number and the
/// scheme of its URL. The Trash is a plain place there; here it gets its own
/// section. Only "Recent" survives of the recently-saved group (its dated
/// entries need an index Telamon doesn't have).
pub fn section_for(group: i32, scheme: &str) -> Section {
    let scheme = lower(scheme);
    if scheme == "trash" {
        return Section::Trash;
    }
    match group {
        GROUP_PLACES => {
            if scheme == "network" || scheme == "remote" {
                Section::Network
            } else {
                Section::Favourites
            }
        }
        GROUP_RECENT => {
            if scheme == "recentlyused" {
                Section::Favourites
            } else {
                Section::Unlisted
            }
        }
        GROUP_DEVICES | GROUP_REMOVABLE => Section::Drives,
        GROUP_REMOTE => Section::Network,
        _ => Section::Unlisted,
    }
}

/// What a place is. The numbers are the C ABI's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Kind {
    /// A folder: Home, a standard folder, a pin.
    Folder = 0,
    /// `recentlyused:`.
    Recent = 1,
    /// The Network place (`network:/`, `remote:/`).
    Network = 2,
    /// A saved server or other remote place.
    Server = 3,
    Trash = 4,
    /// A disk or partition that stays in the machine.
    Drive = 5,
    /// A USB stick, SD card or other drive that can be taken out.
    Removable = 6,
    /// A phone, camera or media player (MTP and the like).
    Phone = 7,
    Other = 8,
}

/// What a place is, from the facts KFilePlacesModel and Solid give.
/// `is_storage` is a device that can be mounted; `is_player` one that KIO
/// reads itself (MTP, a camera).
pub fn kind_of(
    section: Section,
    scheme: &str,
    is_device: bool,
    is_storage: bool,
    removable: bool,
    is_player: bool,
) -> Kind {
    let scheme = lower(scheme);
    if scheme == "trash" {
        return Kind::Trash;
    }
    if is_device {
        return if is_player && !is_storage {
            Kind::Phone
        } else if is_storage && removable {
            Kind::Removable
        } else if is_storage {
            Kind::Drive
        } else {
            Kind::Other
        };
    }
    match scheme.as_str() {
        "recentlyused" => Kind::Recent,
        "network" | "remote" => Kind::Network,
        // Phones and the like that KDE lists as bookmarks (KDE Connect).
        "kdeconnect" | "bluetooth" | "obexftp" | "mtp" => Kind::Phone,
        _ if section == Section::Network => Kind::Server,
        _ => Kind::Folder,
    }
}

// Actions a place's menu offers (bits of the number `actions` returns).
pub const ACT_RENAME: u32 = 1;
pub const ACT_HIDE: u32 = 1 << 1;
pub const ACT_UNHIDE: u32 = 1 << 2;
pub const ACT_REMOVE: u32 = 1 << 3;
pub const ACT_MOUNT: u32 = 1 << 4;
pub const ACT_UNMOUNT: u32 = 1 << 5;
pub const ACT_OPEN_IN_DISKS: u32 = 1 << 6;
pub const ACT_EMPTY_TRASH: u32 = 1 << 7;
pub const ACT_NEW_TAB: u32 = 1 << 8;
/// The place can be dragged to a new position (pins and the folders KDE
/// seeds; devices sit where Solid lists them).
pub const ACT_REORDER: u32 = 1 << 9;
/// Files dropped on the place are moved or copied into it.
pub const ACT_ACCEPTS_FILES: u32 = 1 << 10;

/// The actions a place's menu offers. `mounted` is meaningful for drives;
/// `trash_empty` for the Trash; `disks_installed` is whether Telamon Disks
/// is on the machine (the entry is for drives only, so it stays hidden
/// without it).
pub fn actions(
    kind: Kind,
    hidden: bool,
    mounted: bool,
    trash_empty: bool,
    disks_installed: bool,
) -> u32 {
    let mut a = ACT_NEW_TAB | if hidden { ACT_UNHIDE } else { ACT_HIDE };
    match kind {
        Kind::Folder | Kind::Server => {
            a |= ACT_RENAME | ACT_REMOVE | ACT_REORDER | ACT_ACCEPTS_FILES;
        }
        Kind::Recent | Kind::Network | Kind::Other => {
            a |= ACT_RENAME | ACT_REMOVE | ACT_REORDER;
        }
        Kind::Trash => {
            if !trash_empty {
                a |= ACT_EMPTY_TRASH;
            }
        }
        Kind::Drive | Kind::Removable => {
            a |= if mounted { ACT_UNMOUNT } else { ACT_MOUNT };
            if disks_installed {
                a |= ACT_OPEN_IN_DISKS;
            }
        }
        Kind::Phone => {}
    }
    a
}

/// Where a moved place goes, as the `row` KFilePlacesModel::movePlace takes
/// ("before this row"), when `src` is dropped on `dst`: moving down puts it
/// after the target, moving up before it. `None` for a drop on itself.
pub fn reorder_row(src: usize, dst: usize) -> Option<usize> {
    match src.cmp(&dst) {
        std::cmp::Ordering::Equal => None,
        std::cmp::Ordering::Less => Some(dst + 1),
        std::cmp::Ordering::Greater => Some(dst),
    }
}

/// Whether a folder at a URL with this scheme can be pinned: it has to be a
/// place KIO can open again later, not one that only exists for a moment or
/// is not a folder.
pub fn pinnable(scheme: &str) -> bool {
    matches!(
        lower(scheme).as_str(),
        "file" | "smb" | "sftp" | "ftp" | "ftps" | "webdav" | "webdavs" | "nfs" | "fish" | "gdrive"
    )
}

/// A name a user typed for a place, made safe: control characters dropped,
/// ends trimmed, cut at `MAX_LABEL_CHARS`. `None` when nothing is left.
pub fn clean_label(raw: &str) -> Option<String> {
    let kept: String = raw.chars().filter(|c| !c.is_control()).collect();
    let trimmed = kept.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(MAX_LABEL_CHARS).collect())
}

/// The name a new pin starts with: the folder's own name made safe to show
/// (Home, a server's name and "Root" as in the path bar).
pub fn pin_label(url: &str, home: &str) -> String {
    let segs = location::segments(url, home);
    let label = segs
        .last()
        .map(|s| s.label.clone())
        .unwrap_or_else(|| display_name(url));
    clean_label(&label).unwrap_or_else(|| String::from("Folder"))
}

/// How full a disk is, as a whole percent (0 to 100); -1 when the sizes are
/// not known.
pub fn usage_percent(total: i64, free: i64) -> i32 {
    if total <= 0 || free < 0 {
        return -1;
    }
    let used = (total - free.min(total)) as f64;
    ((used * 100.0 / total as f64).round() as i32).clamp(0, 100)
}

/// A disk this full gets the warning colour.
pub const NEARLY_FULL_PERCENT: i32 = 90;

/// What the Trash shows beside its name: the number of items, nothing while
/// it is empty.
pub fn trash_value(count: usize) -> String {
    match count {
        0 => String::new(),
        n if n > MAX_TRASH_COUNT_SHOWN => format!("{MAX_TRASH_COUNT_SHOWN}+"),
        n => n.to_string(),
    }
}

fn items(count: usize) -> String {
    if count == 1 {
        String::from("1 item")
    } else {
        format!("{count} items")
    }
}

/// The Trash's tooltip.
pub fn trash_tip(count: usize) -> String {
    if count == 0 {
        String::from("Trash is empty")
    } else {
        format!("Trash: {}", items(count))
    }
}

/// The question Empty Trash asks: it names how many items and how big they
/// are (`size` is already written as people read it, "4.2 MiB"; empty when
/// it could not be worked out).
pub fn empty_trash_text(count: usize, size: &str) -> String {
    let what = if count == 0 {
        String::from("everything")
    } else if size.is_empty() {
        format!("all {}", items(count))
    } else if count == 1 {
        format!("the item ({size})")
    } else {
        format!("all {count} items ({size})")
    };
    format!("Permanently delete {what} in the Trash? This can't be undone.")
}

/// What is said when a drive has been unmounted: a drive that can be taken
/// out is "Safe to remove". `name` is made safe to show already.
pub fn unmounted_text(kind: Kind, name: &str) -> String {
    if kind == Kind::Removable {
        format!("{name}: Safe to remove")
    } else {
        format!("{name} unmounted")
    }
}

/// How Telamon Disks can be found on the machine: desktop file IDs, then
/// program names. Disks is installed when any one is present.
pub const DISKS_DESKTOP_IDS: [&str; 2] = [
    "net.eterneon.telamon.disks.desktop",
    "net.eterneon.atlas.disks.desktop",
];
pub const DISKS_PROGRAMS: [&str; 2] = ["telamon-disks", "atlas-disks"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_follow_the_group_and_the_scheme() {
        assert_eq!(section_for(GROUP_PLACES, "file"), Section::Favourites);
        assert_eq!(section_for(GROUP_PLACES, "trash"), Section::Trash);
        assert_eq!(section_for(GROUP_PLACES, "TRASH"), Section::Trash);
        assert_eq!(section_for(GROUP_PLACES, "remote"), Section::Network);
        assert_eq!(section_for(GROUP_PLACES, "network"), Section::Network);
        assert_eq!(section_for(GROUP_REMOTE, "remote"), Section::Network);
        assert_eq!(section_for(GROUP_REMOTE, "smb"), Section::Network);
        assert_eq!(section_for(GROUP_DEVICES, "file"), Section::Drives);
        assert_eq!(section_for(GROUP_REMOVABLE, "file"), Section::Drives);
        assert_eq!(
            section_for(GROUP_RECENT, "recentlyused"),
            Section::Favourites
        );
        assert_eq!(section_for(GROUP_RECENT, "timeline"), Section::Unlisted);
        assert_eq!(section_for(3, "baloosearch"), Section::Unlisted);
        assert_eq!(section_for(7, "tags"), Section::Unlisted);
        assert_eq!(section_for(99, "file"), Section::Unlisted);
    }

    #[test]
    fn kinds_come_from_what_the_place_is() {
        use Section::*;
        assert_eq!(
            kind_of(Favourites, "file", false, false, false, false),
            Kind::Folder
        );
        assert_eq!(
            kind_of(Trash, "trash", false, false, false, false),
            Kind::Trash
        );
        assert_eq!(
            kind_of(Favourites, "recentlyused", false, false, false, false),
            Kind::Recent
        );
        assert_eq!(
            kind_of(Network, "remote", false, false, false, false),
            Kind::Network
        );
        assert_eq!(
            kind_of(Network, "smb", false, false, false, false),
            Kind::Server
        );
        assert_eq!(
            kind_of(Drives, "file", true, true, false, false),
            Kind::Drive
        );
        assert_eq!(
            kind_of(Drives, "file", true, true, true, false),
            Kind::Removable
        );
        // A phone is a player KIO reads itself, not a volume.
        assert_eq!(kind_of(Drives, "mtp", true, false, true, true), Kind::Phone);
        assert_eq!(
            kind_of(Drives, "kdeconnect", false, false, false, false),
            Kind::Phone
        );
        assert_eq!(kind_of(Drives, "x", true, false, false, false), Kind::Other);
    }

    #[test]
    fn menus_offer_what_a_place_can_do() {
        let folder = actions(Kind::Folder, false, false, false, true);
        assert!(folder & ACT_RENAME != 0 && folder & ACT_HIDE != 0 && folder & ACT_REMOVE != 0);
        assert!(folder & ACT_UNHIDE == 0 && folder & ACT_OPEN_IN_DISKS == 0);
        assert!(folder & ACT_REORDER != 0 && folder & ACT_ACCEPTS_FILES != 0);
        let hidden = actions(Kind::Folder, true, false, false, false);
        assert!(hidden & ACT_UNHIDE != 0 && hidden & ACT_HIDE == 0);
        // Recent takes no files; the Network place neither.
        assert!(actions(Kind::Recent, false, false, false, false) & ACT_ACCEPTS_FILES == 0);
        assert!(actions(Kind::Network, false, false, false, false) & ACT_ACCEPTS_FILES == 0);
        // The Trash cannot be renamed or removed; Empty Trash needs something in it.
        let trash = actions(Kind::Trash, false, false, false, true);
        assert!(trash & (ACT_RENAME | ACT_REMOVE | ACT_OPEN_IN_DISKS) == 0);
        assert!(trash & ACT_EMPTY_TRASH != 0);
        assert!(actions(Kind::Trash, false, false, true, false) & ACT_EMPTY_TRASH == 0);
    }

    #[test]
    fn drives_mount_unmount_and_open_in_disks_only_when_installed() {
        for kind in [Kind::Drive, Kind::Removable] {
            let off = actions(kind, false, false, false, false);
            assert!(off & ACT_MOUNT != 0 && off & ACT_UNMOUNT == 0);
            assert!(off & (ACT_RENAME | ACT_REMOVE | ACT_REORDER) == 0);
            let on = actions(kind, false, true, false, false);
            assert!(on & ACT_UNMOUNT != 0 && on & ACT_MOUNT == 0);
            // Open in Disks: drives only, and only when Disks is installed.
            assert!(on & ACT_OPEN_IN_DISKS == 0);
            assert!(actions(kind, false, true, false, true) & ACT_OPEN_IN_DISKS != 0);
            assert!(actions(kind, false, false, false, true) & ACT_OPEN_IN_DISKS != 0);
        }
        // Not for a phone, a folder, the Trash or the Network.
        for kind in [
            Kind::Phone,
            Kind::Folder,
            Kind::Trash,
            Kind::Network,
            Kind::Server,
        ] {
            assert!(actions(kind, false, true, false, true) & ACT_OPEN_IN_DISKS == 0);
        }
        // A phone has nothing to mount: KIO reads it.
        let phone = actions(Kind::Phone, false, false, false, true);
        assert!(phone & (ACT_MOUNT | ACT_UNMOUNT) == 0 && phone & ACT_NEW_TAB != 0);
    }

    #[test]
    fn a_moved_place_goes_before_the_row_the_model_takes() {
        assert_eq!(reorder_row(1, 1), None);
        // Down: after the target. Up: before it.
        assert_eq!(reorder_row(1, 4), Some(5));
        assert_eq!(reorder_row(4, 1), Some(1));
        assert_eq!(reorder_row(0, 1), Some(2));
    }

    #[test]
    fn only_folders_that_can_be_opened_again_are_pinned() {
        for s in ["file", "FILE", "smb", "sftp", "ftp", "webdavs", "nfs"] {
            assert!(pinnable(s), "{s}");
        }
        for s in [
            "trash",
            "recentlyused",
            "network",
            "remote",
            "mtp",
            "timeline",
            "baloosearch",
            "tags",
            "http",
            "",
        ] {
            assert!(!pinnable(s), "{s}");
        }
    }

    #[test]
    fn labels_are_cleaned_and_cut() {
        assert_eq!(clean_label("  Photos \u{7} "), Some("Photos".into()));
        assert_eq!(clean_label("a\nb"), Some("ab".into()));
        assert_eq!(clean_label(" \t\n "), None);
        assert_eq!(clean_label(""), None);
        let long = "x".repeat(200);
        assert_eq!(clean_label(&long).unwrap().chars().count(), MAX_LABEL_CHARS);
    }

    #[test]
    fn a_new_pin_is_named_after_its_folder() {
        assert_eq!(
            pin_label("file:///home/zach/Projects", "/home/zach"),
            "Projects"
        );
        assert_eq!(
            pin_label("file:///home/zach/My%20Photos/", "/home/zach"),
            "My Photos"
        );
        assert_eq!(pin_label("file:///home/zach", "/home/zach"), "Home");
        assert_eq!(pin_label("file:///", "/home/zach"), "Root");
        assert_eq!(pin_label("smb://nas/share", "/home/zach"), "share");
        // Controls in a name never reach a label.
        assert!(
            !pin_label("file:///tmp/a%0Ab", "/home/zach")
                .chars()
                .any(char::is_control)
        );
        assert_eq!(pin_label("", "/home/zach"), "Folder");
    }

    #[test]
    fn usage_is_a_clamped_whole_percent() {
        assert_eq!(usage_percent(1000, 250), 75);
        assert_eq!(usage_percent(1000, 0), 100);
        assert_eq!(usage_percent(1000, 1000), 0);
        assert_eq!(usage_percent(1000, 5000), 0);
        assert_eq!(usage_percent(0, 0), -1);
        assert_eq!(usage_percent(1000, -1), -1);
        assert_eq!(usage_percent(-5, 3), -1);
        assert!(usage_percent(1000, 99) >= NEARLY_FULL_PERCENT);
    }

    #[test]
    fn the_trash_shows_its_count() {
        assert_eq!(trash_value(0), "");
        assert_eq!(trash_value(1), "1");
        assert_eq!(trash_value(999), "999");
        assert_eq!(trash_value(1000), "999+");
        assert_eq!(trash_tip(0), "Trash is empty");
        assert_eq!(trash_tip(1), "Trash: 1 item");
        assert_eq!(trash_tip(12), "Trash: 12 items");
    }

    #[test]
    fn empty_trash_names_the_size() {
        let t = empty_trash_text(12, "4.2 MiB");
        assert!(t.contains("12 items") && t.contains("4.2 MiB") && t.contains("can't be undone"));
        assert!(empty_trash_text(1, "3 B").contains("the item (3 B)"));
        // Without a size the count still says how much.
        let t = empty_trash_text(5, "");
        assert!(t.contains("all 5 items") && !t.contains("()"));
        assert!(empty_trash_text(0, "").contains("everything"));
    }

    #[test]
    fn only_a_removable_drive_is_safe_to_remove() {
        assert_eq!(
            unmounted_text(Kind::Removable, "Backup"),
            "Backup: Safe to remove"
        );
        assert_eq!(unmounted_text(Kind::Drive, "Data"), "Data unmounted");
    }

    #[test]
    fn disks_is_found_by_desktop_id_or_program() {
        assert!(DISKS_DESKTOP_IDS.iter().all(|d| d.ends_with(".desktop")));
        assert!(DISKS_PROGRAMS.iter().all(|p| !p.contains('/')));
    }
}
