# Telamon Explorer: design

What this file fixes: the layout, the threading rule, what is trusted, the
APIs Explorer exports and consumes, who owns what, the failure modes and the
budgets. Change it together with the code that changes them. The full plan
and its reasons are the Atlas Notes note "AtlasOS/Explorer/Plan"; the
checklist is "AtlasOS/Explorer/Roadmap".

App ID `net.eterneon.telamon.explorer`, binary `telamon-explorer`, shown name
**Files** (the name the image already gives Dolphin). Rust + Qt 6.11 Quick +
Kirigami over CXX-Qt, Telamon.Ui from the installed `telamon-ui`, KF6 6.30 (KIO,
Solid, KService, KCoreAddons, KDBusAddons, KWindowSystem).

## Status

This file describes the finished Files. The code is smaller, and
`docs/ROADMAP.md` lists what is built and what is planned, wave by wave. As of
0.2.0 plus waves 1 to 3 (tabs; the path bar, history menus and status line; the sidebar with pins, drives and the Trash)
only these parts of the sections below exist: a tab strip with one folder per
tab (Details, Icons and Compact views), a breadcrumb path bar that becomes a
text field with completion, a status line, the command bar's New Folder, Cut, Copy, Paste, Rename and Move to Trash
with View and Sort menus, a sidebar of KIO places (pins, drives, phones, the Trash), KIO jobs with KIO's own
dialogs, `FileManager1`, the launch parser and the index service. Everything
else (search field, preview and details panes, Quick
Look, the operations popover and queue wiring, Columns and Gallery
views, split view) is design, not behaviour. Sections that
have been built say so in a "Built" line.

## Scope

Explorer replaces Dolphin completely.

| Dolphin today on Telamon OS | Who covers it |
|---|---|
| Browse any KIO URL (file, smb, sftp, ftp, webdav, mtp, trash, recentlyused, network, archives through kio-extras) | Explorer, through KIO |
| Places panel (`user-places.xbel`), devices, mount, unmount, eject | Explorer: KFilePlacesModel and Solid |
| Tabs, split view, icons, compact, details views, previews, info panel | Explorer: tabs, split view, icons, list, details, columns, gallery, preview pane, details pane |
| Copy, move, link, rename, trash, delete, undo, conflict dialog | Explorer's operation queue, on KIO jobs |
| Context menu: Open With, service menus, "Open Terminal Here", Shift+F4 | Explorer: KFileItemActions, KTerminalLauncherJob (Ghostty through kdeglobals) |
| Search (Baloo, filenamesearch) | Explorer: the file index (below) and a live walk |
| Properties dialog | Explorer, with permissions, default app and checksums |
| `org.freedesktop.FileManager1` ("Show in folder") | Explorer |
| Default for `inode/directory` | Explorer (the image's `mimeapps.list`) |
| Archive extract and compress | **Atlas Archive** over its D-Bus API; Explorer only calls it |
| Drive details, partitioning | **Atlas Disks** ("Open in Disks" hook) |
| Previous versions | **Atlas Backups** ("Restore Previous Versions" hook) |
| Baloo's indexer (already removed in the image) | Explorer's index service, used by Atlas Launcher |

## Layout

- `crates/atlas-explorer-core`: no Qt, no KF6. Display names from untrusted
  file names, natural sort keys and sorting, kinds from MIME and extension,
  the operation queue's state machine (order, pause, speed and time left,
  conflict policy), the operation journal (crash recovery), launch and D-Bus
  argument parsing, address-bar parsing and completion ranking, checksums,
  the live search walker and search filters.
- `crates/atlas-file-index`: no Qt. The index (scanner, inotify watcher,
  on-disk snapshot, matcher and ranking) as a library, so tests and the
  service share it.
- `apps/telamon-explorer`: the app.
  - `src/*.rs`: CXX-Qt QObjects (app state, settings, search, operation
    queue front, index client).
  - `cpp/kio/*`: thin C++ adapters over KF6, which only C++ can call:
    `FolderModel` (KCoreDirLister into a list model), `ThumbnailProvider`
    (KIO::PreviewJob), `JobRunner` (KIO jobs for the queue), `UiDelegate`
    (KIO's AskUserAction, OpenOrExecute and UntrustedProgram interfaces,
    answered by QML dialogs), `ItemActions` (KFileItemActions to a QML menu
    model), `Places` (KFilePlacesModel and Solid), `Opener`
    (KIO::OpenUrlJob, KTerminalLauncherJob), `DragHelper` (QDrag).
    Heavy or pure logic is called from these through a `cxx` bridge into the
    core crate, never written twice.
  - `cpp/main.cpp`: Qt start, telamon-framework-ui startup, single instance,
    FileManager1.
  - `qml/`: the window, views and dialogs, all from Telamon.Ui.
- `apps/telamon-explorer-indexd`: the index service binary (zbus), plus its
  systemd user unit and D-Bus activation file.
- `apps/telamon-explorer-search`: the index CLI.

## Window

- Title bar with Windows 11 caption buttons (the Telamon OS decoration), then
  the **tab strip**: closable, reorderable by drag, drop files on a tab to
  move them into it, middle-click closes, Ctrl+T, Ctrl+W, Ctrl+Shift+T
  reopens, Ctrl+Tab cycles, a folder dragged out of the window opens there.

  **Built (wave 1):** the strip is Telamon.Ui's `TabBar` over a `ListModel`
  in `qml/Main.qml`; each tab is a `qml/FilesTabPage.qml` (own folder, back
  and forward history, view mode, selection and scroll position, because the
  page and its `FolderView` stay alive while the tab is hidden). The decisions
  that are not drawing live in `atlas_explorer_core::tabs` (index arithmetic
  after close, move and cycle, Alt+1..9, where an opened tab goes, limits,
  checking a saved session) behind a C ABI (`src/ffi.rs`) and the `TabLogic`
  QML singleton (`cpp/kio/TabLogic.*`), which also keeps the settings.
  - New tab (Ctrl+T, the + button) opens Home at the end of the strip. A tab
    opened from another (middle click on a folder row or a sidebar place,
    Ctrl+click or Ctrl+Enter on a sidebar place, Ctrl+Enter on selected
    folders, "Open in New Tab" in the context menu) opens behind the current
    tab, in click order, without switching; its title shows a small dot until
    it is shown. Ctrl+click on a folder row stays multi-select.
  - Ctrl+W, the close button or a middle click on the tab closes it; the tab
    to its right (else left) is shown. Closing the last tab closes the window.
    Ctrl+Shift+T reopens the last 10 closed tabs, newest first, in their old
    place, with their back and forward history (50 entries each) and view mode;
    selection and scroll position are not kept for a closed tab.
  - Ctrl+Tab and Ctrl+PgDown go to the next tab, Ctrl+Shift+Tab and Ctrl+PgUp
    to the previous (wrapping); Alt+1..8 the nth tab, Alt+9 the last. (Ctrl+1..9
    stay free for view modes.) At most 64 tabs per window, shown as a plain
    message beyond that.
  - Tab title: the folder's name (Home, Trash, Recent, Network by their own
    names) made safe to show; the tooltip is the full location.
  - A tab's right-click menu: New Tab, Duplicate Tab, Close Tab, Close Other
    Tabs, Close Tabs to the Right, Reopen Closed Tab. The strip's "..." menu has
    New Tab, Reopen Closed Tab and **Restore Tabs on Start** (off by default).
  - Back and Forward are per tab: the buttons, Alt+Left, Alt+Right and the
    mouse side buttons.
  - Files dragged onto a tab go to that tab's folder through the same KIO drop
    menu as a drop on a folder (Move Here, Copy Here, Link Here); holding the
    drag over another tab for 0.8 s shows it first.
  - **Restore Tabs on Start:** with the setting on, the window keeps its tabs'
    locations and the shown tab in `telamon-explorerrc` (`[Tabs]`, written
    half a second after a change and at close, the last tab included), and the
    next start without a location to open brings them back; only the tab shown
    lists its folder at once, the others when first shown. The saved list is
    read through the launch parser (`tabs::restore`), so a hand-edited file
    can't open a kind of location a launch would refuse. Turning it off deletes
    what was kept.
  - **Framework gaps** (for the Telamon OS Framework session): Telamon.Ui's
    `TabBar` takes no drops and has no way to find the tab under a point, so
    Files overlays a `DropArea` and walks the bar's items for the delegate
    (`qml/Main.qml`, `tabAt`); it also has no "new content" marker, so the
    background-tab dot reuses `modified`, which a screen reader announces as
    "modified". It has no drag-out to a new window either (needs the multiple
    windows of a later wave).
- **Toolbar row:** Back, Forward, Up, Refresh; the **address bar**; the
  **search field** ("Search Documents").
  - Address bar: a Telamon breadcrumb (segments with chevrons; a chevron opens
    the subfolder menu; drop files on a segment). A click on the empty part,
    Ctrl+L, F4 or Alt+D turns it into a text field holding the URL or path,
    with completion of folder names (local: a worker lists the folder; remote:
    a KIO listDir with a timeout), `~` and environment-free shortcuts
    (`~`, `trash:`, `recent:`, `network:`), Enter goes, Esc returns to the
    breadcrumb.

  **Built (wave 2):** `qml/PathBar.qml`, with the decisions in the core
  (`atlas_explorer_core::location`, `address`) behind the C ABI and the
  `LocationLogic` QML singleton (`cpp/kio/LocationLogic.*`).
  - Segments come from the core's `location::segments`: the home folder is
    one segment "Home" (Home > Documents > Reports), the rest of the local disk
    starts at "Root", `trash:`, `recentlyused:` and `network:` at Trash,
    Recent and Network, a server at the server's name. Labels are display names
    (control and bidi characters made visible); the links keep the real,
    percent-encoded bytes and never show a password. A click goes there (Ctrl+click
    or a middle click opens a background tab). When the path is wider than the
    bar the first segments collapse into a "..." menu, the last always stays.
  - Every segment, the last included, has a chevron: its menu lists the
    folder's subfolders in natural order, hidden ones only when Show Hidden is
    on, at most 100 ("N more not shown"). The menu opens at once with
    "Loading..." and fills when the list arrives. A local folder is read by a
    worker (`QDirListing`, at most 20,000 names); a folder on a server, Trash
    and so on by a `KIO::listDir` job that is stopped after 8 s with "The
    server isn't answering." and cut off at 5,000 names. A second request stops
    the first, and an answer to an old request is dropped (each carries a
    number).
  - Editing: a click on the empty part, Ctrl+L, F4, F6 or Alt+D. The field
    holds the path of a local folder, else the URL without its password; a
    path with control or bidi characters is shown as a URL, so nothing hides.
    Enter goes (the core's `parseAddress`; a refusal is plain words in red under
    the bar and the field stays), Esc or leaving the field returns. Typing
    asks `LocationLogic.complete` (after 40 ms, 300 ms for a server): the core
    splits the text into the folder and the name typed, a worker (or KIO) lists
    the folder, the core ranks the folders (names starting with the text, then
    names containing it; hidden ones only after a dot; names that could not be
    typed back, with control or bidi characters, never; at most 50), and the
    list opens under the field. Down and Up choose, Tab takes the chosen one
    (the first when none is) and lists the folders inside it, Enter goes to the
    chosen one, a click takes one. The last listing is kept 3 s so a server
    isn't asked again for the next key. In a URL the name is percent-encoded.
  - Drops: a drop on a segment moves the files into that folder through a
    `KIO::CopyJob` recorded by the undo manager, or copies them with Ctrl held
    (or when the source offers only copy); no menu, KIO's own dialog for names
    already taken. While the pointer is over a segment it is lit and "Move to
    Documents" or "Copy to Documents" shows under it, and the drag action is
    set to match, which is what the drag cursor shows. The source is told the
    drop was a copy, so it never deletes files itself after a move that Files
    already did. Dropping on the folder the files are in does nothing. (Files
    dropped on a *tab* still use KIO's menu; see wave 1.)
  - Back and Forward: a long press (500 ms) or a right click opens a menu of
    the last 10 places of the tab's list, nearest first, as full paths; a
    pick jumps there in one step and the places passed stay in the other list,
    so Forward or Back walks them again. The release after a long press does not
    also click the button.
  - **Framework gaps** (for the Telamon OS Framework session): `TelamonBreadcrumb`
    has no menu per chevron, no drop target, no middle click and no edit mode,
    and its last segment has no chevron, so Files has its own `PathBar`, built
    from the same pieces (`Symbol`, `ContextMenu`, `TelamonTextField`,
    `TelamonStyle`); `TelamonAutocompleteField` filters its own list by the typed
    text (so it can't show "names containing it" completions that a worker
    ranked) and starts with nothing chosen, so the completion list is
    Files' too. Both would move upstream.
- **Command bar:** New (folder, text file, templates from
  `~/Templates` and KNewFileMenu's system templates), Cut, Copy, Paste,
  Rename, Share (a portal-free menu: email via `mailto:`, KDE Connect when
  installed, copy location), Delete, then Sort and View menus, and "…"
  (select all, invert selection, hidden files, file extensions, Properties,
  Open Terminal Here). Disabled states follow the selection and the folder's
  write access.
- **Sidebar** (TelamonSidebar): Home (a Windows-style home: pinned folders,
  recent files, frequent folders), Recent (`recentlyused:/`), pinned
  favourites (`user-places.xbel`, drag to pin and reorder), Desktop,
  Documents, Downloads, Pictures, Music, Videos, then Drives (Solid: internal
  partitions, USB, phones over MTP, with eject buttons and usage bars), then
  Network (`network:/`, saved servers, "Connect to Server…"), then Trash
  (with its item count). The sidebar shares `user-places.xbel` with every KDE
  file dialog, so pins show in Open and Save dialogs too.

  **Built (wave 3):** `PlacesLogic` (`cpp/kio/PlacesLogic.*`) owns one
  `KFilePlacesModel`; `PlacesModel` hands one section of it to a `Repeater`
  and `qml/PlaceItem.qml` draws a place. What decides is in the core
  (`atlas_explorer_core::places`: the section of a place from KFilePlacesModel's
  group, its kind, the actions its menu offers, the texts). Sections: the
  places (Home, Recent, the standard folders, pins), **Drives** (internal
  disks, USB drives, phones), **Network**, and the Trash pinned under the list
  as the sidebar's footer. The dated "Modified Today" lists and searches need an
  index Files doesn't have and are not listed; tags wait for their own wave.
  A first run (no `user-places.xbel` yet) adds Recent and the standard folders
  to the file KDE creates (Home, Trash, Network), so the sidebar starts as it
  always did and the same list shows in Open and Save dialogs; a file that
  exists is never added to. Network opens `network:/`, not KDE's `remote:/`.
  - **Pins:** "Pin to Sidebar" in a folder's context menu, "Pin This Folder to Sidebar" in the background menu, or a folder dragged from a view onto the sidebar, is pinned (KIO
    `stat` says it is a folder; only schemes that can be opened again, `file`,
    `smb`, `sftp`, `ftp(s)`, `webdav(s)`, `nfs`, `fish`) after the place it was
    dropped on; one that is a place already is not added twice (a hidden one
    comes back). Files dropped on a folder place move there (copy with Ctrl);
    anything dropped on the Trash is trashed. Pins and the standard folders are
    dragged by a copy of their row to a new position (`movePlace`, within the
    group). Right click (or the Menu key): Open, Open in New Tab, Rename…,
    Hide, Remove from Sidebar; hidden places show again through "Show Hidden
    Places" under the list (dimmed, "Show in Sidebar" in their menu).
  - **Drives:** a drive that is not mounted is mounted when clicked and opens
    when it is ready (the drive's state changing is what is waited for, as not
    every Solid backend sends `setupDone`); a mounted USB drive has an eject
    button that unmounts it and says "<name>: Safe to remove"; the menu of a
    fixed disk says Unmount. A mounted disk shows a usage bar (QStorageInfo on
    a worker, read again when the set of mounted disks changes and every 30 s)
    that turns to the warning colour from 90 %, with "12 GiB free of 64 GiB"
    as its tooltip. A phone (MTP) is a Solid portable media player: its place
    is the `mtp:` URL KIO's worker lists, and copying from it is a normal copy.
    KFilePlacesModel lists no gphoto2-only camera (there is no `camera:` worker
    installed); a camera that speaks PTP/MTP shows as a phone.
  - **Trash:** the count beside its name comes from a lister that follows
    `trash:/`, so it moves when something is trashed or restored. "Empty
    Trash…" measures the Trash (a KIO `DirectorySizeJob`, 6 s at most) and
    asks "Permanently delete all 12 items (4.2 MiB) in the Trash? This can't be
    undone." with Cancel as the default button.
  - **Open in Disks** (drives only, never a phone or folder): shown when
    Telamon Disks is installed, found by the desktop file
    `net.eterneon.telamon.disks.desktop` (or the `atlas` name) or the program
    `telamon-disks` (`atlas-disks`); checked when the menu opens. It calls
    `ShowDevice(udisks path)` on the session bus (`net.eterneon.telamon.disks`,
    then the `atlas` name; assumed names, to be agreed with Disks) and starts
    the program if nobody answers.
- **Views**, per tab, remembered per folder (in the settings file, not in
  hidden files dropped into folders):
  - Icons (sizes 48 to 256, thumbnails), List (compact, multi-column
    flow), Details (sortable, resizable and reorderable columns: Name, Size,
    Type, Modified, Created, Accessed, Permissions, Owner, Path in search
    results), Columns (Finder's Miller columns, one KCoreDirLister per
    column, preview of the selected file in the last column), Gallery (a
    large preview with a filmstrip, arrow keys move).
  - All views are virtualized (`ListView`, `GridView`, `TableView` with
    fixed delegate sizes and `reuseItems`), so a 100k-item folder creates
    only the delegates on screen.
- **Panes:** a preview pane (Alt+P: a large thumbnail, or text, or a media
  player) and a details pane (Alt+Shift+P: name, kind, size, dates,
  dimensions or duration from KFileMetaData's extractors run in a worker,
  permissions summary, tags later). **Split view** (F3): two panes in one
  tab, drag between them, F5/F6 copy and move to the other side.
- **Quick Look** (Space): an overlay with a 1024 px preview from KIO's
  thumbnailers (images, video frames, PDFs, fonts, office documents), the
  first 256 KB of a text file as plain text, and audio and video playable
  through QtMultimedia. Arrow keys move through the selection. Space or Esc
  closes.
- **Status line** (in the details pane footer when it is shown, at the
  bottom otherwise): item count, selection size, free space.

  **Built (wave 2):** `qml/StatusLine.qml` (Telamon.Ui's `StatusBar`) along the
  bottom of the window, one line of plain text: "12 items, 5 hidden, 3 selected
  (4.2 MiB), 128 GiB free". "N hidden" shows only while Show Hidden is off
  (`FolderModel.hiddenCount`: what the lister holds beyond the rows, counted
  100 ms after the listing changes), the size is the files of the selection
  (folders add nothing; none shown when only folders are selected), sizes are
  written as in the Size column (KIO's `convertSize`), and free space is `QStorageInfo`
  on a worker, asked when the folder changes and 1 s after its items change,
  and left out for a place that isn't on a disk (a server, Trash, Recent). The
  line is empty while the folder shows an error.
- **Operations:** a button in the toolbar shows a ring while the queue runs.
  It opens a popover with one row per operation: title ("Copying 1,204
  items to Backup"), a speed graph, bytes and items done, speed, time left,
  Pause/Resume and Cancel, and conflicts waiting for an answer. A finished
  operation stays in the list with Undo until the next one replaces it.

## Exported interfaces

### Launch and single instance

`telamon-explorer [--new-window] [--select] [--split] [URL|PATH ...]`. One
process (KDBusService Unique); a second launch hands its arguments and
working directory to the first, which opens them in new tabs of the active
window (the first is shown, the others open behind it; `--new-window` is read but
still opens tabs until multiple windows exist) and raises it with the launcher's
activation token. Arguments are parsed in Rust (`atlas_explorer_core::launch`):
at most 64 read, local paths resolved against the caller's working directory,
URLs must have a scheme KIO knows (`KProtocolInfo::isKnownProtocol`) and no
control or bidi characters. Anything else is refused and shown as plain text.

### org.freedesktop.FileManager1

Bus name `org.freedesktop.FileManager1`, object `/org/freedesktop/FileManager1`,
interface `org.freedesktop.FileManager1`, registered by the running app and
D-Bus activated through `/usr/share/dbus-1/services/org.freedesktop.FileManager1.service`
(Exec=`telamon-explorer --daemon-activation`; KDBusService takes the name).

| Method | Behaviour |
|---|---|
| `ShowFolders(as URIs, s StartupId)` | Each URI opens in a new tab of the active window (the first is shown) |
| `ShowItems(as URIs, s StartupId)` | Opens each item's parent folder (one tab per parent) with the items selected and scrolled into view |
| `ShowItemProperties(as URIs, s StartupId)` | Opens the Properties dialog for the URIs |

Any process in the session can call these, so: at most 64 URIs, each parsed
by the same launch rules, never executed or opened with an app, only shown.
StartupId is used to activate the window (KWindowSystem) and otherwise
ignored.

### Explorer's own D-Bus API (net.eterneon.telamon.explorer)

Object `/net/eterneon/telamon/explorer`, interface
`net.eterneon.telamon.explorer.Window1`, for the other Atlas apps:

- `OpenLocation(s uri, as select, a{sv} options)` (options: `new_window` b,
  `split` b, `view` s)
- `ShowOperations()` (raise the operations popover; Archive may call it after
  handing over a job)

### File index: net.eterneon.telamon.explorer.Search1

A separate user service, `telamon-explorer-indexd`, so search works when the
window is closed and Atlas Launcher can use it. It replaces Baloo's file
indexer for file names and metadata. No content indexing.

- Bus name `net.eterneon.telamon.explorer.Search`, object
  `/net/eterneon/telamon/explorer/Search`, interface
  `net.eterneon.telamon.explorer.Search1`. D-Bus activated
  (`/usr/share/dbus-1/services/net.eterneon.telamon.explorer.Search.service`,
  `SystemdService=telamon-explorer-indexd.service`).
- **Also, for one release, under the old name** (before Telamon Explorer, the
  Launcher and any app that has not moved use it): bus name
  `net.eterneon.atlas.explorer.Search`, object
  `/net/eterneon/atlas/explorer/Search`, interface
  `net.eterneon.atlas.explorer.Search1`, the same methods and signal, answered
  by the same process (`LegacySearch1` in the service) and with the same limit
  on running searches. The service claims the new name first, the old one
  right after (the old one is best effort: an `atlas-explorer-indexd` of the
  previous package may still hold it). Its own activation file
  (`net.eterneon.atlas.explorer.Search.service`) starts the same unit, and the
  old unit name `atlas-explorer-indexd.service` is a link to the new unit.
  Remove all of it, and `data/dbus/net.eterneon.atlas.explorer.Search1.xml`,
  in the release after.
- The introspection XML is installed in `/usr/share/dbus-1/interfaces/`
  and kept in the repo at `data/dbus/net.eterneon.telamon.explorer.Search1.xml`.

```
Search(s query, u limit, a{sv} options) -> (a(sssssxtd) hits)
  hit = (uri, name, kind, mime, icon, mtime, size, score)
    uri    file:// URI, percent-encoded (lossless for any byte in a name)
    name   display name: control and bidi characters made visible (see Trust)
    kind   "folder" or "file"
    mime   MIME type from the name (no content sniffing)
    icon   freedesktop icon name for the MIME type
    mtime  seconds since the epoch
    size   bytes (0 for folders)
    score  higher is better; hits come sorted, at most `limit` (cap 500)
  options (all optional):
    "kind"            s   "folder" | "file"
    "kinds"           as  categories: folder, document, spreadsheet,
                          presentation, pdf, image, audio, video, archive,
                          code, text, executable, font, disk-image
    "root"            s   a file:// URI; only hits under it
    "include_hidden"  b   default false
    "modified_after"  x   seconds since the epoch
    "modified_before" x
    "size_min"        t
    "size_max"        t
    "match"           s   "name" (default) | "path"

Status() -> (a{sv})
  "state"     s  "ready" | "scanning" | "stale" | "disabled" | "error"
  "entries"   u  number of entries
  "updated"   x  last completed scan or change, seconds since the epoch
  "roots"     as file:// URIs indexed
  "error"     s  plain-words reason when state is "error"

NotifyChanged(as uris)    hint from Explorer after its own operations
Refresh()                 rescan now (the Settings "Rebuild" button)

signal StatusChanged(a{sv} status)
```

- **Matching:** case- and diacritic-insensitive (NFKD, combining marks
  dropped, Unicode case folding). The query is split on spaces; every word
  must match. Classes, best first: whole basename, basename prefix, word
  prefix (words split at space, `-`, `_`, `.` and camelCase), acronym (the
  initials of the words), substring. Matched against the basename, or the
  whole path with `"match": "path"`.
- **Ranking:** match class first, then recency (newer of mtime and the
  last use in `recently-used.xbel`), then a boost for shallow paths and for
  Desktop, Documents and Downloads. Ties by name.
- **Calls:** each call is answered from memory, on its own thread, so rapid
  calls (one per keystroke) never queue behind each other; a newer call does
  not cancel an older one, the caller drops stale replies.
- **Cold start** (agreed with the Launcher): on activation the service maps
  the snapshot before it claims its bus name, and answers from it before any
  rescan: ≤ 100 ms from activation to the first reply. With no usable
  snapshot it claims the name at once, answers every Search with an empty
  array and reports `"scanning"` until the first scan is published. It never
  blocks a call on a scan.
- **What is indexed:** the user's home, on the home's own filesystem
  (mounts below it, such as FUSE, network shares, Vaults and removable
  drives, are not crossed). More roots can be added in Settings, removable
  drives only by the user's choice.
- **Excluded by default:** names starting with `.` (hidden files and
  folders, including `.cache`, `.local/share/Trash`, `.git`, `.venv`);
  names listed in a folder's `.hidden` file; folders holding `CACHEDIR.TAG`
  (cargo's `target`, many caches); `node_modules`, `__pycache__`; folders
  holding `pyvenv.cfg`. The list lives in
  `~/.config/telamon-explorer/indexrc` (`[Index] Roots=`, `Exclude=`), a plain
  INI file.
- **Freshness with no idle CPU:** the service blocks on inotify (no timers,
  no polling). It watches every indexed folder up to a budget (64k watches
  or half of `fs.inotify.max_user_watches`, whichever is less). Folders past
  the budget are rechecked when a query arrives and the last check is older
  than 5 minutes: a walk that compares folder mtimes and rescans only the
  folders that changed, run at idle priority while the query is answered
  from the current index. On start it loads the last snapshot (answering at
  once) and does the same mtime walk.
- **Storage:** a snapshot file at `~/.cache/telamon-explorer/index/v1.idx`
  (folder 0700, file 0600, written atomically), a documented flat format:
  header with version, checksum and counts, then a string arena and fixed-size
  records. It is read into memory and decoded (not mapped), checked as
  untrusted input, and must load within the 100 ms cold-start budget. It also
  records the exclusion rules it was made under: changed rules mean a rescan.
  It is a cache: deleting it only costs a rescan. No private data
  leaves the user's cache folder.
- **CLI:** `telamon-explorer-search [--kind K] [--in DIR] [--modified 7d]
  [--larger 10M] [--smaller 1G] [--limit N] [--json] QUERY`, a thin client of
  the D-Bus API.
- **Service unit:** a systemd user unit, `Type=dbus`, `Nice=19`,
  `CPUSchedulingPolicy=idle`, `IOSchedulingClass=idle`, `MemoryHigh=96M`,
  `NoNewPrivileges=yes`, `PrivateNetwork=yes`, `ProtectSystem=strict`,
  `CacheDirectory=telamon-explorer` (0700; the service reads `$CACHE_DIRECTORY`),
  `RestrictAddressFamilies=AF_UNIX`, a system-call filter, `TasksMax` and `MemoryMax`,
  `Restart=on-failure` with backoff. Not started at login; the first query
  starts it, and it stays to keep the watches.

## Renamed from Atlas Explorer (0.2.0)

Explorer was `atlas-explorer` (`net.eterneon.atlas.explorer`) until 0.2.0;
its shown name, Files, did not change. What it kept under the old name comes
over once, one way, never replacing anything (`atlas_explorer_core::legacy`,
run by the app at start through `telamon_adopt_legacy`):

- `~/.config/atlas-explorerrc` is copied to `telamon-explorerrc` by the
  framework (`Settings::for_app`, which the app asks for first), so `ShowHidden`
  and the rest stay; the old file is left, as a framework app that has not
  moved still reads it.
- `~/.config/atlas-explorer/` (`indexrc`) is moved to `telamon-explorer/`,
  or just its `indexrc` into a new folder that has none. The index service
  can't move it (it may write only its cache folder), so until the app has,
  it reads `atlas-explorer/indexrc` when the new file is absent.
- `~/.cache/atlas-explorer/` (the snapshot `index/v1.idx`) is moved to
  `telamon-explorer/`, so the first query after the upgrade is answered at
  once; when systemd has already made the new folder, only the snapshot is
  moved into it, if it has none. A snapshot that stays costs a rescan at worst
  (it is a cache). The snapshot's magic, `ATLASIDX`, is the format's and stays.
- The journal of operations (`~/.local/state/…/journal`, not written yet)
  is `telamon-explorer` from the start.

Kept for this release besides the bus names above: `/usr/bin/atlas-explorer`,
`atlas-explorer-indexd` and `atlas-explorer-search` (links to the new
commands), the hidden `net.eterneon.atlas.explorer.desktop` (the image's
`inode/directory` default and the Launcher's pinned apps name it), `Provides:
atlas-explorer`, and `ATLAS_EXPLORER_LOG` (read when `TELAMON_EXPLORER_LOG` is
not set). `org.freedesktop.FileManager1` is unchanged: a still-running
`atlas-explorer` keeps it until it exits.

## Consumed interfaces

| Peer | Interface (owner proposes, both review) | Used for |
|---|---|---|
| Atlas Archive | `net.eterneon.atlas.archive`, interface `net.eterneon.atlas.Archive1` (agreed 2026-10-05; Archive's DESIGN.md is the source) | "Extract Here", "Extract To…", "Compress to ZIP", "Compress…", drops of archive entries. Explorer never links libarchive |
| Atlas Backups (later) | `net.eterneon.atlas.backups`: a `ShowVersions(s uri)`-style deep link | "Restore Previous Versions" in the context menu and Properties, shown only when the service is installed (activatable name present) |
| Atlas Disks (later) | `net.eterneon.atlas.disks` (`net.eterneon.telamon.disks` after the rename): a `ShowDevice(s udisks_object_path)`-style deep link | "Open in Disks" for drives in the sidebar (built, wave 3; the names are assumed) and in Properties, shown only when installed |
| Atlas Launcher | consumes Search1 | — |

**Archive1, as Explorer uses it.** Every call passes `activation_token`,
`parent_window` and `show_progress=false`, so the job shows in Explorer's
queue, driven through its `net.eterneon.atlas.Archive1.Job` object (Title,
State, Processed/Total Bytes and Items, Error; Pause, Resume, Cancel;
`Finished(s state, as results)`, whose results Explorer selects).

- Extract Here: `ExtractHere(as archives, a{sv}) -> o`
- Extract To…: `ExtractAll(as archives, a{sv})` (Archive's own dialog)
- Compress to ZIP: `Compress(as files, "zip", "", a{sv}) -> o` (empty
  destination: `<name>.zip` beside the first item, `Archive.zip` for
  several, ` (2)` on a clash); Compress…: `CompressDialog(as files, a{sv})`
- A drop carrying `application/x-atlas-archive-entries` on a local folder,
  tab, breadcrumb segment or place: `ExtractEntries(s archive, as entry_ids,
  s folder, a{sv}) -> o` (IDs opaque); `text/uri-list` otherwise.
- Which files get the extract items: the `MimeType=` list of
  `net.eterneon.atlas.archive.desktop`, read through KService.
- Double-clicking an archive opens it with the mimeapps default (Archive's
  window). No "Open as Folder" in v1; when Archive ships its read-only
  `atlas-archive:` KIO worker, Explorer browses through it.
- `TooManyJobs` is shown as "Archive is busy, try again when a job
  finishes", without a retry.
- Archive asks its own questions (passwords, conflicts with the same
  choices as Explorer's, archive-bomb limits) stacked on Explorer's window.
| kdeglobals `TerminalApplication` | KTerminalLauncherJob | "Open Terminal Here", Shift+F4 (Ghostty) |

## Threads

The GUI thread never blocks.

- **KIO runs on the GUI thread, asynchronously**, as KIO is designed: jobs
  and KCoreDirLister deliver results through the event loop from worker
  processes (or KIO's in-process worker threads). The GUI thread only does
  bookkeeping per signal, in batches: a listing batch is appended in one
  `beginInsertRows`, sorting is not redone per batch.
- **Never on the GUI thread:** stat, `statfs`, `QStorageInfo`, KMountPoint
  lookups, reading file contents, `KFileItem::determineMimeType`, image
  decoding, sorting more than 1,000 items, search, checksums, completion
  listing of local folders, the journal's fsync. These run on workers (Rust:
  a small thread pool; C++: QThreadPool) and come back through
  `qt_thread().queue` or a queued signal. The MIME type on the GUI thread is
  the fast one, from the name; a content check, when the name gives none,
  runs on a worker and updates the row.
- Path bar: the subfolder menus and completions read a local folder on a
  worker (`LocationLogic`, QThreadPool) and a server's through a `KIO::listDir`
  job with a timeout; free space (`QStorageInfo`) is a worker too. Results
  return as queued signals carrying the request's number, and an answer to an
  old request is dropped.
- Sorting: names get a natural sort key once (Rust, casefolded, digit runs
  compared as numbers), computed on a worker when the batch arrives; a sort
  is a permutation computed on a worker, applied with one `layoutChanged`.
- Thumbnails: the freedesktop cache is read on workers first (a hit never
  touches KIO); misses go to KIO::PreviewJob, which runs the thumbnailers in
  the `thumbnail` KIO worker process and writes the cache. Only rows on
  screen (plus one screen ahead) are requested; scrolling away cancels.
  Remote files get no thumbnails unless the user turns them on.

## Operations

One queue per process, `OperationQueue`, holds every copy, move, link,
trash, delete, rename, new folder, restore from trash and empty trash.

- Runs one transfer at a time by default (two disks thrashing helps nobody);
  quick operations (rename, new folder, trash on the same filesystem) run at
  once beside it. "Run now" on a waiting row runs it in parallel.
- Each operation is a KIO job (CopyJob, DeleteJob, `KIO::trash`, SimpleJob)
  with Explorer's own UI delegate. Pause and Resume are `KJob::suspend` and
  `resume`; Cancel is `kill`. Speed and time left are computed in the core
  from processed bytes over a 5 s window.
- **Conflicts:** KIO's AskUserActionInterface is answered by a QML dialog:
  both files side by side (thumbnail, size, date, which is newer), Replace,
  Skip, Keep Both (KIO's suggested name, editable), and "Do this for all
  conflicts" for multi-item operations. Folders get Merge or Skip.
- **v0.1 note:** v0.1 uses KIO's `FileUndoManager` (every job is recorded
  with it) and KIO's own delegate for conflicts and job progress; the core's
  own undo record and the QML conflict dialog below come later.
- **Undo** (Ctrl+Z) undoes the last operation: copy (trash the copies),
  move and rename (move back), new folder (remove if still empty), trash
  (restore from the trash). Explorer keeps its own undo record in the core,
  not KIO's FileUndoManager, so it can say exactly what an undo will do and
  refuse when the files changed since. Permanent delete has no undo, and its
  confirmation says so.
- **Delete:** Delete moves to the Trash. Shift+Delete asks first ("Delete 3
  items for good? This can't be undone.", default button Cancel) and then
  deletes. Where a trash isn't available (some remote and removable
  filesystems), Delete says so and asks the same question.

### No data loss

What KIO 6.30 already does (read in `src/kioworkers/file/file_unix.cpp`):
an overwrite writes `<name>.part` and renames it over the target, so the old
file stays whole until the new one is complete; on any error the partial
output is removed; a move across filesystems deletes each source only after
its copy succeeded. What Explorer adds:

- **Journal.** Before an operation starts, a record (operation, sources,
  destination, what existed before) is appended to
  `~/.local/state/telamon-explorer/journal` (0600, fsync'd). The file being
  copied is recorded from CopyJob's `copying` signal (appended, fsync'd at
  most every 250 ms on a worker). A finished operation is marked done.
- **After a crash** (an unfinished record at start), Explorer removes only
  what the operation itself created and left incomplete: a destination file
  that didn't exist before, whose size is smaller than its source's, and a
  stale `.part` next to a target. Sources are never touched. Then it tells
  the user what was interrupted, with "Run Again".
- **Full disk:** a copy is refused at the start when the destination's free
  space (a worker `statvfs`, local only) is clearly too small, with the
  numbers; when it fills during the copy, KIO's error stops the operation,
  the partial file is removed, and the dialog offers Retry, Skip or Cancel
  after space is freed.
- **Pulled drive or dropped network:** the job's error stops the operation
  with the item it was on; nothing is deleted from the source side of a
  move that did not finish; the folder view of the vanished device closes
  back to its parent or Home.
- **Removable drives:** eject waits for the queue's operations on that
  drive to finish (or offers to cancel them), then syncs (udisks unmount).

## Trust

Untrusted input, checked where it enters:

- **File names** from any filesystem or server: shown through
  `atlas_explorer_core::display_name`, which makes C0 and C1 controls and DEL
  visible (U+2400 control pictures, newline as ␊), shows bidi overrides,
  embeddings and isolates (U+202A to U+202E, U+2066 to U+2069, U+200E,
  U+200F, U+061C) as a visible marker instead of letting them reorder the
  text, does the same for characters that show nothing or pass for something
  else (zero-width and other format characters, soft hyphen, tag characters,
  variation selectors, blank fillers such as U+3164 and U+2800), allows only
  one no-break or width space in a row (more are marked, so a run cannot hide
  an extension), shows invalid UTF-8 bytes as `\xNN`, and caps the shown length. Every
  QML `Text`/`Label` showing a name, path, or file content sets
  `textFormat: Text.PlainText`. The rename field shows the true name;
  saving a name with a `/`, NUL, or only dots is refused, a name with
  control or bidi characters gets a warning.
- **Opening files:** KIO::OpenUrlJob with Explorer's delegate. An executable
  file (exec bit, or an executable MIME type) is never run on a double-click
  without the "Run or open?" prompt (OpenOrExecuteFileInterface), whose
  default is Open. A `.desktop` file that isn't executable or not in a
  system location gets KIO's untrusted-program prompt
  (UntrustedProgramHandlerInterface), showing its real `Exec=` line, default
  Cancel. Scripts are opened in the text editor by default.
- **Service menus:** loaded by KFileItemActions with KIO's rules (a user's
  own service menu must be executable to load).
- **Thumbnails** are made out of process by the KIO thumbnail worker, with
  KIO's size caps (MaximumSize); a crash there costs one thumbnail.
- **D-Bus callers** (FileManager1, Window1, Search1): any process in the
  session. Arguments are capped and parsed like launch arguments; nothing
  they send is executed or opened with an app.
- **Drops** from other apps: URL lists are validated; raw data (text,
  images) is saved only after the user names the file.
- **Remote servers:** credentials go through KIO's password server
  (kiod, KWallet); Explorer never stores or logs them. URLs shown or logged
  have the user name and password removed.
- **The index** holds only names the user can already read, in the user's
  own cache (0700/0600); Search1 answers only processes in the same session
  bus.

## Privilege

None. Explorer and its index service run as the user. Mount, unmount and
eject go through udisks2 and its polkit actions (via Solid). Root-owned
files are read-only: no "Open as Administrator", no `admin:/` (Zach,
2026-10-05).

## Failure modes

| Failure | Behaviour |
|---|---|
| Folder can't be read | Empty state with the error in plain words and Retry |
| Folder removed while open | The tab goes to the nearest existing parent with a message |
| Slow or dead server | Listing shows a spinner and Stop; the window stays responsive; KIO's timeouts end it |
| Thumbnailer crashes or hangs | That file falls back to its icon; PreviewJob's timeout |
| Index missing, corrupt, older version | Rebuilt; Status says "scanning"; queries answer from what is ready |
| inotify watches exhausted | Lazy mtime rechecks on query (above) |
| Disk full, drive pulled, crash mid-copy | See "No data loss" |
| Settings file unreadable | Defaults, with a warning in the log |
| Peer app missing (Archive, Backups, Disks) | Its menu items are hidden |

Logging: `telamon-framework-ui` logging to the journal as `telamon-explorer` and
`telamon-explorer-indexd`: every operation's start, end, error and recovery;
URLs without credentials; never file contents.

## Budgets

Measured on this machine (i9-14900KF, NVMe btrfs) unless noted.

| What | Budget |
|---|---|
| Cold start to Home painted | ≤ 400 ms (≤ 1 s on the T480 profile) |
| Open a local 100k-file folder | first rows ≤ 150 ms, fully listed and sorted ≤ 1.5 s, no frame over 50 ms while loading |
| Scroll 100k items (details, icons) | p95 frame ≤ 8 ms, none over 16 ms |
| Change sort column on 100k | ≤ 200 ms |
| Thumbnail from cache after scrolling stops | ≤ 50 ms |
| Keypress to selection moved, type-ahead | ≤ 16 ms |
| Search1 query, 200k entries | ≤ 10 ms p95 server-side |
| Live search of a 100k-file tree | first hits ≤ 100 ms, done ≤ 1 s (warm cache) |
| Copy throughput, large local files | within 5 % of `cp` |
| RSS | ≤ 150 MB at Home, ≤ 300 MB with a 100k folder open |
| RSS of the index service | ≤ 40 MB for 200k entries (anonymous memory) |
| Idle CPU, window open or index running | 0 % (no timers, no animation when idle) |
| RPM | ≤ 10 MB |
