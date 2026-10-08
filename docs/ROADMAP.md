# Telamon Files: roadmap

The curated feature list and the build plan in waves. `docs/DESIGN.md` says how Files is built; this file says
what it will do, in which order, and what is done (see "Wave status" at the end of section 3).

Display name **Files** (formerly Atlas Explorer; crate and binary `telamon-explorer`, app ID
`net.eterneon.telamon.explorer`). Written 2026-10-07 from the owner's 117-item
"Unified File Manager Feature Spec" (Windows 11 Explorer + macOS Finder + Dolphin), the repo as it was at
Telamon Explorer 0.2.0, and web research.

Rules this spec keeps: simple first, power folded away (few things in view; advanced behind menus and one short
Settings page, like Telamon Settings), Telamon.Ui controls, plain words. Functionality only for now (the owner said
stability, reliability and performance come later), but never unsafe: no shell strings (argv only), confirmation
for every destructive or irreversible action, file names always treated as untrusted (display names, PlainText).

Research caveat: Reddit blocks automated fetches, so sentiment comes from Apple/MacRumors/AppleInsider forums, KDE
Discuss and the KDE bug tracker, Arch/Kubuntu bug threads, Microsoft's own Insider blog and tech press. A few
points come from search-result summaries rather than a full page read; those are marked "(summary)". Where the
evidence is thin (Gallery view) the spec says so.

---

## 0. What the code does today (inventory)

State of `main` at Telamon Explorer 0.2.0 (Telamon.Ui 2.0.0, "Files stays Files"), before wave 1. The README and
`docs/DESIGN.md` describe the finished product (tabs, split view, columns, Quick Look, operations popover, ...); the
code was far smaller. Both now carry a Status note that separates what exists from what is planned. Honest status
at 0.2.0:

**Built and working (v0.1):**
- One window showing one folder (`FolderView` over `FolderModel`, a `KCoreDirLister` list model); any KIO URL
  (`file:`, `smb:`, `sftp:`, `trash:`, `recentlyused:`, `network:`, `mtp:` through kio-extras).
- Three views: Details (sortable columns), Icons (thumbnails through `ThumbnailProvider` / KIO PreviewJob), Compact.
- Sort by Name/Size/Type/Modified/Created/Accessed, ascending/descending, folders first, natural sort (Rust core).
  Show-hidden toggle (Ctrl+H, saved).
- Back/Forward/Up buttons; Alt+Up, Backspace; a **plain-text** location label that turns into a text field on click,
  Ctrl+L, F6, Alt+D; address parsing in the Rust core (`~`, `trash:`, `recent:`, `network:`, URL checks). No
  breadcrumb, no completion.
- Sidebar: ten **static** places (Home, Recent, Desktop, Documents, Downloads, Pictures, Music, Videos, Network,
  Trash). No drives, no pins, no tags, no trash count.
- Command bar: New Folder, Cut, Copy, Paste, Rename, Move to Trash, View menu, Sort menu. Shortcuts: Ctrl+C/X/V/Z,
  Delete, Shift+Delete (KIO's confirmation), Ctrl+Shift+N, F2, Shift+F4 (terminal), Ctrl+A, type-ahead, arrow/Home/End/Page keys, click/Shift/Ctrl selection.
- File operations are plain **KIO jobs** with KIO's own dialogs and Plasma's job tracker and
  `KIO::FileUndoManager` (single-step undo). The Rust `OperationQueue`, journal and own `undo` record exist in
  `telamon-explorer-core` and are tested, but **are not wired into the app**.
- Context menu: Open, Open With, KDE service menus, Cut/Copy/Rename/Trash/Delete/Properties; background menu: New
  Folder, Paste, Open Terminal Here, Show Hidden, Properties. It is a `QMenu` popup (widget styling, not Telamon.Ui).
- Properties: KDE's `KPropertiesDialog` (QWidgets, not Telamon.Ui; no checksums yet).
- Drag and drop: drag out, drop on folders/background through `KIO::DropJob` (copy/move/link menu by modifiers).
- Launch: single instance, args parsing, `org.freedesktop.FileManager1` (ShowFolders/ShowItems/ShowItemProperties).
- Executable and `.desktop` trust prompts through KIO. Right-to-left mirroring on. Display names (control/bidi chars made
  visible) in the core.
- **File index service** (`*-indexd`, D-Bus `Search1`, CLI `*-search`): complete, with ranking, kind/date/size
  filters, inotify freshness, snapshot. **Explorer's UI does not use it yet** (no search field).

**Not built at all:** tabs, split view, breadcrumb, Columns and Gallery views, preview/details panes, Quick Look,
status bar, search UI, drives and removable media, MTP/camera polish, Connect to Server, tags, batch rename, archive
integration (Archive's `Archive1` D-Bus API is specified, no caller), operations popover, own conflict dialog,
redo, Home page, per-folder view memory, zoom, mouse side buttons, middle-click, settings page, accessibility
roles, trash tools.

Status words used below: **Done** (works in v0.1), **Partial** (some of it, or the engine exists but no UI/wiring),
**Missing**.

---

## 1. Sentiment summary

### 1.1 macOS Finder

**Loved**
- **Quick Look on Space.** "One of the best features of Mac OS": a near full-size preview of images, PDFs, office
  documents, video and audio without opening an app, and arrow keys move the preview through the selection.
  [MacMost: Viewing File Contents With Quick Look](https://macmost.com/viewing-file-contents-with-quick-look.html),
  [MacMost: Use Quick Look](https://macmost.com/use-quick-look-instead-of-preview-to-view-files.html),
  [O'Reilly: Quick Look](https://www.oreilly.com/library/view/macos-sierra-the/9781491977224/ch02s10.html).
- **Column view** (Miller columns) for deep trees, especially with Quick Look for image folders (same sources).
- **Path bar tricks**: drag a file onto any folder in the path bar to move it there; Option to copy.
  [Macworld: overlooked abilities of the path bar](https://www.macworld.com/article/220947/five-overlooked-abilities-of-the-finders-path-bar.html).
- Tags and smart folders for people who use them (summary of the Finder tips lists:
  [MacMost: 21 useful Finder tips](https://macmost.com/21-actually-useful-finder-tips-and-tricks.html)).

**Hated**
- **No Cut.** Moving needs Copy then Option+Command+V ("Move Item Here"), a hidden modifier. An entire paid utility
  exists to add Cmd+X. [Sindre Sorhus: Command-X](https://sindresorhus.com/command-x),
  [MacRumors: no cut option in Finder](https://forums.macrumors.com/threads/there-is-no-cut-option-in-finder-app.2407806/),
  [Apple Discussions: move to option](https://discussions.apple.com/thread/254648745).
- **No visible path by default**: users want an address bar to know where they are; the path bar is hidden behind
  View > Show Path Bar (Option+Command+P). [iDownloadBlog](https://www.idownloadblog.com/2015/12/16/how-to-finder-path-bar-mac/),
  [MacRumors: I can't stand Finder](https://forums.macrumors.com/threads/i-cant-stand-finder-user-for-around-5-months.1203127/) (summary).
- **Sorting and renaming surprises**: folders-on-top splits opinion (people want a switch, not a rule); a renamed
  file "leaps" to its sorted position so you lose your place; sort settings feel ignored.
  [MacRumors thread 1281546](https://forums.macrumors.com/threads/1281546) (summary),
  [BinaryAge: broken Arrange/Sort](https://discuss.binaryage.com/t/can-tf-fix-broken-finder-arrange-sort/1718) (summary).
- **Per-folder settings in hidden `.DS_Store` files**: clutter synced to Dropbox/Drive, and a source of **slow SMB**
  folders because Finder parses them over the wire. No simple way to set a default for all folders.
  [Studio Network Solutions: SMB folders slow](https://support.studionetworksolutions.com/hc/en-us/articles/360000938163-Folder-contents-slow-to-display-Mac-SMB),
  [Kartick: Simplifying the Finder](https://kartick.substack.com/p/simplifying-and-modernising-the-finder-16-04-09).
- **Network browsing**: shares missing from the sidebar, long delays before a folder lists
  ([TrueNAS forum](https://forums.truenas.com/t/macos-not-finding-smb-shares/67027), summary).

### 1.2 Windows 11 File Explorer

**Loved / worth taking**
- Tabs (late but liked) with drag to reorder and drag out into a window, Ctrl+T / Ctrl+W.
  [gHacks](https://www.ghacks.net/?p=179162), [Winaero: tab drag-out](https://winaero.com/you-now-drag-tabs-out-of-file-explorer-to-open-them-in-a-new-window/?amp).
- A command bar of plain labelled buttons (Cut, Copy, Paste, Rename, Share, Delete) instead of a ribbon.
- Folders first plus natural number sorting as the default; Enter opens, F2 renames.
- The thing people install instead is **Everything**, an instant filename search: that is the bar for search.
  [Windows Report](https://windowsreport.com/file-explorer-is-not-the-fastest-way-to-search-on-windows-try-these-alternative-tools-instead/).

**Hated**
- **Slow**: up to 1-2 s before a right-click menu appears; folders taking seconds to paint; tabs appearing "in slow
  motion". [TechRadar](https://www.techradar.com/computing/windows/finding-windows-11-sluggish-when-youre-working-with-files-and-folders-youre-not-alone-and-its-high-time-for-microsoft-to-sort-out-file-explorer)
  (headline only, page body blocked), [Windows Latest 2021](https://windowslatest.com/2021/11/02/windows-11-is-hit-by-slow-explorer-content-menu-bug-but-a-fix-is-coming),
  [Windows Latest 2026-08](https://windowslatest.com/2026/08/19/microsoft-admits-it-made-windows-11-worse-than-windows-10-for-right-click-menus-promises-to-fix-sluggish-and-cluttered-ux).
- **The context menu**: the "Show more options" second menu, extensions (Copilot, Clipchamp, Paint, Photos) that
  load *after* the menu opens so items shift under the cursor and cause mis-clicks, no user control. Microsoft
  itself admitted this in August 2026 and is adding customisation and grouping.
  [Windows Insider blog 2026-08-17](https://blogs.windows.com/windows-insider/2026/08/17/improving-file-explorer-context-menu-faster-simpler-and-more-customizable/),
  [Windows Latest](https://windowslatest.com/2026/08/19/microsoft-admits-it-made-windows-11-worse-than-windows-10-for-right-click-menus-promises-to-fix-sluggish-and-cluttered-ux).
- **Ads, OneDrive and Copilot nags** in the file manager ("one of the worst places to show ads"), a "Recommended"
  Home. [BleepingComputer](https://www.bleepingcomputer.com/news/microsoft/microsoft-is-testing-ads-in-the-windows-11-file-explorer),
  [Laptop Mag](https://www.laptopmag.com/news/windows-11-shows-ads-in-file-explorer-microsoft-tests-promoting-its-own-products),
  [Tom's Guide](https://tomsguide.com/news/please-dont-do-this-to-windows-11-microsoft).
- **Search that misses files** and does not search inside files by default; renames interrupted by sync.
  [Windows Report](https://windowsreport.com/file-explorer-is-not-the-fastest-way-to-search-on-windows-try-these-alternative-tools-instead/).
- **Gallery view**: little evidence either way. Coverage is announcements and "quite buggy" first builds
  ([Neowin](https://www.neowin.net/news/windows-11-dev-build-23435-brings-new-file-explorer-gallery-but-its-quite-buggy/),
  [BetaNews](https://betanews.com/2023/04/14/windows-11-gallery-file-explorer/)); it was designed around OneDrive's
  Camera Roll. Treat as "good idea, bad dependency": take a photo-browsing view without the cloud tie.
- Tabs arrived years late, with first versions missing drag and reorder
  ([Windows Latest: tabs hands on](https://windowslatest.com/2022/03/10/hands-on-with-windows-11s-leaked-tabs-feature-for-file-explorer)).

### 1.3 KDE Dolphin

**Loved**
- Split view, tabs, an embedded terminal that follows the folder, a filter bar, bulk rename: "our top choice for the
  technical user". [ITPro](https://www.itpro.com/software/linux/359977/best-linux-file-managers),
  [KDE forum: Dolphin ideas](https://forum.kde.org/dolphin-ideas-with-mockups-t-28472-2.html).
- Network protocols through KIO (SFTP, FTP, SMB, MTP, WebDAV) that "just work" in the same window; Open With,
  service menus and Places shared with file dialogs.
  [Linux Mint community reviews](https://community.linuxmint.com/software/view/org.kde.dolphin).
- Recent care for safety and accessibility: red Empty Trash, Restore moved away from Delete, selection mode with
  arrow keys, Orca support, renameable tabs. [Linuxiac: KDE Gear 25.04](https://linuxiac.com/kde-gear-25-04-apps-collection-released/).

**Hated**
- **Too many options / stripped defaults**: "a labyrinth of configuration menus"; toolbar and features hidden until
  configured. [ITPro](https://www.itpro.com/software/linux/359977/best-linux-file-managers). The maintainer's own
  line: "Configuration is not easy", every option multiplies testing
  ([KDE bug 336910](https://bugs.kde.org/show_bug.cgi?id=336910)).
- **`.directory` files** littering folders (the Linux `.DS_Store`), a pain for git and anything reading folders; the fix
  being worked on is extended attributes. [KDE Discuss: per-directory settings](https://discuss.kde.org/t/dolphin-per-directory-settings-revisited/10288),
  [dev.to](https://dev.to/svhl/dolphin-switches-from-directory-to-extended-attributes-jpj).
- **Freezes on slow or dead remote shares**, in some reports for a minute per click, even for local folders
  afterwards; folder-size counting and MIME header reads over the wire make big SMB folders crawl.
  [KDE bug 215953](https://bugs.kde.org/show_bug.cgi?id=215953), [bug 423187 / 2025 thread](https://mail.kde.org/pipermail/kfm-devel/2025-June/057336.html),
  [bug 505069](https://mail.kde.org/pipermail/kfm-devel/2025-May/057229.html),
  [KDE Discuss: Samba sluggish](https://discuss.kde.org/t/samba-share-on-dolphin-is-sluggish/49507).
- **Baloo**: indexer eating CPU/RAM/disk (80-90 % CPU, UI stalls), search that finds nothing or not content.
  [KDE bug 334397](https://bugs.kde.org/show_bug.cgi?id=334397), [Arch BBS](https://bbs.archlinux.org/viewtopic.php?id=231709),
  [Kubuntu bugs 2025](https://lists.ubuntu.com/archives/kubuntu-bugs/2025-August/168814.html),
  [KDE bug 333652](https://bugs.kde.org/show_bug.cgi?id=333652), [bug 438850](https://mail.kde.org/pipermail/kfm-devel/2021-June/039222.html).
- A filter left on and forgotten (users asked for a visible tint). [KDE forum](https://forum.kde.org/viewtopic.php%3Ff=15&t=28472&start=15.html) (summary).
- Split view that "doesn't play well" with the folders panel; tabs preferred by many.

### 1.4 What this means for Telamon Files

Take: Quick Look, column view, drop-on-breadcrumb (Finder); tabs, command bar, natural sort, folders first,
Enter opens (Explorer); split view, filter bar, KIO network, Open With and service menus, bulk rename (Dolphin);
Everything-style instant name search (the owner's own index).

Fix: no hidden Cut; always-visible path; no sidecar files (`.DS_Store` / `.directory`); no UI freeze on dead
servers; a context menu that never shifts; no ads/recommendations; no always-on content indexer; no option
labyrinth.

Where all three fall short: see section 2.0.

---

## 2. Curated feature list

Verdicts: **Keep** (take as is), **Improve** (take, with a better behaviour), **Drop** (not for Telamon). Status is
for the current code (section 0). Item numbers (F1...) are this spec's own and are referenced by the waves.

### 2.0 Top improvements over all three

1. **Never freezes, never writes sidecar files, on remote shares.** Listing is async with a visible Stop and a
   10 s "Server isn't answering" state with Retry; no MIME sniffing, thumbnails or folder-size counting on remote
   folders unless turned on; per-folder view memory lives in Telamon's own settings file, never in `.DS_Store` /
   `.directory`. (Dolphin freezes, Finder's `.DS_Store` slows SMB, both litter.)
2. **Cut and Paste that is honest.** Ctrl+X marks files (dimmed, with "2 items waiting to move"), Ctrl+V moves,
   nothing is deleted until the paste succeeds; "Paste as Copy" in the Paste menu. Finder has no Cut; Explorer's
   Cut state is invisible.
3. **A context menu that is short, stable and yours.** Open / Open With / icon row (Cut, Copy, Paste, Rename,
   Trash) / Compress / Properties on top; rare and third-party items in one named submenu ("More Actions"); built
   fully *before* it opens (never shifts under the cursor); a Settings list to hide any entry. Beats Explorer's
   "Show more options" and Dolphin's long service-menu pile.
4. **Search you can trust.** Instant name search from the index (Everything's speed, no per-keystroke walk), with an
   honest status chip ("Indexing, results may be missing" instead of silently missing files); content search is
   opt-in, on demand, never indexed in the background (Baloo's complaint); results show the folder path and "Open
   file location"; any result can be previewed with Space.
5. **Undo/Redo that says what it will do** ("Undo Move 3 Items to Backup"), covering move, copy, rename, batch
   rename, new folder and trash (permanent delete says it has no undo). Neither Finder (single, vague) nor
   Explorer nor Dolphin states the effect.
6. **Quick Look everywhere**, not just in Finder windows: Space works from any view, from search results, from the
   Trash, with arrow keys moving through the selection and a text/code, image, PDF, audio and video preview.
7. **The path is always there.** Breadcrumb by default (Finder hides it, Explorer's is fine, Dolphin's is fine);
   click the empty part to type; every segment is a drop target and a menu.
8. **Safe names.** Control, bidi and look-alike characters in file names are shown visibly (already in the core), so
   `invoice‮txt.exe` cannot pose as a text file. None of the three do this.
9. **Interruption-free.** No ads, no "Recommended", no sign-in nags, no telemetry, no cloud account: Windows'
   biggest complaint is a non-issue by construction.
10. **Rename keeps your place.** After F2 the view scrolls to and keeps the renamed item selected (and arrow keys
    continue from it) rather than letting it "leap" (Finder complaint).
11. **One simple Settings page** (four small sections) instead of Dolphin's labyrinth or Finder's scattered options;
    everything else is a menu item or a context action.
12. **Tags in an open standard** (`user.xdg.tags` extended attribute, shared with KDE apps), not an Apple-only
    database.

### 2.1 Navigation and layout

| ID | Feature | Verdict | Why and best-of design | Status |
|---|---|---|---|---|
| F1 | Tabs: new, close, reorder, reopen closed, Ctrl+T/W/Shift+T/Tab | **Keep** | Explorer's tabs after drag-out/reorder, Dolphin's "rename tab". Per-tab folder, history, view, selection. Closed tabs list (last 10). | Done (W1) |
| F2 | Middle-click opens folder in a new tab (also on breadcrumb segments and sidebar places); Ctrl+click too | **Keep** | Browser habit, zero learning. Opens in the background; a small "Opened in new tab" cue on the tab. | Done (W1: folder rows by middle click, sidebar places; W2: breadcrumb segments) |
| F3 | Split view (two panes in one tab), copy/move to the other pane | **Keep** | Dolphin's best feature; make it a toolbar toggle and F3. Open the second pane at the same folder; "Copy to Other Pane" / "Move to Other Pane" in the context menu; F5 / F6 do the same *only while split* (assumption: F6 stops meaning "edit address" then; Ctrl+L stays). Active pane has a clear accent line (Dolphin asks "which pane?"). Close the *inactive* pane (KDE wishlist). | Missing |
| F4 | Breadcrumb path bar, editable on click | **Keep / Improve** | Always shown; chevron after a segment opens a menu of sibling subfolders (Explorer); each segment accepts drops (Finder's loved trick); Ctrl+L / Alt+D / F4 edit as text with folder-name completion (local: worker lists; remote: bounded). Path is shown as plain words (`Home > Documents`) with the full path in the edit box. | Done (W2) |
| F5 | Back / Forward / Up, Alt+Left/Right/Up, mouse side buttons | **Keep** | Back/Forward long-press (or right-click) shows the history list (browser style). Side buttons (Qt.BackButton/ForwardButton) in every view. | Done (W1: side buttons, Alt+Left/Right per tab; W2: the history menus) |
| F6 | Sidebar: places, drives, favourites, tags, network, trash | **Keep / Improve** | One list from `KFilePlacesModel` (shares `user-places.xbel` with every Open/Save dialog), sections: Favourites (drag to pin/reorder), Drives (usage bar, eject), Network, Tags, Trash with item count. Hide-a-place via right-click (Dolphin), never a settings maze. | **Done (W3)** |
| F7 | Folder tree panel | **Drop** (as a separate panel) | Costs width, duplicates breadcrumb menus + sidebar + Columns view; Finder dropped it, Explorer hides it. The breadcrumb chevron menu and Columns view cover the use. | n/a |
| F8 | Status bar (item count, selection size, free space) | **Keep** | Slim one-line footer (Dolphin 25.04 slimmed theirs); also shows "Filter on" and index status. Hidden items count shown ("12 hidden") so empty-looking folders aren't confusing. | Done (W2: "12 items, 5 hidden, 3 selected (4.2 MiB), 128 GiB free"; W4: while search results show it counts them, "42 results", and the index status is the chip in the search row; "Filter on" comes with wave 15) |
| F9 | Home page: pinned folders, recent files, frequent folders | **Improve** | Explorer's Home *without* Recommended/cloud/ads. Three plain sections, each collapsible; only things the user opened or pinned. Reads `recently-used.xbel` + the index. | Missing |
| F10 | Recent (files/folders) | **Keep** | `recentlyused:/` place. | Done (place exists; browse depends on KIO worker) |
| F11 | Go to Folder (Ctrl+Shift+G / Ctrl+L) | **Keep** | Same as the editable address bar; accepts `~`, paths, URLs (`smb://`, `sftp://`), `trash:`; unknown scheme refused in plain words. Completion list as you type. | Done (W2: Ctrl+L, F4, F6, Alt+D, completion; Ctrl+Shift+G is not bound) |
| F12 | Pinned folders / Quick Access | **Keep** | Favourites in the sidebar (F6); "Pin to Sidebar" in the folder context menu and on the breadcrumb. | Partial (W3: drag a folder onto the sidebar, or "Pin to Sidebar" in the folder menus; not on the breadcrumb yet) |
| F13 | Open in new window | **Keep** | Context item + `--new-window`; tabs dragged out of the window make a window. | Partial (launch flag parsed, no multi-window UI) |
| F14 | Dark mode, theming | **Keep** | Inherited from Telamon.Ui; nothing to build in Files beyond not hard-coding colours. | Done (framework) |
| F15 | Accessibility | **Keep** | Full keyboard operation; every row exposes name, type, selected and size to screen readers (Orca); focus ring always visible; honours reduced motion and high contrast; RTL mirrored. | Partial (RTL mirroring; no Accessible roles) |
| F16 | Window state memory (size, last tabs) | **Keep** | Restore last window size; tabs restored only if the user opts in (default: start on Home). | Partial (framework `stateKey`) |

### 2.2 Views

| ID | Feature | Verdict | Why and best-of design | Status |
|---|---|---|---|---|
| F20 | Icons, List (compact) and Details views | **Keep** | The three everyday views; a single View menu button and Ctrl+1..5 hint. | Done |
| F21 | Columns view (Miller columns) with preview in the last column | **Keep** | Finder's best loved view; deep folders and photos. Right arrow enters, left goes back, Space = Quick Look. Optional (in the View menu, not the default). | Missing |
| F22 | Gallery view (big preview + filmstrip) | **Keep (no cloud)** | Photo folders; arrow keys; no OneDrive/iCloud tie. Always in the View menu; nothing switches to it automatically. | Missing |
| F23 | Cover Flow | **Drop** | Apple removed it itself; Gallery replaces it. | n/a |
| F24 | Folders first, natural sort (file2 before file10), case-insensitive | **Keep** | On by default, one toggle each in the Sort menu (Finder users disagree on folders first, so it is a plain switch). | Done |
| F25 | Sort and Group by (Name, Size, Type, Modified; group by Kind or Date) | **Improve** | Sort menu with Group by submenu (Windows/Finder both have it); stable sort. Group headers collapsible. | Partial (sort done, grouping missing) |
| F26 | Per-folder view settings | **Improve** | Remembered per folder (view mode, sort, icon size) in Telamon's settings file (bounded LRU of ~500 folders), **never** `.directory` or `.DS_Store`. Settings switch "Use one view for all folders" and per-folder "Reset to default" and "Apply to all folders" (Finder users cannot do this). | Missing |
| F27 | Ctrl+scroll zoom and zoom slider | **Keep** | Ctrl+wheel changes icon size (Icons) / row height (Details) with limits and a quick reset (Ctrl+0). Also Ctrl+plus/minus. | Missing (icon size property exists, no input) |
| F28 | Thumbnails | **Keep** | Local files by default via KIO thumbnailers; remote only if the user turns it on (avoids the SMB crawl). | Done (local); remote rule Missing |
| F29 | Preview pane (Alt+P) and details pane | **Keep** | Right-hand pane: large thumbnail / text / media, then name, kind, size, dates, dimensions or duration. One pane with two parts; off by default. | Missing |
| F30 | Quick Look on Space | **Keep / Improve** | See 2.0 item 6. Arrow keys move through the *selection or the folder*; Space/Esc closes; Enter opens in the app. Text capped at 256 KB, plain text only. | Missing |
| F31 | Selection checkboxes / selection mode | **Keep** | Dolphin 25.04 selection mode and Explorer's checkbox mode: toggle in "..." menu for touch/keyboard-only users. | Missing |
| F32 | Show hidden files, show extensions | **Keep** | Ctrl+H for hidden (saved); extensions always shown (security: never hide extensions). | Done (hidden); extensions always shown |
| F33 | Filter bar (type to narrow the current folder) | **Improve** | Ctrl+F opens a "Filter this folder" field; when on, the list area gets a visible tint/chip ("Showing 4 of 120, Clear") to fix Dolphin's forgotten filter. Same field switches to *Search* (F52) with one click. | Missing |
| F34 | Desktop stacks, icon grid snapping, "Clean Up" | **Drop** | Apple's Desktop is not Files' job; Plasma handles the desktop. | n/a |

### 2.3 File operations

| ID | Feature | Verdict | Why and best-of design | Status |
|---|---|---|---|---|
| F40 | Copy, Cut, Paste | **Keep / Improve** | See 2.0 item 2: cut items dimmed; "Paste" moves, Paste menu has "Paste as Copy". Clipboard shared with other KDE apps (text/uri-list + `application/x-kde-cutselection`). | Done (functional); dim + count Missing |
| F41 | Rename (F2), inline | **Improve** | Inline edit; selects the base name without the extension; name check from the core (refuses `/`, NUL, only dots; warns on control/bidi chars); keeps selection after (2.0 item 10). | Partial (rename via KDE dialog; no inline edit, no keep-place) |
| F42 | Batch rename | **Keep** | Dolphin's dialog: select 2+, F2. Modes: Find and replace, Number (Name 001), Change case, Add text before/after. Live before/after preview, conflict and illegal-name detection, one undo step. Argv-free (pure Rust renames through the queue). | Missing |
| F43 | New folder / New file from templates | **Keep** | Ctrl+Shift+N; "New" menu: Folder, Text File, plus `~/Templates` entries. After creation the name is in edit mode. | Partial (New Folder only) |
| F44 | Trash (default delete), restore, empty | **Keep / Improve** | Delete = Trash. Shift+Delete = "Delete 3 items for good? This can't be undone." default Cancel. Trash view toolbar: Restore, Empty Trash (red, shows size, confirms). Original-location and deleted-date columns. Trash item count in sidebar. | Partial (Trash browses via KIO, Delete/Shift+Delete via KDE dialog; no Trash tools) |
| F45 | Trash auto-empty (older than N days) | **Keep (opt-in)** | One Settings switch "Empty Trash items older than [30] days", off by default (Finder offers 30 days; Dolphin has it in System Settings). Warn when turning on. Runs when Files starts and daily while open; logs what it removed. Check `trashrc` / `ktrash6` before reimplementing. | Missing |
| F46 | Undo / Redo | **Improve** | See 2.0 item 5. Ctrl+Z / Ctrl+Shift+Z (and Ctrl+Y). Undo list (last 20) in the Edit/"..." menu. Refuse politely when files changed since (core `undo.rs` does this check). | Partial (KIO one-step undo; core record exists but is not wired; no redo) |
| F47 | Operations queue and progress (speed, time left, pause, cancel) | **Improve** | One popover from a toolbar ring: one row per operation, speed, time left, Pause/Resume, Cancel, "Run now". Big copy jobs do not block the window. | Partial (core queue exists; app uses Plasma job tracker) |
| F48 | Conflict dialog (Replace, Skip, Keep Both, do for all, side by side) | **Improve** | Explorer's detail (both files with thumbnail, size, date, "newer") + Finder's clear "Keep Both" + Dolphin's rename field. Merge/Skip for folders. In Telamon.Ui, not KDE widgets. | Partial (KIO's dialog) |
| F49 | Drag and drop incl. onto folders, breadcrumbs, tabs, sidebar places, between panes | **Keep / Improve** | Default move on same drive, copy across drives; Ctrl copy, Shift move, Alt/Ctrl+Shift link; a cursor badge shows "Copy / Move / Link"; spring-loaded folders (hover 1 s opens it). | Partial (to folders, background, tabs (W1, KIO's menu) and breadcrumb segments (W2, silent move or Ctrl copy); no pane/sidebar targets) |
| F50 | Compress / Extract (ZIP etc.) | **Keep** | Through Telamon Archive's `Archive1` D-Bus API: Extract Here, Extract To..., Compress to ZIP, Compress...; jobs appear in the queue; hidden when Archive isn't installed. Files never links libarchive. | Missing |
| F51 | Browse archives as folders | **Keep (read-only)** | Open `.zip`/`.tar.*` like a folder (read-only, Explorer-style) using KIO's `zip:/` / `tar:/` workers from kio-extras *until* Archive ships `atlas-archive:`/Telamon equivalent; toolbar shows "Extract". Drag out of an archive to extract. Assumption: kio-extras archive worker present on the image (verify). | Missing |
| F52 | Open With, default app | **Keep** | Context "Open With" submenu with recent apps + "Other Application..." and "Always use this"; Properties changes the default. | Done (KIO `KFileItemActions`) |
| F53 | Copy path / Copy as path | **Keep** | Ctrl+Shift+C copies the full path as plain text (not quoted); "Copy Path as Quoted"/URL in More Actions. Works in search results. | Missing |
| F54 | Open terminal here | **Keep** | Shift+F4 / context (uses Ghostty from kdeglobals via KTerminalLauncherJob, no shell string). | Done |
| F55 | Embedded terminal panel (F4) | **Drop (for now)** | Dolphin's panel is a KParts QWidget and cannot sit in a QML app cleanly; Open Terminal Here covers 95%. Revisit with a QML terminal widget later. | n/a |
| F56 | Hide files from view | **Keep** | "Hide" in More Actions writes to the folder's `.hidden` file (read by the index and Nautilus/Dolphin); "Show Hidden" (Ctrl+H) reveals them dimmed. Needs write access; says so otherwise. | Missing |
| F57 | Open file location (from search, recent, Trash-origin) | **Keep** | Context item and Ctrl+Enter on a result: opens the parent folder with the file selected. | Partial (W4: Ctrl+Enter and "Open File Location" in the context menu on search results, which show the parent in the tab with the file selected; Recent and Trash-origin items wait for their own wave) |
| F58 | Quick actions: rotate image, convert image, combine to PDF, resize | **Keep (small)** | "More Actions > Rotate Left/Right, Convert to PNG/JPEG/WebP, Combine into PDF" for images; in-process Rust image code for rotate/convert, `pdfunite`/`qpdf` by argv for merging PDFs; writes new files (original kept), never in place without Undo. No "Copilot/AI" actions. | Missing |
| F59 | Link creation (symlink / "Create Shortcut") | **Keep** | "Link Here" in drop menu and Paste menu; named "<Name> (link)". | Partial (drop menu) |
| F60 | Properties window | **Keep / Improve** | One window: General (name, kind, size with folder-size on demand, dates, location), Permissions (read-only summary + change for own files), Details (dimensions, duration), Checksums (SHA-256 on demand, worker thread, cancellable), Open With, Tags and rating. Folder size is computed only when asked (Dolphin remote crawl). | Partial (KPropertiesDialog) |
| F61 | Share menu | **Keep (tiny)** | "Share" = Copy Location, Send by Email (`mailto:` with attachment path via the user's mail app), KDE Connect only when installed. No cloud-account share. | Missing |
| F62 | Share with other computers (SMB share my folder) | **Drop** | Needs root/Samba config; AtlasOS rule "no privilege". | n/a |

### 2.4 Search

| ID | Feature | Verdict | Why and best-of design | Status |
|---|---|---|---|---|
| F70 | Search field: "Search Documents" (this folder and below) / "Search Everywhere" | **Keep / Improve** | One field in the toolbar; scope chips **This Folder** and **Everywhere** (home). Name matches from the index appear as you type (case/diacritic-insensitive, ranked); in non-indexed locations (external drives, remote) a live walk with a cancel/spinner. Esc returns to the folder. | **Done (W4)** (This Folder and Everywhere chips; the live walk for folders the index does not hold, with Stop) |
| F71 | Filters: kind, modified date, size | **Keep** | Chips under the field: Kind (Document, Image, ...), Modified (Today, Past 7 days, Past month, Past year, Custom), Size (Small, Medium, Large, Custom). Maps 1:1 to Search1 options. Folded into "Filters" button until used. | **Done (W4)** (Kind, Modified and Size chips, always shown while a search is open rather than folded into a Filters button; no Custom date or size yet) |
| F72 | Search results as a normal view (Details with Path column) | **Keep** | Same views, same actions, same Quick Look; "Open file location". | **Done (W4)** |
| F73 | Full-text content search | **Improve** | On demand, never indexed: "Search inside files" toggle runs a worker walk of the current folder tree (text, source, plain documents; PDF text through `pdftotext` by argv if present), shows snippets, skips binaries, can be cancelled, caps file size. Explains why in one line ("Searches the files in this folder; nothing is stored"). No background content indexer (Baloo complaint). | Missing (index design says no content indexing) |
| F74 | Regex filter / advanced query syntax | **Keep (folded)** | Filter bar and Search accept `re:` prefix in the Filters popover ("Use pattern (regular expression)") with an error line on invalid input; default plain words. Bounded runtime. | Missing |
| F75 | Search by tag | **Keep** | Click a tag in the sidebar or "tag:Red" chip. Needs F90. | Missing |
| F76 | Saved searches / smart folders | **Keep (small)** | "Save Search" turns the current query + filters + scope into a sidebar item under Favourites (JSON file in Files' data dir, not a `.savedSearch` in the user's folders). Rename/delete from its context menu. | Missing |
| F77 | Index status and control | **Keep** | Status chip in the search field area ("Ready", "Updating..."); Settings > Search: indexed folders, exclude list, Rebuild index button (calls `Refresh()`); "Turn off indexing" which makes search a live walk. | Partial (W4: the status chip, from Status and StatusChanged; no Settings > Search page, Rebuild button or "Turn off indexing" yet) |
| F78 | Spotlight-style suggestions, Siri Suggestions, web results | **Drop** | Cloud/profiling; not private. | n/a |

### 2.5 Tags and metadata

| ID | Feature | Verdict | Why and best-of design | Status |
|---|---|---|---|---|
| F90 | Colour tags (and named tags) | **Keep** | Stored in the `user.xdg.tags` xattr (freedesktop standard, Baloo-compatible); fall back with a clear message when the filesystem has no xattrs (FAT/exFAT, many network shares: "This location can't keep tags"). Context "Tags" submenu: toggle Red/Orange/.../Custom; dot(s) on the icon/row; a Tags column; sidebar Tags section lists tags in use (found by the index extension below). Seven colours + user-named tags. | Missing |
| F91 | Ratings (stars) | **Keep (folded)** | Stars in Properties and Details pane for images/audio via `user.baloo.rating` xattr (works with KDE apps). No separate column by default. | Missing |
| F92 | Comments | **Drop** | Rarely used by any of the three; Properties shows an existing `user.xdg.comment` read-only. | Partial (via KDE dialog) |
| F93 | Details columns from metadata (dimensions, duration, artist) | **Keep** | Add "Dimensions", "Duration", "Date taken" columns from KFileMetaData in a worker; off by default. | Missing |
| F94 | Backup/"versions" in Properties | **Keep (hook)** | "Restore Previous Versions" only when Telamon Backups is installed (activatable D-Bus name), per DESIGN. | Missing (waits for Backups) |

### 2.6 Network, cloud and devices

| ID | Feature | Verdict | Why and best-of design | Status |
|---|---|---|---|---|
| F100 | SFTP, SMB, FTP, WebDAV(S), NFS-as-mount | **Keep** | Through KIO; credentials in KDE wallet via KIO's password handling, never stored or logged by Files. Plain FTP shows "Not encrypted" warning; prefer SFTP/FTPS/WebDAVS. | Partial (KIO URLs open; no UI to connect) |
| F101 | Connect to Server... (Ctrl+Shift+K) with saved servers | **Keep / Improve** | One dialog: protocol, server, folder, user (password is asked by KIO). "Add to Sidebar" saves in Network. Recent servers list. Plain-words errors ("Can't reach the server. Check the address and that it is on."). | Missing |
| F102 | Network discovery (mDNS / SMB browse) | **Keep** | `network:/` through kio-extras (DNSSD/SMB); async list with spinner and Stop; empty state explains "No computers found. Use Connect to Server." | Partial (place only) |
| F103 | Responsiveness on dead/slow servers | **Improve** | See 2.0 item 1. Browsing a remote folder never blocks the window or other tabs; a 10 s no-answer state; Stop and Retry; closing the tab abandons the request. | Partial (async KIO; no Stop/timeout UI) |
| F104 | Removable media: detect, mount, unmount, eject, usage bars | **Keep** | Solid via `KFilePlacesModel`; toast "Drive ready" with Open; Eject also asks if a copy is running. | Partial (W3: detect, mount, unmount with "Safe to remove", usage bars; no "Drive ready" toast, no copy check) |
| F105 | Phones (MTP) and cameras (PTP) | **Keep** | `mtp:/` and `camera:/` through kio-extras as Drives entries; copy-off is a normal copy (progress + cancel). "Import photos" is just copy; no vendor apps. | Partial (W3: MTP phones listed under Drives; cameras only as far as KDE lists them) |
| F106 | OneDrive, iCloud Drive, Google Drive account integrations | **Drop** | Account-bound, proprietary and privacy-hostile; Telamon has no Microsoft/Apple accounts. Nextcloud and any WebDAV server cover self-hosted cloud (F100). | n/a |
| F107 | AirDrop, Handoff, Continuity camera, iPhone backup management | **Drop** | Apple-only protocols and ecosystem; KDE Connect covers phone transfer if installed. | n/a |
| F108 | CD/DVD burning, .dmg mounting | **Drop** | Optical burning is dead weight; `.dmg` is a macOS format. (ISO mounting through udisks may return later as a tiny "Mount Disk Image" item for `.iso`/`.img`.) | n/a |
| F109 | AFP | **Drop** | Deprecated by Apple itself. | n/a |
| F110 | Open in Disks (partitioning, drive details) | **Keep (hook)** | Shown only when Telamon Disks is installed. | **Done (W3)** (hidden until Disks is installed) |

### 2.7 Context menu and customisation

| ID | Feature | Verdict | Why and best-of design | Status |
|---|---|---|---|---|
| F120 | Short, grouped context menu, dividers, nested "More" submenu | **Improve** | See 2.0 item 3. Items in fixed order; icon row for the five everyday actions; "Open With" and "More Actions" submenus. Rendered with Telamon.Ui `ContextMenu` (not `QMenu`). | Partial (works, QMenu, long list) |
| F121 | Configurable context menu | **Keep (small)** | Settings > Context Menu: a list of switches for the built-in entries (Open Terminal Here, Copy Path, Compress, Share, Quick actions...) and for installed service menus (each KDE service menu can be hidden). No drag-ordering in v1. | Missing |
| F122 | Custom actions (user commands) | **Keep (safe)** | Settings > Actions: name, command *program + arguments* (no shell), placeholders `%f %F %u %d` (KDE's own set), file type filter, "ask first" checkbox. Stored as a `.desktop` service menu in `~/.local/share/kio/servicemenus` so Dolphin-compatible. No shell strings: a command line is split into argv by a quoted-word parser and run through `QProcess` with an argument list. | Partial (service menus load; no editor) |
| F123 | Plugin system (e.g. Git status) | **Drop (third-party API)** / **Keep (Git badges)** | A public plugin API is a security and maintenance burden. Service menus already extend the menu. Keep one built-in optional feature: Git status overlays (modified/untracked badges) using `git status --porcelain=v1 -z` by argv with a timeout, only inside a repo, off by default. | Missing |
| F124 | Context menu that never shifts | **Improve** | Service-menu items are loaded before the menu appears (or in the fixed "More Actions" submenu), so nothing inserts afterwards. | Partial (loads synchronously) |
| F125 | Keyboard menu key / Shift+F10 | **Keep** | Opens the same menu at the focused row. | Missing |
| F126 | Copilot / AI hover actions, Microsoft 365 persona cards, "ask Copilot", voice-typing rename | **Drop** | Proprietary services and profiling; not Telamon. | n/a |
| F127 | Automator, Shortcuts, Quick Actions workflows | **Drop** | Apple automation; custom actions (F122) and service menus cover simple needs. | n/a |
| F128 | Backup reminder nags, tips banners, "try OneDrive", sync badges | **Drop** | Nags are the top Windows complaint; no sync clients here. | n/a |
| F129 | Portable mode | **Drop** | Not needed: Files is a system app with its own settings. | n/a |

### 2.8 System integration

| ID | Feature | Verdict | Why and best-of design | Status |
|---|---|---|---|---|
| F140 | `org.freedesktop.FileManager1`, launch args, single instance | **Keep** | "Show in folder" from every app. | Done |
| F141 | Atlas/Telamon app hooks (Archive, Backups, Disks, Launcher via Search1) | **Keep** | Listed in DESIGN "Consumed interfaces". | Partial (Search1 done; others Missing) |
| F142 | Default for `inode/directory`, desktop file, icon | **Keep** | | Done |
| F143 | Settings | **Keep (small)** | One dialog, four sections: General (start page, hidden files, confirmations), View (folders first, one view for all, previews on remote), Search (indexed folders, rebuild), Context Menu and Actions, Trash (auto-empty). That is all. | Missing |
| F144 | Executable / `.desktop` trust prompts | **Keep** | "Run or open?" default Open. | Done (KIO) |

Count check: the owner's 117 items fold into the rows above; items not named here were merged into the nearest
row (for example "up arrow" and "Alt+Up" into F5, "hide extensions" dropped into F32, "sort by tag" into F90).

### 2.9 Dropped groups, and why

| Group | Dropped items | Why |
|---|---|---|
| Vendor cloud accounts | OneDrive, iCloud Drive, Google Drive, file-on-demand placeholders | Need Microsoft/Apple/Google accounts; privacy; no sync client. WebDAV/SFTP cover self-hosting. |
| Apple ecosystem | AirDrop, Handoff, Continuity, iPhone backup management, .dmg mounting, AFP, Finder tags *as an Apple database* (replaced by xattr tags) | Apple-only protocols or formats. |
| Microsoft services | Copilot and AI hover actions, Microsoft 365 persona cards, "Recommended" in Home, Gallery's OneDrive Camera Roll tie, Windows libraries and drive-letter mapping | Account- and ad-driven. |
| Automation | Automator, Shortcuts, Quick Actions builders, voice-typing rename | Platform-specific; custom actions + service menus replace them. |
| Disc features | CD/DVD burning | Obsolete. |
| Nags and ads | Backup reminders, tip banners, upsells | Top Windows complaint. |
| UI weight | Folder tree panel, Cover Flow, desktop stacks, icon clean-up, portable mode, embedded terminal panel (for now), comments, third-party plugin API, share-my-folder via Samba | Duplicates something simpler, depends on a widget stack, needs privilege, or is rarely used. |

---

## 3. Build plan in waves

Each wave is a pull request of a few hours' work that builds and tests in the dev container (`scripts/dev.sh`, see
`CLAUDE.md`) and ends with the app running offscreen. Every wave: file names shown through `display_name` and
`textFormat: Text.PlainText`; no shell strings; every destructive action confirmed; no files outside the generated
test trees touched by tests; update `docs/DESIGN.md` together with the code. Order is by value to everyday users and
by dependency. "Needs" = hard dependency.

### Wave 1: Tabs (F1, F2, part of F5)
Needs: nothing. Refactor `Main.qml`'s single `FolderView` into a per-tab component (own URL, back/forward stack,
view mode, selection).
- Ctrl+T opens a tab at Home; Ctrl+W closes it; closing the last tab closes the window. Ctrl+Tab / Ctrl+Shift+Tab / Alt+1..9 switch.
- Ctrl+Shift+T reopens the last closed tab (up to 10).
- Middle-click or Ctrl+click on a folder, a sidebar place or a breadcrumb segment opens it in a new background tab.
- Drag a tab to reorder it; drag files onto a tab moves them into that tab's folder (confirm only if overwriting, via the conflict dialog).
- Mouse back/forward side buttons and Alt+Left/Right work per tab.
- Launch `--select`, FileManager1 ShowFolders open new tabs of the active window (as DESIGN says).
- Tab title is the folder name (plain text), tooltip shows the full path.

### Wave 2: Breadcrumb address bar, history, status bar (F4, F5, F8, F11)
Needs: Wave 1 (per-tab history).
- The path shows as clickable segments with chevrons; clicking a segment goes there; the chevron opens a menu of that folder's subfolders (lazy, off the UI thread).
- Clicking the empty part, Ctrl+L, Alt+D or F4 turns the bar into a text field with the full path; Enter goes; Esc returns; typing a partial folder name offers completions (Tab accepts) from a worker.
- Dropping files on a segment moves them there (Ctrl copies), with the drag cursor saying Move/Copy.
- Long-press (or right-click) on Back/Forward lists the last 10 places; picking one jumps there.
- A footer shows "12 items, 3 selected (4.2 MB), 128 GB free" (and "5 hidden"), updating as selection changes; all plain text.
- Bad paths/URLs show the reason in plain words under the bar (existing `parseAddress`).

### Wave 3: Sidebar with drives, pins and trash (F6, F12, F104, F105, F110)
Needs: nothing (parallel with 1-2 if sharing no files; otherwise after Wave 2).
- Sidebar is built from `KFilePlacesModel`; pinning writes `user-places.xbel`, so pins appear in Open/Save dialogs.
- Drag a folder onto the sidebar to pin it; drag pins to reorder; right-click a pin: Rename, Hide, Remove from Sidebar.
- Plugging in a USB drive adds it under Drives with a usage bar; click mounts and opens; the eject button unmounts and says "Safe to remove".
- A phone (MTP) or camera appears under Drives; opening it lists photos; copying works like any copy.
- Trash shows an item count; right-click Trash offers Empty Trash with a confirmation that names the size.
- "Open in Disks" appears for drives only when Telamon Disks is installed.

### Wave 4: Search in the toolbar (F70, F71, F72, F57, F77)
Needs: Wave 1-2 for a good results view; the index service exists. Uses `Search1` over D-Bus (async, drop stale replies).
- Typing in the search box shows ranked name matches within 100 ms for the indexed home; scope chips **This Folder** / **Everywhere**.
- Results are a normal Details view with a Path column; Enter opens; "Open File Location" (Ctrl+Enter) opens the parent with the file selected.
- Filter chips: Kind (Document, Image, Audio, Video, Archive, Code, Folder), Modified (Today, Past 7 days, Past month, Past year), Size (Small/Medium/Large); combine freely; each chip can be cleared.
- Outside the index (external drives, remote) a live walk streams hits with a Stop button.
- A status chip shows index state ("Updating the search index, results may be missing"); Esc clears the search and returns to the folder.
- Works while the indexer is not running (D-Bus activation) and shows "Search isn't available" in plain words when it fails.

### Wave 5: Quick Look and the preview/details pane (F29, F30, F27)
Needs: nothing.
- Space opens a Quick Look overlay for the selected file (image, PDF, video, audio, font, office via KIO thumbnailers; text/code as plain text, first 256 KB); Space or Esc closes; arrow keys move through the folder selection/files; Enter opens in the default app.
- Works from search results and the Trash too.
- Alt+P shows a right-hand preview pane (large thumbnail or text or a media player) with name, kind, size, dates and, where known, dimensions/duration; off by default.
- Ctrl+scroll (and Ctrl+plus/minus, Ctrl+0) change icon size/row height within limits; the size is remembered.

### Wave 6: Operations queue, conflict dialog, undo/redo (F40, F46, F47, F48)
Needs: nothing, but easier after Wave 1. Wires the existing core `OperationQueue`, journal and `undo`.
- All copy/move/trash/delete/rename/new-folder run through one queue; a toolbar ring shows progress; its popover lists each job with speed, time left, Pause/Resume, Cancel.
- Name conflicts open a Telamon.Ui dialog with both files side by side (thumbnail, size, date, "Newer"), Replace / Skip / Keep Both (editable name) / "Do this for all conflicts"; folders get Merge/Skip.
- Cut items are dimmed until pasted, and the toolbar says "2 items waiting to move".
- Ctrl+Z undoes the last operation with a toast naming it ("Undid: Move 3 Items to Backup"); Ctrl+Shift+Z / Ctrl+Y redoes; permanent delete states it can't be undone.
- Moving a folder into itself, or onto a full disk, is refused up front with the numbers in plain words.

### Wave 7: Context menu rebuild and "More Actions" (F120, F121, F124, F125, F53, F56, F57)
Needs: Wave 6 optional. Replaces the `QMenu` with Telamon.Ui `ContextMenu` backed by a model built from `KFileItemActions`.
- Right-click (or the Menu key / Shift+F10) shows: Open, Open With, an icon row (Cut, Copy, Paste, Rename, Trash), Compress (when Archive exists), Properties, and a "More Actions" submenu.
- The menu never changes after it appears: service-menu items are ready before it shows or live inside More Actions.
- More Actions holds Copy Path (also Ctrl+Shift+C), Hide, Open File Location, Open Terminal Here, Open in New Tab/Window, Pin to Sidebar and installed service menus.
- Background menu: New (Folder, Text File, templates), Paste, Sort, View, Open Terminal Here.
- Settings (Wave 14) later toggles entries on/off; here the entries simply exist.

### Wave 8: Views: Columns, Gallery, per-folder memory, grouping (F21, F22, F25, F26)
Needs: Waves 1 and 5 (Quick Look/preview reused).
- View menu gains Columns and Gallery; Columns opens a new column on folder select, right arrow enters, left goes back, last column shows a preview, Space is Quick Look.
- Gallery shows a large preview with a filmstrip; arrow keys move.
- Each folder remembers its view, sort and icon size, in Files' settings file (a bounded list), never in the folder; the View menu has "Use the same view for every folder" and "Reset this folder's view".
- Sort menu has Group by (None, Name, Type, Date Modified) with collapsible headers.

### Wave 9: Archives (F50, F51)
Needs: Telamon Archive's `Archive1` API available (otherwise stub and skip F50); F51 uses kio-extras `zip:/`.
- Right-click an archive: Extract Here, Extract To...; right-click files: Compress to ZIP, Compress...; the jobs appear in the operations popover and the result is selected when done.
- Items are hidden when Archive isn't installed.
- Opening a `.zip` or `.tar.gz` shows its contents as a read-only folder with an "Extract" button; dragging entries out extracts them.
- Errors ("Archive is busy, try again when a job finishes") are shown in plain words.

### Wave 10: Rename polish and batch rename (F41, F42, F43)
Needs: Wave 6 (one undo step).
- F2 renames in place with the base name selected; invalid names (`/`, only dots) are refused in plain words, control/bidi characters get a warning; after renaming the item stays selected and in view.
- Selecting several files and pressing F2 opens Batch Rename: Find and Replace, Add Number, Change Case, Add Text; a live before/after list; conflicts and bad names are flagged and block Apply; Apply is one undoable step.
- The New menu offers Folder, Text File and `~/Templates` entries; the new name is in edit mode.

### Wave 11: Home page, Connect to Server, network (F9, F100-F103)
Needs: Wave 3 (sidebar).
- Home shows Pinned, Recent files and Frequent folders (all collapsible; nothing recommended, nothing from the cloud).
- Ctrl+Shift+K opens Connect to Server (protocol, server, folder, user); the password is asked by KIO; "Add to Sidebar" saves it; recents are listed.
- Network lists discovered computers with a spinner and Stop; an unreachable server shows "Can't reach the server" with Retry after 10 s; the window and other tabs stay usable.
- Remote folders show icons only (no thumbnails/MIME sniff/folder sizes) unless the Settings switch is on.
- FTP shows "Not encrypted"; SFTP/WebDAVS don't.

### Wave 12: Trash tools and auto-empty (F44, F45)
Needs: Wave 3 (count) and Wave 6.
- In Trash, the toolbar shows Restore and Empty Trash; columns show Original Location and Date Deleted; Restore puts files back (conflict dialog if needed).
- Empty Trash is red and confirms ("Delete 120 items (4.3 GB) for good? This can't be undone.", default Cancel).
- Settings switch "Empty items older than [N] days" (off by default, 30 suggested); when on, Files removes only expired items at start and daily, and logs the count.

### Wave 13: Tags and metadata (F90, F91, F93, F60)
Needs: Waves 4 and 7 (search chip and menu).
- Right-click > Tags toggles colour/named tags (xattr `user.xdg.tags`); colour dots show on the file; a Tags column is available.
- The sidebar Tags section lists tags in use; clicking one shows all tagged files (via the index/live walk).
- On filesystems without xattrs it says "This location can't keep tags".
- Properties (Telamon.Ui, replacing KPropertiesDialog) shows General, Permissions, Details, Checksums (SHA-256 on demand, cancellable), Open With default, tags and star rating; folder size is computed only on request.
- Optional Dimensions/Duration/Date-taken columns.

### Wave 14: Split view and quick actions (F3, F49 extras, F58)
Needs: Waves 1, 2, 6.
- F3 splits the tab; the active pane has an accent line; drag and drop between panes; "Copy to Other Pane"/"Move to Other Pane" (F5/F6 while split); closing the inactive pane is one click.
- Spring-loaded folders open after 1 s of hover; drags show Move/Copy/Link badges.
- Quick actions for images (More Actions): Rotate Left/Right, Convert to PNG/JPEG/WebP, Combine into PDF (images or PDFs); writes new files and never overwrites without the conflict dialog; each step is undoable.

### Wave 15: Content search, saved searches, filter bar (F33, F73, F74, F76)
Needs: Wave 4.
- Ctrl+F in a folder filters the visible items as you type, with a visible "Showing 4 of 120, Clear" chip.
- "Search inside files" runs a cancellable worker search of the chosen folder tree (text files; PDFs if `pdftotext` exists) with matched-line snippets; nothing is stored.
- A Filters popover has "Use pattern (regular expression)"; invalid patterns show an error line.
- "Save Search" adds the query + scope + filters to the sidebar; it can be renamed or removed.

### Wave 16: Settings, custom actions, accessibility pass (F15, F121, F122, F143, F123 Git badges)
Needs: Wave 7 (menu), others for content.
- One Settings window with five short sections (General, View, Search, Context Menu and Actions, Trash); every control has a one-line plain description.
- The user can add an action (name, program, arguments, file types, "ask first"); it appears under More Actions and runs by argument list, never through a shell.
- Every service menu and built-in menu entry can be hidden.
- Optional Git status badges (off by default; `git` run by argument list with a timeout).
- Accessibility: Orca reads each row's name, type, size and selected state; every feature is reachable with the keyboard alone; reduced-motion and high-contrast respected; RTL checked.

### Wave status
| Wave | Status |
|---|---|
| W1 Tabs | **Done** (PR `files/w1-tabs`). Deviations: Alt+1..9 only (Ctrl+1..9 left for view modes); Ctrl+click on a folder row stays multi-select (middle click, Ctrl+Enter and "Open in New Tab" open it in a tab); a file dropped on a tab uses KIO's drop menu (Move, Copy, Link) instead of a silent move; closed tabs keep URL, view mode and history but not selection or scroll; no breadcrumb segments yet (wave 2), so none to middle-click; dragging a tab out into its own window waits for multiple windows. Added: tab context menu, Duplicate Tab, opt-in Restore Tabs on Start, 64-tab limit. |
| W2 Breadcrumb, history, status bar | **Done** (PR `files/w2-breadcrumb`). Deviations: a drop on a segment moves or copies with no menu as specified, but a drop on a *tab* still uses KIO's drop menu (W1); the drag cursor is set through the drop action (Move, or Copy with Ctrl held) and a small "Move to X" or "Copy to X" label shows under the segment, since a cursor can't be checked headless; free space is shown for local disks only (a server, Trash and Recent show none); sizes read "MiB/GiB" like the Size column, not "MB/GB"; "N hidden" is shown only while hidden files are off, after the item count; Esc always returns to the segments (the completion list closes with it); F6 still edits the address beside Ctrl+L, Alt+D and F4; the home folder is one segment "Home" so the path above it is reached with Up or by typing; the path bar and its completion list are Files' own (`qml/PathBar.qml`) because Telamon.Ui's `TelamonBreadcrumb` and `TelamonAutocompleteField` lack what F4 needs. Added: a "..." menu for segments that don't fit, Ctrl+click on a segment or a subfolder, "N more not shown" in a long subfolder menu. |
| W3 Sidebar | **Done** (PR `files/w3-sidebar`). Deviations: the camera half of F105 is only what KDE lists (PTP cameras that libmtp serves show as phones; a gphoto2-only camera is not listed by `KFilePlacesModel` and there is no `camera:` worker); a folder dropped on any place is pinned rather than moved into it (files dropped on a folder place move there; anything dropped on the Trash is trashed, which the spec did not ask for); no "Drive ready" toast with Open when a drive is plugged in, and Eject does not ask about a running copy (there is no operation queue yet, wave 6); Recent follows the standard folders (KFilePlacesModel groups it apart); "Show Hidden Places" is an entry under the list, not a menu on the sidebar background; Open in Disks assumes Disks' D-Bus names; MTP browsing could not be tried without a phone (the places, URL and copy path are KIO's). Added: Open in New Tab for every place, "Pin to Sidebar" in the folder and background context menus. |
| W4 Search | **Done** (PR `files/w4-search`). Deviations: the chips are a row under the toolbar that shows while there are words or chips or the field has the keyboard, not folded into a "Filters" button; Modified and Size have no Custom choice yet (Size is Small under 1 MiB, Medium to 100 MiB, Large over); Ctrl+F and Ctrl+E focus the field (Ctrl+F is promised to the filter bar of wave 15, to be moved then); This Folder asks the index only when the folder is below an indexed root, on its disk, and not in a folder the scanner leaves out, else it is a live walk, and a name added to `Exclude=` in `indexrc` is not known to Files; a live walk of a folder on this computer is `atlas_file_index::walk` on a thread (breadth first, no symlinks, one filesystem), one of a server, the Trash or an archive is `KIO::listRecursive`, and only the Trash was tried (no server in the test setup); both stop at 5,000 hits, the index at 500; results are always shown as Details while searching; New Folder and Paste are off while results show; "Open File Location" shows the parent in the same tab (results in other folders open in tabs behind it) where `FileManager1.ShowItems` opens one tab per parent; if the index can't be reached a search that needs it says "Search Isn't Available" (no fallback walk of the home folder), a walk of another folder still works. Added: the "Best Match" sort, a Try Again button, the results are repeated when the index finishes its scan, and rows whose files were trashed, moved or renamed are dropped. |
| W5 Quick Look, preview pane, zoom | **Done** (PR `files/w5-quicklook`). Deviations: Quick Look and the pane preview only files on this computer (a server's file shows its icon and details; thumbnails and reads are local, as in DESIGN); fonts have a preview only where a KIO font thumbnailer is installed (else the icon), and a PDF needs `gsthumbnail` with Ghostscript; dimensions are read for pictures (header) and videos (from the player), a PDF's page count is not shown; a length and a video's size appear once the player has opened the file (it is opened, not played); arrows stop at the ends and, with two files or more selected, walk those files starting at the one with the cursor; the preview pane is one pane with the details inside it, 20 grid units wide and not resizable (Alt+Shift+P and a separate details pane wait for later); the sizes are one setting for all tabs (per-folder memory is wave 8) and only icons and row height scale, not the font. Added: View menu entries Preview Pane, Zoom In, Zoom Out, Reset Zoom; Qt Multimedia (`qt6-qtmultimedia`) for the player, in the spec's Requires and BuildRequires; `ffmpeg-free` and `ghostscript` in the dev image (test media and PDF thumbnails). Fixed from wave 4: search results for files on this computer had no name (KFileItem takes its name from the entry only). |
| W6 Operations queue, conflict dialog, undo/redo | **Done** (PR `files/w6-operations`). Deviations: there is no operation journal in the core (DESIGN described one; crash recovery is the Reliable phase), so nothing is written to disk; a copy, move or paste that *replaced or merged* anything (Replace, Merge) is not undoable: its result holds files that were there before, so it empties both lists and says so (trashing the replaced file first would make it undoable, not done); undo is never a delete: it trashes copies, moves back, restores from the Trash, or `rmdir`s an empty new folder, and a stale entry (files changed since) is refused in words and dropped; drops on a tab or on the folder view no longer use KIO's DropJob menu: Shift moves, Ctrl copies, Ctrl+Shift links, no key shows a Move Here / Copy Here / Link Here menu; Delete for good asks in a Telamon.Ui dialog (Cancel is the default); the room check is local-only (a server has no honest free-space answer) and runs when the operation starts, so a queued copy is checked against the space left by the one before it; text and image paste still ask for a name in KIO's own dialog (it runs through the queue); the Rename and New Folder name prompts are still Qt widgets (wave 7); Undo and Redo are in the background context menu with their names, the "last 20" list is not shown anywhere yet; Eject still does not wait for the queue. Added: the "1 item waiting to move" line, Empty Trash through the queue, a problem dialog (Retry, Skip, Skip All, Cancel) for what a job asks, and KIO's "delete instead of Trash?" question in a Telamon.Ui dialog. |
| W7 to W16 | Not started. |

### Later (not scheduled)
Mount `.iso`/`.img` through udisks; Share menu (F61); embedded terminal panel (F55); restore previous versions (F94, when Backups exists); selection mode (F31).

### Dependencies at a glance
W1 -> W2 -> W4 -> W13/W15; W3 -> W11, W12; W5 -> W8; W6 -> W10, W12, W14; W7 -> W13, W16.

---

## 4. Sources

Finder
- https://sindresorhus.com/command-x
- https://forums.macrumors.com/threads/there-is-no-cut-option-in-finder-app.2407806/
- https://discussions.apple.com/thread/254648745
- https://forums.macrumors.com/threads/i-cant-stand-finder-user-for-around-5-months.1203127/
- https://forums.macrumors.com/threads/1281546
- https://discuss.binaryage.com/t/can-tf-fix-broken-finder-arrange-sort/1718
- https://www.idownloadblog.com/2015/12/16/how-to-finder-path-bar-mac/
- https://www.macworld.com/article/220947/five-overlooked-abilities-of-the-finders-path-bar.html
- https://macmost.com/viewing-file-contents-with-quick-look.html
- https://macmost.com/use-quick-look-instead-of-preview-to-view-files.html
- https://macmost.com/21-actually-useful-finder-tips-and-tricks.html
- https://www.oreilly.com/library/view/macos-sierra-the/9781491977224/ch02s10.html
- https://kartick.substack.com/p/simplifying-and-modernising-the-finder-16-04-09
- https://support.studionetworksolutions.com/hc/en-us/articles/360000938163-Folder-contents-slow-to-display-Mac-SMB
- https://forums.truenas.com/t/macos-not-finding-smb-shares/67027

Windows 11 File Explorer
- https://blogs.windows.com/windows-insider/2026/08/17/improving-file-explorer-context-menu-faster-simpler-and-more-customizable/
- https://windowslatest.com/2026/08/19/microsoft-admits-it-made-windows-11-worse-than-windows-10-for-right-click-menus-promises-to-fix-sluggish-and-cluttered-ux
- https://windowslatest.com/2021/11/02/windows-11-is-hit-by-slow-explorer-content-menu-bug-but-a-fix-is-coming
- https://windowslatest.com/2022/03/10/hands-on-with-windows-11s-leaked-tabs-feature-for-file-explorer
- https://www.techradar.com/computing/windows/finding-windows-11-sluggish-when-youre-working-with-files-and-folders-youre-not-alone-and-its-high-time-for-microsoft-to-sort-out-file-explorer
- https://www.ghacks.net/?p=179162
- https://winaero.com/you-now-drag-tabs-out-of-file-explorer-to-open-them-in-a-new-window/?amp
- https://www.bleepingcomputer.com/news/microsoft/microsoft-is-testing-ads-in-the-windows-11-file-explorer
- https://www.laptopmag.com/news/windows-11-shows-ads-in-file-explorer-microsoft-tests-promoting-its-own-products
- https://tomsguide.com/news/please-dont-do-this-to-windows-11-microsoft
- https://windowsreport.com/file-explorer-is-not-the-fastest-way-to-search-on-windows-try-these-alternative-tools-instead/
- https://www.neowin.net/news/windows-11-dev-build-23435-brings-new-file-explorer-gallery-but-its-quite-buggy/
- https://betanews.com/2023/04/14/windows-11-gallery-file-explorer/

Dolphin / KDE
- https://www.itpro.com/software/linux/359977/best-linux-file-managers
- https://community.linuxmint.com/software/view/org.kde.dolphin
- https://linuxiac.com/kde-gear-25-04-apps-collection-released/
- https://bugs.kde.org/show_bug.cgi?id=336910
- https://forum.kde.org/dolphin-ideas-with-mockups-t-28472-2.html
- https://discuss.kde.org/t/dolphin-per-directory-settings-revisited/10288
- https://dev.to/svhl/dolphin-switches-from-directory-to-extended-attributes-jpj
- https://bugs.kde.org/show_bug.cgi?id=215953
- https://mail.kde.org/pipermail/kfm-devel/2025-June/057336.html
- https://mail.kde.org/pipermail/kfm-devel/2025-May/057229.html
- https://discuss.kde.org/t/samba-share-on-dolphin-is-sluggish/49507
- https://bugs.kde.org/show_bug.cgi?id=334397
- https://bbs.archlinux.org/viewtopic.php?id=231709
- https://lists.ubuntu.com/archives/kubuntu-bugs/2025-August/168814.html
- https://bugs.kde.org/show_bug.cgi?id=333652
- https://mail.kde.org/pipermail/kfm-devel/2021-June/039222.html

Repo: `README.md`, `CLAUDE.md`, `docs/DESIGN.md`, `apps/telamon-explorer/{qml,cpp/kio,src}`,
`crates/atlas-explorer-core`, `crates/atlas-file-index`.
