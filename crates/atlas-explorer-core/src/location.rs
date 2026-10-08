//! The path bar's decisions, with no Qt: the clickable segments of a
//! location, and which subfolders the chevron menu lists and in what order.
//! Pure data in, data out, so it is tested without a display. The window
//! lists folders on workers and draws the bar (qml/PathBar.qml); see
//! docs/DESIGN.md, "Window".

use crate::address::Candidate;
use crate::archive;
use crate::display::display_name;
use crate::launch::scheme_of;
use crate::sort::name_key;

/// Rows a subfolder menu shows; a folder with more says how many are left out.
pub const MAX_MENU: usize = 100;
/// Places a Back or Forward menu lists.
pub const MAX_HISTORY_MENU: usize = 10;
/// Longest location the bar splits, in bytes (a path of 4096 bytes fully
/// percent-encoded is 12288).
pub const MAX_URL_LEN: usize = 32 * 1024;

/// One clickable part of the path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// What the bar shows: the folder's name made safe to show, or a place's
    /// own name (Home, Trash, Recent, Network).
    pub label: String,
    /// Where it leads, as a URL.
    pub url: String,
}

fn decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(hex) = b.get(i + 1..i + 3)
            && let Ok(hex) = std::str::from_utf8(hex)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

/// The segments of `url` (as KIO writes it, percent-encoded), from the top
/// of the place to the folder itself. `home` is the user's home folder as a
/// plain path: a local location inside it starts at one segment "Home" and
/// goes on from there (Home > Documents), anything else on the local disk
/// starts at "Root". `trash:`, `recentlyused:` and `network:` start at their
/// own names; a location on a server starts at the server. Empty when the
/// text is not a location the bar can split.
pub fn segments(url: &str, home: &str) -> Vec<Segment> {
    if url.len() > MAX_URL_LEN || url.chars().any(char::is_control) {
        return Vec::new();
    }
    let Some(scheme) = scheme_of(url) else {
        return Vec::new();
    };
    let scheme_lc = scheme.to_ascii_lowercase();
    // Inside an archive the bar shows where the archive is, then the archive
    // as a folder, then the folders in it.
    if archive::is_scheme(&scheme_lc)
        && let Some(loc) = archive::locate(url)
    {
        let mut out = segments(&loc.file_url, home);
        if let Some(last) = out.last_mut() {
            last.url = loc.root_url.clone();
        }
        let mut prefix = loc.root_url;
        for p in loc.inner.split('/').filter(|p| !p.is_empty()) {
            prefix.push('/');
            prefix.push_str(p);
            out.push(Segment {
                label: display_name(&decode(p)),
                url: prefix.clone(),
            });
        }
        return out;
    }
    let rest = &url[scheme.len() + 1..];
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    let (authority, path) = match rest.strip_prefix("//") {
        Some(r) => match r.find('/') {
            Some(i) => (Some(&r[..i]), &r[i..]),
            None => (Some(r), ""),
        },
        None => (None, rest),
    };

    // The part every segment's URL starts with (no trailing slash), and the
    // first segment's label.
    let local = scheme_lc == "file" && matches!(authority, None | Some("") | Some("localhost"));
    let (base, root_label) = if local {
        ("file://".to_string(), String::from("Root"))
    } else if let Some(auth) = authority.filter(|a| !a.is_empty()) {
        let host = auth.rsplit('@').next().unwrap_or(auth);
        let label = if host.is_empty() {
            display_name(scheme)
        } else {
            display_name(&decode(host))
        };
        (format!("{scheme}://{auth}"), label)
    } else {
        let label = match scheme_lc.as_str() {
            "trash" => "Trash".to_string(),
            "recentlyused" => "Recent".to_string(),
            "network" => "Network".to_string(),
            _ => display_name(scheme),
        };
        (format!("{scheme}:"), label)
    };

    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    let mut out = Vec::with_capacity(parts.len() + 1);
    // Local locations inside the home folder start there.
    let mut skip = 0;
    let mut prefix = base.clone();
    if local {
        let home_parts: Vec<&str> = home.split('/').filter(|p| !p.is_empty()).collect();
        if !home_parts.is_empty()
            && parts.len() >= home_parts.len()
            && parts
                .iter()
                .zip(&home_parts)
                .all(|(p, h)| decode(p) == h.as_bytes())
        {
            skip = home_parts.len();
            for p in &parts[..skip] {
                prefix.push('/');
                prefix.push_str(p);
            }
            out.push(Segment {
                label: "Home".to_string(),
                url: prefix.clone(),
            });
        }
    }
    if skip == 0 {
        out.push(Segment {
            label: root_label,
            url: format!("{base}/"),
        });
    }
    for p in &parts[skip..] {
        prefix.push('/');
        prefix.push_str(p);
        out.push(Segment {
            label: display_name(&decode(p)),
            url: prefix.clone(),
        });
    }
    out
}

/// Which subfolders the chevron menu lists, and in what order, as indices
/// into `candidates`: folders only, natural order, hidden ones only when
/// `show_hidden`. Not capped: the menu shows [`MAX_MENU`] rows and says how
/// many more there are.
pub fn subfolder_order(candidates: &[Candidate], show_hidden: bool) -> Vec<usize> {
    let mut keyed: Vec<(Vec<u8>, usize)> = candidates
        .iter()
        .enumerate()
        .filter(|(_, c)| c.is_dir && (show_hidden || !(c.hidden || c.name.starts_with('.'))))
        .map(|(i, c)| (name_key(c.name.as_bytes()), i))
        .collect();
    keyed.sort_unstable();
    keyed.into_iter().map(|(_, i)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/home/u";

    fn crumbs(url: &str) -> Vec<(String, String)> {
        segments(url, HOME)
            .into_iter()
            .map(|s| (s.label, s.url))
            .collect()
    }

    fn pair(l: &str, u: &str) -> (String, String) {
        (l.to_string(), u.to_string())
    }

    #[test]
    fn home_is_one_segment() {
        assert_eq!(crumbs("file:///home/u"), [pair("Home", "file:///home/u")]);
        assert_eq!(
            crumbs("file:///home/u/Documents/a%20b"),
            [
                pair("Home", "file:///home/u"),
                pair("Documents", "file:///home/u/Documents"),
                pair("a b", "file:///home/u/Documents/a%20b"),
            ]
        );
    }

    #[test]
    fn outside_home_starts_at_root() {
        assert_eq!(crumbs("file:///"), [pair("Root", "file:///")]);
        assert_eq!(
            crumbs("file:///usr/share/"),
            [
                pair("Root", "file:///"),
                pair("usr", "file:///usr"),
                pair("share", "file:///usr/share"),
            ]
        );
        // Another user's folder is not "Home", nor a sibling with the same start.
        assert_eq!(crumbs("file:///home/uu")[0].0, "Root");
        assert_eq!(crumbs("file:///home")[0].0, "Root");
        assert_eq!(crumbs("file:///home/u2/x").len(), 4);
        assert_eq!(segments("file:///home/u", "").len(), 3);
        assert_eq!(segments("file:///home/u", "/")[0].label, "Root");
    }

    #[test]
    fn home_is_compared_decoded() {
        let s = segments("file:///home/a%20b/c", "/home/a b");
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].url, "file:///home/a%20b");
        assert_eq!(s[1].label, "c");
    }

    #[test]
    fn places_have_their_own_names() {
        assert_eq!(crumbs("trash:/"), [pair("Trash", "trash:/")]);
        assert_eq!(
            crumbs("trash:/dir/sub"),
            [
                pair("Trash", "trash:/"),
                pair("dir", "trash:/dir"),
                pair("sub", "trash:/dir/sub"),
            ]
        );
        assert_eq!(crumbs("recentlyused:/"), [pair("Recent", "recentlyused:/")]);
        assert_eq!(crumbs("network:/"), [pair("Network", "network:/")]);
        assert_eq!(crumbs("trash:"), [pair("Trash", "trash:/")]);
    }

    #[test]
    fn servers_start_at_the_server() {
        assert_eq!(
            crumbs("smb://nas/share/dir"),
            [
                pair("nas", "smb://nas/"),
                pair("share", "smb://nas/share"),
                pair("dir", "smb://nas/share/dir"),
            ]
        );
        // The user stays in the links, never in a label.
        assert_eq!(
            crumbs("sftp://me@host:22/srv"),
            [
                pair("host:22", "sftp://me@host:22/"),
                pair("srv", "sftp://me@host:22/srv")
            ]
        );
        assert_eq!(crumbs("smb://nas"), [pair("nas", "smb://nas/")]);
        assert_eq!(crumbs("mtp:/Phone/DCIM")[0], pair("mtp", "mtp:/"));
        assert_eq!(
            crumbs("mtp:/Phone/DCIM")[2],
            pair("DCIM", "mtp:/Phone/DCIM")
        );
    }

    #[test]
    fn an_archive_is_a_folder_below_the_folder_it_is_in() {
        assert_eq!(
            crumbs("zip:/home/u/Docs/a%20b.zip/sub/x"),
            [
                pair("Home", "file:///home/u"),
                pair("Docs", "file:///home/u/Docs"),
                pair("a b.zip", "zip:/home/u/Docs/a%20b.zip"),
                pair("sub", "zip:/home/u/Docs/a%20b.zip/sub"),
                pair("x", "zip:/home/u/Docs/a%20b.zip/sub/x"),
            ]
        );
        // The archive's own segment is where the archive opens, not the file.
        let s = segments("tar:///srv/data.tar.gz", HOME);
        assert_eq!(
            s[0],
            Segment {
                label: "Root".into(),
                url: "file:///".into()
            }
        );
        assert_eq!(s.last().unwrap().url, "tar:///srv/data.tar.gz");
        assert_eq!(s.last().unwrap().label, "data.tar.gz");
    }

    #[test]
    fn query_and_fragment_are_dropped() {
        assert_eq!(crumbs("smb://nas/s?x=1#f").len(), 2);
    }

    #[test]
    fn names_are_made_safe_to_show() {
        let s = segments("file:///tmp/a%0Ab%E2%80%AEc", HOME);
        let label = &s[2].label;
        assert!(
            !label.contains('\n') && !label.contains('\u{202E}'),
            "{label}"
        );
        // The link keeps the real bytes.
        assert_eq!(s[2].url, "file:///tmp/a%0Ab%E2%80%AEc");
        // Invalid UTF-8 is shown as hex.
        assert!(segments("file:///tmp/%FF", HOME)[2].label.contains("FF"));
    }

    #[test]
    fn not_a_location() {
        assert!(segments("", HOME).is_empty());
        assert!(segments("/etc", HOME).is_empty());
        assert!(segments("file:///a\nb", HOME).is_empty());
        assert!(segments(&format!("file:///{}", "a".repeat(MAX_URL_LEN)), HOME).is_empty());
    }

    #[test]
    fn a_bad_escape_stays_as_it_is() {
        assert_eq!(decode("a%zzb%4"), b"a%zzb%4");
        assert_eq!(decode("%41%2f"), b"A/");
    }

    fn cand(name: &str, is_dir: bool, hidden: bool) -> Candidate {
        Candidate {
            name: name.into(),
            is_dir,
            hidden,
        }
    }

    #[test]
    fn subfolders_are_folders_in_natural_order() {
        let list = [
            cand("b10", true, false),
            cand("file.txt", false, false),
            cand("B9", true, false),
            cand(".git", true, false),
            cand("flagged", true, true),
            cand("a", true, false),
        ];
        let names = |show| -> Vec<String> {
            subfolder_order(&list, show)
                .into_iter()
                .map(|i| list[i].name.clone())
                .collect()
        };
        assert_eq!(names(false), ["a", "B9", "b10"]);
        assert_eq!(names(true), [".git", "a", "B9", "b10", "flagged"]);
        assert!(subfolder_order(&[], true).is_empty());
    }
}
