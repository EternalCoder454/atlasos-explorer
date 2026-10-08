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
0.2.0 plus waves 1 to 11 (tabs; the path bar, history menus and status line; the sidebar with pins, drives and the Trash; search; Quick Look, the preview pane and zoom; the operation queue, conflict dialog and undo; the context menus and name prompts; the Columns and Gallery views, grouping and each folder's remembered view; archives: Telamon Archive's jobs in the queue, and zip, tar and 7z files opened as read-only folders; names edited in place, Batch Rename, new items edited as they are made; the Home page, Connect to Server, the Network page and the handling of servers that don't answer)
only these parts of the sections below exist: a tab strip with one folder per
tab (Details, Icons, Compact, Columns and Gallery views, remembered per folder, with Group by), a breadcrumb path bar that becomes a
text field with completion, a search field with scope and filter chips that
shows the index's answer (or a live walk) as a Details view with a Path column, a status line, the command bar's New Folder, Cut, Copy, Paste, Rename and Move to Trash
with View and Sort menus, a sidebar of KIO places (pins, drives, phones, the Trash), KIO jobs run by one operation queue (ring and popover, conflict dialog, undo and redo), `FileManager1`, the launch parser and the index service. Everything
else (the details pane, split view) is design,
not behaviour. Sections that
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
| Archive extract and compress | **Telamon Archive** over its D-Bus API; Explorer only calls it |
| Archives as folders (read-only) | Explorer, through kio-extras' archive worker, with its own checks ("Archives", below) |
| Drive details, partitioning | **Atlas Disks** ("Open in Disks" hook) |
| Previous versions | **Atlas Backups** ("Restore Previous Versions" hook) |
| Baloo's indexer (already removed in the image) | Explorer's index service, used by Atlas Launcher |

## Layout

- `crates/atlas-explorer-core`: no Qt, no KF6. Display names from untrusted
  file names, natural sort keys and sorting, kinds from MIME and extension,
  the operation queue's state machine (order, pause, speed and time left,
  conflict policy), the operation journal (crash recovery), launch and D-Bus
  argument parsing, address-bar parsing and completion ranking, checksums,
  Frequent folders' counter (`home`), server addresses and recent servers
  (`servers`),
  the search filters' meaning, where a search runs, and the search texts
  (`search`). The live search walker is in `atlas-file-index` (`walk`), beside
  the matcher it shares with the index.
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
    answered by QML dialogs), `FileActions` (what the context menus hold, from KFileItemActions,
    as plain data for the QML menus), `Places` (KFilePlacesModel and Solid), `Opener`
    (KIO::OpenUrlJob, KTerminalLauncherJob), `DragHelper` (QDrag).
    Heavy or pure logic is called from these through a `cxx` bridge into the
    core crate, never written twice. `SearchController` is the search of one
    tab and `SearchService` the client of the index service.
  - `cpp/kio/HomeLogic.*` (the Home page's lists: Pinned, Recent files and
    Frequent folders), `cpp/kio/ServerLogic.*` (Connect to Server: the
    protocol list, the address from the dialog's fields, recent servers, the
    "Not encrypted" note, the switch for previews on servers) and
    `cpp/kio/NetworkModel.*` (the computers of the Network page, from Avahi over
    QtDBus and the SMB worker's browsing).
  - `cpp/kio/ArchiveClient.*` (Archive1 over D-Bus and `ArchiveJob`),
    `cpp/kio/ArchiveGuard.*` (what Files checks before KIO copies out of an
    archive).
  - `cpp/main.cpp`: Qt start, telamon-framework-ui startup, single instance,
    FileManager1.
  - `qml/`: the window, views and dialogs, all from Telamon.Ui.
- `apps/telamon-explorer-indexd`: the index service binary (zbus), plus its
  systemd user unit and D-Bus activation file.
- `apps/telamon-explorer-search`: the index CLI.
- `tests/archive-standin`: a stand-in for Telamon Archive's `Archive1` D-Bus
  API, for smoke tests (see "Archives"). Not installed.
- `tests/avahi-standin`: a stand-in for the part of the Avahi daemon the
  Network page asks (`ServiceBrowserNew`, `ResolveService`, `Free`), for
  smoke tests on a private "system" bus in a container with no multicast
  network. Not installed.

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
- **Search field** (wave 4): Ctrl+F or Ctrl+E focuses it; typing searches, Esc
  ends the search and shows the folder again.

  **Built (wave 4):** the field is in the toolbar (Telamon.Ui's `SearchField`,
  "Search Documents" with the tab's folder name, "Search Everywhere" for the
  other scope), the row under the toolbar is `qml/SearchBar.qml`, and each tab
  has its own `SearchController` (`cpp/kio/SearchController.*`: words, scope,
  chips), so a search belongs to its tab and survives switching tabs. The row
  shows while there are words or chips, or the field or the row has the
  keyboard.
  - **Scope:** the chips **This Folder** and **Everywhere**. Everywhere is the
    index (the home folder). This Folder below an indexed root is the index with
    `root` set; any other folder is a walk (see below). A folder is "in the
    index" when it is below a root the service reports, on the root's own
    filesystem, and no folder on the way is one the scanner leaves out (a
    dot-folder, `node_modules`, `__pycache__`, a `CACHEDIR.TAG` or `pyvenv.cfg`
    folder: `atlas_file_index::walk::covers`, asked of a worker). Names a
    person added to `Exclude=` are not known to Files; searching such a folder
    through the index finds nothing from it. With the index off (`disabled`:
    `Roots=` empty) every search is a walk (Everywhere walks the home folder).
  - **Filters:** chips **Kind** (Document, Image, Audio, Video, Archive, Code,
    Folder), **Modified** (Today, Past 7 Days, Past Month, Past Year) and
    **Size** (Small under 1 MiB, Medium 1 MiB to under 100 MiB, Large 100 MiB
    and over). They combine, each one that is set shows its value and an x that
    clears it, and they map one to one onto Search1's options (`kinds`,
    `modified_after`, `size_min`/`size_max`; a size also sends `kind` =
    "file", as a folder has no size). A Document is `document`, `spreadsheet`,
    `presentation`, `pdf` and `text`. Today starts at local midnight; the other
    periods count 7, 30 and 365 days back from now. Words and chips may be used
    alone: chips with no words list what passes them, newest first. Custom
    dates and sizes are not offered yet. The meaning of the chips is in the core
    (`atlas_explorer_core::search::filter`), which the walk and the D-Bus
    options both use.
  - **The index route:** every change of the words or a chip calls
    `Search(query, 500, options)` at once, asynchronously
    (`QDBusConnection::asyncCall`, 10 s), with no debounce: a call is answered
    from memory in a millisecond or two and several can be in flight. Each
    search has a number and an answer to an older one is dropped. A call to a
    service that isn't running starts it through its D-Bus activation file, and
    the first search waits for `Status` (which also starts it) so it knows the
    indexed folders; the field asks for `Status` when it gets the keyboard, so
    the first key does not wait. The hits replace the rows in one reset, in the
    order the index ranked them (the sort "Best Match"). `include_hidden`
    follows Show Hidden. More than 500 matches say "The first 500 results".
  - **Results are the Details view** (the view menu is ignored while searching)
    with a **Path** column (the folder of each result, `~/Documents/Reports`,
    written by the core's `search::path_text`); the same selection, keys,
    drags, Open With and context menu as a folder. Enter opens a file (and goes
    into a folder, which ends the search); **Open File Location** (Ctrl+Enter, or
    in the context menu) shows the result's folder in the tab with the file
    selected, and results in several folders open the others in tabs behind.
    Sorting by a column keeps working, and "Best Match" in the Sort menu
    returns to the order of the search. New Folder and Paste are off while
    results are shown (there is no "here"); Cut, Rename and Move to Trash work on
    the results and the rows whose files are gone are dropped when the job is
    done (`FolderModel::pruneSearchResults`), and an index search is repeated
    a moment later. Esc (in the field, the row or the results) clears the words
    and chips and lists the folder again; so does going to another folder.
  - **Outside the index** (a folder on an external drive, a folder the index
    leaves out, the index off): a live walk. A folder on this computer is walked
    by `atlas_file_index::walk` on its own thread: breadth first, the index's
    matcher and filters, no symlinks followed, no other filesystem entered,
    hidden names only with Show Hidden, the first hit sent at once and then a
    batch every 30 ms. A location that is not a local folder (a server, the
    Trash, an archive) is walked by `KIO::listRecursive` on the GUI thread, the
    entries judged by the same matcher. Both stop at 5,000 hits ("Stopped at
    5000 results"). The row shows a spinner, "Searching, 12 found" and a
    **Stop** button; Stop keeps what was found. A new search or leaving the
    search stops the walk. (A debug build reads
    `TELAMON_EXPLORER_TEST_WALK_BATCH_MS`, a pause after every batch, so the
    smoke tests can look at and stop a walk; a release build ignores it.)
  - **The status chip** at the right of the row shows the index's state, from
    `Status` and the service's `StatusChanged` signal: "The search index is up
    to date", "Updating the search index, results may be missing" (`scanning`
    and `stale`), "The search index is turned off, so searches look through the
    folders", "The search index has a problem, results may be missing: ..." and,
    when the service can't be reached, "Search isn't available". The status
    line counts the results ("42 results").
  - **When the service fails:** if a search that needs the index can't reach it
    (it won't start, doesn't answer in 10 s, or answers with an error), the
    results area says "Search Isn't Available" with one line of why, and a Try
    Again button; the chip says "Search isn't available". A search of a folder
    the index doesn't hold (a walk) does not need it and still works.
  - **Not in this wave** (see the roadmap): custom dates and sizes, content
    search, saved searches, the Settings page for indexed folders and the
    rebuild button, `re:` patterns.
- **Command bar:** New (folder, text file, templates from
  `~/Templates` and KNewFileMenu's system templates), Cut, Copy, Paste,
  Rename (one item in place, several in Batch Rename), Share (a portal-free menu: email via `mailto:`, KDE Connect when
  installed, copy location), Delete, then Sort and View menus, and "…"
  (select all, invert selection, hidden files, file extensions, Properties,
  Open Terminal Here). Disabled states follow the selection and the folder's
  write access.
- **Context menus** (Telamon.Ui `ContextMenu`; no `QMenu` is ever shown).

  **Built (wave 7):** right click, the Menu key and Shift+F10 (at the row the
  keyboard is on, or the top left of an empty folder) show one of two menus.
  Each is *decided once, before it is shown*: `FileActions::itemMenu` and
  `backgroundMenu` return a snapshot (plain data), the window fills the menu
  from it and only then pops it up, and nothing in it reads the clipboard, the
  folder or the plugins again while it is open (F124). The rules (which entries
  exist, which are enabled) are in the core, `atlas_explorer_core::menu`, from
  facts the C++ side collects (`MenuFlag`: folder writable, searching, in the
  Trash, items local, something to paste, Telamon Archive installed...).
  - **Items:** Open; Open With; an icon row (Cut, Copy, Paste, Rename, Move to
    Trash; Left and Right pick a button, a button that is off is skipped);
    Compress… (only when Telamon Archive is installed and the items are on
    this computer); Properties; and **More Actions**: Open in New Tab(s) for
    folders, Open File Location (search results and Recent), Open Terminal
    Here, Pin to Sidebar (one folder), Copy Path (Ctrl+Shift+C: the full path
    as plain text, one a line; a server's file gives its address), Hide or
    Unhide (below), Delete for Good… (asks, as Shift+Delete does), then the
    installed service menus. Paste goes into the one folder selected when there
    is one, else into the folder shown.
  - **Open With and the service menus** are KFileItemActions' own
    (`insertOpenWithActionsTo`, `addActionsTo`), made while the snapshot is,
    in a `QMenu` that is never shown; the window gets their actions as data
    (two levels: a service menu's group is a submenu) and runs one by its id
    through `runMenuAction`. KIO starts a service's command from its `Exec=`
    line by program and argument list, never through a shell, and a user's own
    service menu must be executable to load (KIO's rule). Ark's Compress and
    Extract plugins are left out (Archive replaces them), and so is the
    Activities plugin, which fills its submenu later ("Loading…").
  - **Background:** New (Folder, Text File and the files of `~/Templates`,
    read on a worker and kept up to date by a watcher), Paste, Undo and Redo
    by name, Sort and View (the same menus as the toolbar's buttons), Open
    Terminal Here, Pin This Folder to Sidebar, Properties. Show Hidden Files
    is in View.
  - **Names** (wave 10): F2, Rename and the toolbar's pencil edit one item's
    name where it is shown (`qml/InlineRename.qml`, in Details, Icons,
    Compact and the tab's own column of Columns): the field opens with the
    name without its extension selected (the core's `names::stem_len`: all of
    a folder's name, `report` of `report.tar.gz`, none of `.bashrc`'s dot).
    The name is checked as it is typed (the core's rules, and a listed item
    that has the name) and the reason shows under the field in plain words:
    `/`, an empty name, only dots, over 255 bytes are refused (Enter does
    nothing); control, bidi, leading or trailing space and similar are a
    warning, and Enter needs pressing a second time. Enter renames and the
    item stays selected and in view; Escape leaves the name; a click away
    renames when the name can be used, else leaves it and says why (the line
    above the list). The field shows the true name, not the display form. A
    view with the folder gone, or the item gone, ends the edit. Where a name
    can't be edited in place (the Gallery, which shows no name to edit, and
    search results, which are from many folders) the same check runs in the
    Telamon.Ui dialog `qml/NamePrompt.qml`, with "Use This Name" for a
    warning. The answer is queued like any other change (`rename`).
  - **New** (wave 10): Folder (Ctrl+Shift+N, the toolbar's New Folder), Text
    File and the `~/Templates` entries make the item at once, in the folder
    shown, under the first free name ("New Folder", "New Folder (2)"), through
    the queue (`makeFolder`, `makeFile`; a new file is recorded as a copy, so
    Undo trashes it and Redo brings it back; a template is copied with
    `KIO::copyAs`, an empty file is written with `KIO::storedPut`, which
    refuses a name that exists). When it is listed the view selects it and
    puts its name in edit mode (`createdItem`, `FolderView.renameWhenListed`).
    Escape keeps the free name. Nothing asks first. A rename onto a name the view
    doesn't list but the disk has (a hidden file) is refused in words.
  - **Batch Rename** (wave 10): F2, Rename or the pencil with two or more
    items selected in one folder (not in search results) opens
    `qml/BatchRenameDialog.qml`. Four rules: Find and Replace (plain text or a
    regular expression, "Match case"; on the whole file name, so an extension
    can change; a replacement in a pattern uses `${1}`, a bare `$1_` names a
    group called `1_`), Add Number (start, step, digits, before or after the
    name, text between), Change Case (lower, UPPER, Title, Sentence) and Add
    Text (before or after the name). Number, case and text work on the name
    without its extension, and on a folder's whole name. The items are taken
    in the order the folder shows them. The list shows each name before and
    after as the rule is set; a name that can't be used is marked with the
    reason and blocks Rename: not a name (the single rename's refusals), the
    same name as another item in the list, a name another selected item has
    now (a swap or a chain would need an order and a temporary name, so it
    is refused rather than done), or a file that is in the folder already (the list and Rename look at the
    disk as well as the listing, so a hidden file counts; one `lstat` for each
    name that changes, on this computer only).
    Warnings (hidden characters) show and do not block. The plan is
    `atlas_explorer_core::batch`, over `telamon_batch_plan`; Rename asks the
    core again, so a folder that changed under the dialog can't slip a bad
    name in. Rust's `regex` runs in time linear in the name, the pattern is
    capped at 512 bytes and its compiled size at 1 MiB, and a batch at 5000
    items. Apply is one operation in the queue (`renameMany`: one KIO move
    job per item, in order, as one entry in Undo and Redo, titled "Rename 50
    Items"); if one fails the ones before it stay done and are still one step
    to undo. The renamed items are selected afterwards.
  - **Hide** writes the item's name to the folder's `.hidden` file (one name a
    line, which KIO, Dolphin and the file index read), through the queue as a
    job on a worker: a folder on this computer only, never through a link, a
    file that isn't text or is over 1 MiB is left alone, the write is atomic.
    Unhide takes the name out. Neither is undoable. Show Hidden Files (Ctrl+H)
    shows the item dimmed.
  - **Extract Here, Extract To…, Compress to ZIP and Compress…** (wave 9)
    call Archive; see "Archives" below. Extract shows for archives only
    (every item's name is one of the types in Archive's desktop file's
    `MimeType=`), Compress for any item, and all four only when Archive is
    installed; they are off for items that aren't on this computer.
  - **Open Terminal Here** is `KTerminalLauncherJob`, which starts the
    terminal of kdeglobals (`TerminalApplication`, `TerminalService`) in the
    folder: the folder itself for a folder, the file's folder for a file, the
    folder shown otherwise; only on this computer.
  - **Not built:** "Open in New Window" (there is one window; the launch
    parser reads `--new-window` and opens tabs), the Settings list that hides
    entries (wave 16; the entries simply exist), and a Telamon.Ui menu row that
    holds icon buttons: `qml/IconRowItem.qml` has the shape such a row would
    ask for, and moves upstream when the framework has one.
- **Sidebar** (TelamonSidebar): Home (a Windows-style home: pinned folders,
  recent files, frequent folders), Recent (`recentlyused:/`), pinned
  favourites (`user-places.xbel`, drag to pin and reorder), Desktop,
  Documents, Downloads, Pictures, Music, Videos, then Drives (Solid: internal
  partitions, USB, phones over MTP, with eject buttons and usage bars), then
  Network (Files' own Network page, saved servers, "Connect to Server…"), then Trash
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
- **Home, Connect to Server and Network** (waves 3 and 11).

  **Built (wave 11):**
  - **The Home page** is the address `home:/`, one of Files' own pages: nothing
    is listed by KIO, `FolderModel.pageKind` says "home" and `FilesTabPage`
    draws `qml/HomePage.qml` in place of the folder view (which is hidden, so
    the keyboard goes to the page). A new tab, the first tab of a start with
    nothing to open and the sidebar's Home place show it; the Home *folder* is
    the first tile, "Home Folder", and is also reached by the path bar, `~`
    and Up. The sidebar's Home place counts as selected on both. Three
    sections, each folded by a click on its header and remembered
    (`[Home] Folded=` in `telamon-explorerrc`), and nothing recommended,
    nothing from the cloud:
    - *Pinned*: the folders of the sidebar's own list (the standard ones and
      the user's pins, `PlacesLogic`), hidden places left out.
    - *Recent Files*: at most 10. The Recent place's own source comes first
      (KActivities, through KIO's `recentlyused:/files` worker, asked in the
      background with a 5 s limit; where there is no activity service it
      answers nothing and changes nothing), then the freedesktop list
      `$XDG_DATA_HOME/recently-used.xbel`, read by the index crate's reader
      (size and entry limits, untrusted text), newest first. Only files that
      are still there (a stat each, on a worker), only on this computer, one
      line each: name, folder, "Used <date>". Nothing here is cleared by
      Files: the lists are the system's.
    - *Frequent Folders*: Files' own count of the folders the user went to
      (`home::Frequent`, in `[Home] Frequent=`): a count and the time of the
      last visit per folder. Bounded: 200 folders (the least visited, longest
      unused goes), a count stops at 1,000, a folder not visited for 180 days
      is forgotten, and a folder is listed from its second visit, 8 at most. What counts: a
      folder the user moved a tab to (path bar, sidebar, a tile, Back or
      Forward), on this computer or on a server, never with a password, query
      or fragment, never the home folder or `/`, never the Trash, Recent, the
      Network page, archives or the Home page itself. A folder that is gone is
      forgotten when the page notices. Nothing leaves this computer; **Clear**
      forgets every count at once and deletes the key from the file.
  - **Connect to Server** (Ctrl+Shift+K, the sidebar's "Connect to Server…" and
    the Network page's button): protocol (SFTP, Windows Share (SMB), FTP, WebDAV
    (Secure), WebDAV, NFS; those KIO has a worker for), server (`name`,
    `name:port`, an IPv4 address, `[IPv6]` or `[IPv6]:port`), folder and user.
    There is no password field: KIO asks when the server wants one (its own
    prompt and password service), and Files never reads, keeps or logs it.
    The address is built by the core (`servers::build`) from the checked fields,
    each percent-encoded: a field can't add a user, password, port, query,
    fragment or another host (`user:pw@host`, `host/path`, `a@b`, `?`, `#`,
    `%`, spaces, control and bidi characters, `..` out of the folder, a name
    over 253 bytes, bad ports are refused in words and the button stays off).
    The core also takes an address apart again (to fill the dialog from a
    recent server) and cleans every saved line, so a hand-edited settings
    file can't put anything but our own addresses, and no password, in the
    list. **Connect** goes to the server (and remembers it), **Add to Sidebar**
    adds a place to the Network section (`KFilePlacesModel::addPlace`, so
    Open and Save dialogs show it too; the address without a password).
    *Recent servers* (10, `[Servers] Recent=`) are listed in the dialog and on
    the Network page; a server typed in the path bar joins them. FTP and plain
    WebDAV (and NFS) say **Not encrypted** in the dialog and, while the folder
    is shown, in a banner under the toolbar (the core's `security_note`); SFTP,
    WebDAV over TLS and SMB don't.
  - **The Network page** is the address `network:/` (KIO has no worker for it
    in the image, so Files lists the computers itself, `NetworkModel`): Avahi's
    `ServiceBrowserNew` for `_smb`, `_sftp-ssh`, `_ssh`, `_ftp`, `_webdav(s)` and
    `_nfs` over QtDBus on the system bus, each found service resolved
    (`ResolveService`) and turned into an address by the core (a name that
    isn't a host name is dropped), and a KIO list job on `smb:/` (the SMB
    worker's own DNS-SD, WS-Discovery and NetBIOS browsing; never a password
    dialog). Both are asynchronous on the GUI thread. A scan shows a spinner
    and **Stop**, ends by itself after 10 s, and the Avahi side keeps listening
    while the page is open so a computer that switches on later is added; the
    same computer found twice (`nas` and `nas.local`) is one row; at most 500.
    With nothing found the page says so and points to Connect to Server; with
    no Avahi on the system bus it says Files can't look for computers. The
    recent servers are listed under the computers. Search is off on this page.
  - **Slow and dead servers** (a folder on `smb`, `sftp`, `fish`, `ftp(s)`,
    `webdav(s)` or `nfs`): while KIO has not answered, the view shows a spinner,
    "Connecting to <server>…" and **Stop** (`FolderModel.stop`); with items
    already listed, a small "Loading… Stop" bar. At the same time
    `FolderModel` makes an asynchronous TCP connection to the server's port
    (`QTcpSocket`, the URL's port or the protocol's own) with a 10 s limit. If
    that is refused, has no route, doesn't resolve or doesn't complete in 10 s,
    the listing stops and the page says **Can't Reach the Server** ("Can't
    reach the server. Check the address and that it is on.") with **Retry**.
    Once the server accepts the connection the test is over and only KIO's
    job is waited for, so a password prompt or a slow listing is never cut off
    (Stop ends it). The test is skipped when a proxy is set in KIO's settings
    (`kioslaverc`), and a Windows name that doesn't resolve is left to the SMB
    worker (it may resolve by NetBIOS). KIO's own errors for the same causes
    (cannot connect, unknown host, timeout, connection broken) show the same
    page; a wrong password and a cancelled prompt have their own words.
    Nothing here waits on the GUI thread, so the window and the other tabs
    stay usable (tried: one tab on a port that never answers, another one
    browsed meanwhile); closing the tab frees the model, which stops the job
    and the test.
  - **Servers show icons only.** A file on a server gets no thumbnail, and its
    type comes from its name (KIO's `MatchExtension`; nothing is read to find
    it), and no folder size is worked out, unless the switch **Preview Files on
    Servers** in the View menu is on (`[Remote] PreviewFiles`, off by default;
    the Settings window of wave 16 will hold it). With it on, thumbnails of
    files up to 5 MB on a server are asked of KIO's thumbnailers (which download
    the file), `PreviewSettings/MaximumRemoteSize` being set for that. Quick Look
    and the preview pane stay local-only.
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

  **Built (wave 8):** the views are `qml/DetailsView.qml`, `IconsView.qml`
  (Icons and, with `compact`, Compact; the cell is `IconCell.qml`),
  `ColumnsView.qml` (with `ColumnList.qml`) and `GalleryView.qml`, all drawing
  the rows of the tab's one `FolderView`, which keeps the selection, the
  keyboard and the drag and drop (a view only says where a row is: `reveal`,
  `rowAt`, `rowRect`, `neighbor`). Search results are always the Details view.
  - **Columns:** the strip is the folders on the way from the first column to
    the tab's folder (each a `ColumnList` over its own `FolderModel`, sorted
    and hiding as the tab's does), the tab's own column (the tab's
    `FolderModel`, with the selection), and then what the selected item is: a
    folder's items as one more column, or a file's preview (`PreviewPane`).
    The tab's location is the folder of the column with the keyboard, so
    Back, Forward, the path bar, the status line and every operation work as
    in the other views. Right arrow (or Enter, or a double click) on a folder
    goes into it and selects its first item; Left (and Backspace, Alt+Up)
    goes to the folder it is in, with it selected; both keep the view
    (`FolderView.keepView`: the next folder's remembered view is not brought
    back). A click or a right click on an item in another column goes to that
    column's folder with the item selected (`columnNavigateRequested`, the page
    does it with its history and `showItems`), then the menu is shown for the
    item; a drop on one goes into the folder under it. The first column
    starts at the folder shown when the view was chosen, and moves up when
    Left goes above it.
  - **Gallery:** `PreviewBody` (the preview pane's) shows the row the
    keyboard is on, over a filmstrip of the folder's thumbnails
    (`image://thumb/`, a cell is 5 grid units); Left and Up are the one
    before, Right and Down the one after, Page keys a screenful; the view
    starts on the first item. A player stops while Quick Look is open or the
    tab is not shown.
  - **Group by** (Sort menu: None, Name, Type, Date Modified; Details and Icons
    only): the core (`group`) says what group a row is in (its order key and
    its label: the first letter, "0-9" and "#"; the kind; Today, Yesterday,
    Earlier This Week, Last Week, Earlier This Month, Last Month, Earlier This
    Year, A Long Time Ago, from the calendar day in the local zone and the
    locale's first weekday), and the sort puts the groups first
    (`telamon_sort_permutation` takes each row's group key and whether the
    groups run backwards). Groups follow the sort's direction only when the
    rows are sorted by what they are grouped by; otherwise names and kinds
    run A to Z and dates newest first. `FolderModel` has the label per row
    (`groupKey`), the count per group and the folded groups (forgotten when
    another folder or another grouping is shown); the Details view uses the
    list's `section` (the property stays `groupKey`: with no grouping every
    row's key is empty and the header takes no room), the Icons view a list
    of lines made by `GroupLines` (a header, or a line of cells; `RowSlice`
    is one line's cells) because a grid can't have a header across its width.
    A folded group's items are still rows of the model: they are deselected
    when it folds, the arrow keys, Home, End, type-ahead, Shift-range and
    Select All go past them, and they come back when it opens.
  - **Per-folder memory:** `ViewMemory` keeps, in `telamon-explorerrc`,
    `[FolderViews] List` (one line per folder, the 500 changed last: view,
    sort column, direction, icon size, grouping and the folder's URL without
    password, query or end slash; the core's `views` module reads, bounds and
    writes it, bringing anything out of range back) and, in `[View]`, the
    view that folders without a line of their own have (`Mode`, `SortColumn`,
    `SortDescending`, `IconSize`, `GroupBy`; Details, Name, 96 px and no
    groups until changed). A tab that goes to a folder shows it the way it
    remembers (`FolderView.applyRemembered`), and a change of view, sort,
    icon size or grouping is kept for the folder it was made in (not while
    searching, and only when it differs from what the folder has, so looking
    at a folder never adds one). The list is written 0.4 s after the last
    change and at exit; nothing is written into a folder. View > "Use the
    Same View for Every Folder" makes the shared view the one every folder
    has (the folder shown gives it), and the folders' own lines wait for the
    switch to be turned off; "Reset This Folder's View" forgets the line.
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

  **Built (wave 5):** `qml/QuickLook.qml`, `qml/PreviewPane.qml`,
  `qml/PreviewBody.qml` (what both show) and `qml/MediaControls.qml` (the
  player), over `PreviewLoader` (`cpp/kio/PreviewLoader.*`, a QML type that
  finds out on a worker what a file is and, for text, reads it) and
  `PreviewLogic` (`cpp/kio/PreviewLogic.*`, the settings below). The core
  decides in `atlas_explorer_core::preview` (the category of a MIME type, the
  text reader and its cleaning, the details' texts) and `::zoom` (limits,
  steps, wheel arithmetic); both are tested without a display.
  - Space in a view (not while a name is being typed ahead) opens Quick Look
    over the window for the selected file (the row with the cursor when none is
    selected). With two files or more selected, Left/Up and Right/Down move
    through those files, starting at the one with the cursor, and only the
    cursor follows; with one or none they move through everything the view
    shows (a folder, search results, the Trash) and the selection follows.
    They stop at the ends (no wrap); Home and End go to the first and last.
    Enter opens the file in its app (`FileActions::openUrls`, with the usual
    "Run or open?" prompts; a folder opens in the tab) and closes it; Space,
    Escape or a click outside close it. Quick Look keeps the keyboard while
    it is open; the window's Delete, Copy, Cut, Paste and the like are off,
    and it closes when another tab is shown, the tab goes elsewhere, a text
    field takes the keyboard, or its file is gone.
  - What is shown: a picture from KIO's thumbnailers (`image://thumb/`, 1024 px
    here, 512 px in the pane) for images, PDF, video, audio, fonts and
    documents, and the file's icon with a line in plain words where no
    thumbnailer has one; for text and source code the first 256 KiB as plain
    text; for audio and video of this computer a player. The player opens
    the file when the preview shows it (so its length and picture size are
    known) and plays only when its button is pressed; moving on, closing
    or covering the pane with Quick Look ends it. A file on a server is not
    fetched: its preview is its icon and details, and Enter opens it.
  - The preview pane (Alt+P, View menu "Preview Pane"; off by default, kept in
    `[View] PreviewPane`) is on the right of the window, 20 grid units wide: for
    one selected file the same preview with Name, Kind, Size, Dimensions,
    Duration, Modified, Created and, among search results, Where; for several
    "N Items Selected" and their size. It never takes the keyboard. The
    details pane (Alt+Shift+P) and a pane that can be resized are not built.
  - Zoom: Ctrl+scroll, Ctrl+plus, Ctrl+minus change the icons' size in the
    Icons view (48 to 256 px, steps of 16, default 96) and the rows' height in
    Details, Compact and Columns (24 to 64 px, steps of 4, default two grid
    units); Ctrl+0 resets the one in use. The rows' height is kept in
    `[View] RowHeight` (anything out of range in the file is brought back by
    the core) and shared by every tab; the icons' size is each folder's own
    (wave 8, see per-folder memory) and Ctrl+0 returns it to the shared
    view's. The View menu has Zoom In, Zoom Out and Reset Zoom; the gallery
    has neither size.
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

**Archive1, as Explorer uses it.** The names are Archive's: first
`net.eterneon.telamon.archive` at `/net/eterneon/telamon/archive`, interface
`net.eterneon.telamon.Archive1`; when that name has no owner, the old
`net.eterneon.atlas.archive` (kept for one release). Every call passes
`parent_window` (`x11:<id>` on X11; Wayland's xdg-foreign handle is not
available to Qt, so none there) and, on Wayland, an `activation_token`
(asked of KWaylandExtras, waited for half a second at most). A call that
returns a job passes `show_progress=false`, so the job shows in Explorer's
queue; Archive's dialogs and questions come up in its own window regardless.

- Extract Here: `ExtractHere(as archives, a{sv}) -> o`, a job
- Compress to ZIP: `Compress(as files, "zip", "", a{sv}) -> o` (empty
  destination: `<name>.zip` beside the first item, `Archive.zip` for
  several, ` (2)` on a clash), a job
- Extract To…: `ExtractAll(as archives, a{sv}) -> o` (Archive's own dialog, no
  picker of Explorer's) and Compress…: `CompressDialog(as files, a{sv}) -> o`.
  Since Archive 0.3.0 both return a job that waits in `waiting-for-user`
  while the dialog is open (Cancel in the dialog ends it as `cancelled`) and
  runs once it is answered, so they are followed like the others: in the
  queue, and the result selected. They pass `show_progress=false` too; the
  dialog and any question still come up in Archive's window.
- Which files get the extract items: the `MimeType=` list of
  `net.eterneon.telamon.archive.desktop` (then the old
  `net.eterneon.atlas.archive.desktop`), read with KDesktopFile; an item
  counts by its name's MIME type (`QMimeType::inherits`), never by sniffing.
- A job object (`.../job/<n>`, interface `<iface>.Job`) is followed by
  `ArchiveJob` (`cpp/kio/ArchiveClient.*`), a `KJob`: `Finished(state,
  results)` is listened for from before the call (a short job can end before
  the reply is read), `GetAll` and `PropertiesChanged` give `State`,
  `ProcessedBytes`/`TotalBytes` and `ProcessedItems`/`TotalItems` (the
  queue's progress), Pause, Resume and Cancel are `suspend`, `resume` and
  `kill` (a Cancel asked before the call has returned its path is sent as
  soon as it does), `waiting-for-user` says once "Telamon Archive needs an
  answer from you", and the job's `Error` is shown in plain words (made safe
  to show: it can hold names from the archive). Archive going away ends the
  job with "Telamon Archive stopped before it finished.". `results` that are
  `file://` URIs are selected when the job is done, in a tab that shows their
  folder. A job is `Kind::External` in the queue: a transfer, so it takes
  turns with copies and moves, and is not undoable.
- `TooManyJobs` is shown as "Archive is busy, try again when a job finishes."
  with no retry; `InvalidArgs` as "Telamon Archive can't use these files. It
  opens only files on this computer."; no owner on either name as "Telamon
  Archive could not be started.".
- Not done: a drop carrying `application/x-telamon-archive-entries` (or the
  old `x-atlas-` type) on a folder, tab, breadcrumb segment or place, for
  `ExtractEntries(s archive, as entry_ids, s folder, a{sv}) -> o`. Archive
  0.3.0 specifies the payload (its DESIGN, "Drag-out and `ExtractEntries`"):
  UTF-8 JSON `{"version":1,"archive":"file:///…/a.zip","entries":["12-1a2b3c4d",…]}`,
  the `archive` URI and the tokens passed to `ExtractEntries` unchanged with
  the dropped-on folder's URI. Files doesn't accept the type yet (a later
  wave: a `DropArea` on the views, tabs, breadcrumb and places that calls it
  as a job). Drops of `text/uri-list` work as for any files.
- Double-clicking an archive opens it as a folder in Files (below); "Open
  With" still gives Archive's window.
- Archive asks its own questions (passwords, conflicts with the same
  choices as Explorer's, archive-bomb limits) stacked on Explorer's window.
| kdeglobals `TerminalApplication` | KTerminalLauncherJob | "Open Terminal Here", Shift+F4 (Ghostty) |

## Archives

**Built (wave 9).** Two things, which don't share code: Telamon Archive's jobs
(above), and archives opened as folders.

- **Opening** an archive (Enter, double click, "Open") that is a file on this
  computer and whose MIME type KIO's archive worker has a protocol for
  (`KProtocolManager::protocolForArchiveMimetype` for exactly that type, so a
  `.docx` or `.epub`, which inherits `application/zip`, opens as a document:
  `zip`, `tar` for the `.tar.*` family, `sevenz`, `ar`) goes to `zip:/path/a.zip` and so on, a folder like
  any other that KCoreDirLister lists through kio-extras. Others (a lone
  `.gz`, RAR, ISO) open with the default application as before. The zip's end
  is looked at first, on a worker (zip64, junk after the end record and data
  before the zip are followed): when its central directory marks an entry
  encrypted, or can't be read well enough to tell, the archive is not opened,
  and the window says it needs a password (or that Files can't tell) (KArchive can't decrypt, and would hand out the encrypted bytes as
  the files).
- **Read-only:** `FolderModel.inArchive` is true for these schemes; `canWrite`
  is false (kio-extras reports its folders writable), so New, Cut, Paste,
  Rename and Move to Trash are off, and a drop into an archive (or a paste)
  is refused: "An archive is open for reading only…". The status line says
  "archive, read-only"; free space isn't shown.
- **Where it is:** `atlas_explorer_core::archive::locate` splits
  `zip:/home/u/a.zip/sub` into the file (the first part of the path whose name
  ends like an archive; a folder called `x.zip` above an archive is not told
  apart, KIO's own answer would need the disk) and the path inside. The path bar
  shows where the archive is, then the archive as a folder, then the folders in
  it (Home › Documents › a.zip › sub); Up from the top is the folder the
  archive is in; the display path and tab title follow.
- **Extract button** (command bar, only in an archive): with Archive
  installed it is Archive's "Extract All" (`ExtractAll`, its dialog); without
  it Files extracts by KIO into a new folder beside the archive named after it
  (`photos.tar.gz` gives `photos`, ` (2)` when taken), as one queued operation:
  look, choose the name and check the room, make the folder, copy what the
  archive holds at its top.
- **Taking entries out** (drag, paste, the Extract button without Archive) is
  a copy by KIO, through the queue, so a name that is taken opens the
  conflict dialog and nothing is replaced without it. Out of an archive things
  are only copied (a drag offers Copy only, a drop makes no Move, Copy, Link
  menu, Shift moves nothing, and a drop on the Trash or a Link says why it
  can't) and the operation is not recorded for undo (a redo would skip the look
  below). Before the copy starts, three steps of the same operation run:
  1. `ArchiveGuardJob` looks at the zip's encryption flag, then lists what is
     to be copied (`KIO::listRecursive`, stat for a file or a link) and hands
     every path and link target to the core, which refuses the whole
     operation when any entry's path is absolute, holds `..` (split on `/`
     and `\`), a NUL or a drive, is longer than 256 components or 4,096
     bytes, or is a link whose target is absolute or holds `..` (so no chain
     of links can lead out: a link to `.` followed by `x/..` is why `..` is
     refused and not resolved). The rules are Telamon Archive's own
     (`core::path`), made stricter for links because KIO can't do Archive's
     `openat2(RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS)` work. KArchive lists a
     `../x` entry as a folder named `..`, so this is a real case, and it
     is tested with real zip and tar files made to do it. The refusal is a
     dialog naming the first entry and how many are like it, and offering Telamon
     Archive (which skips such entries and extracts the rest) when installed.
  2. `ArchivePrepareJob` picks the folder's name (for the Extract button) and
     checks, on a worker, that the disk has room for the sizes the listing
     gave ("Not Enough Space"). A server is not asked.
  3. Then `KIO::copy`.
  What it does not do: the archive file can change between the look and the
  copy (it is the user's own file); setuid and setgid bits in a tar come
  through KIO's copy as KIO sets permissions (the files are the user's own,
  but they aren't dropped as Archive drops them); and a `.tar.gz` that is cut
  off lists only what was read and extracts silently as that (KArchive's
  limit; Telamon Archive reports it).
- **Errors** in plain words, never KIO's text: an archive KIO can't list says
  "This archive couldn't be read. It may be damaged, or it may need a password,
  which Files can't enter." (empty state "Can't Open This Archive", with Retry); a
  zip that needs a password says so; a refusal, no room, Archive busy or not
  running, and what Archive says (`Error`) show in the popover row's red line
  and a toast or dialog.
- **Testing without Archive's server:** Telamon Archive doesn't serve `Archive1`
  yet (its DESIGN has the API, its code has none). `tests/archive-standin`
  (`telamon-archive-standin`, a workspace member, never installed) serves the
  API on the session bus: jobs with progress, Pause, Resume, Cancel, `Finished`,
  `TooManyJobs`, `InvalidArgs`; it extracts with `unzip`, `tar` and `7z` and
  compresses with `zip`, `tar` and `7z`, asks nothing, and logs each call
  (`STANDIN_LOG`). `STANDIN_SLOW_MS`, `STANDIN_MAX_JOBS` and `STANDIN_ASK`
  make jobs slow, few, or ask the user, to try the popover and the errors.

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
- Quick Look and the pane: `PreviewLoader` asks a worker (two threads) for
  the MIME type, a picture's size and a text file's first 256 KiB; the answer
  returns through the application object and an answer to a file that is no
  longer shown is dropped (and a queued file that is no longer wanted is not
  read). Thumbnails come through the same `image://thumb/` provider as the
  Icons view.
- Path bar: the subfolder menus and completions read a local folder on a
  worker (`LocationLogic`, QThreadPool) and a server's through a `KIO::listDir`
  job with a timeout; free space (`QStorageInfo`) is a worker too. Results
  return as queued signals carrying the request's number, and an answer to an
  old request is dropped.
- Search: an index search is an asynchronous D-Bus call (QtDBus on the GUI
  thread, 500 hits parsed there), and an answer to an older search is dropped
  (each carries a number). Whether the index holds a folder is asked of a
  worker; the live walk of a local folder runs on its own thread and reports
  back through queued calls that carry the search's number; a walk of a server
  or the Trash is a `KIO::listRecursive` job on the GUI thread.
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
trash, delete, rename, new folder, new file, hide, restore from trash and empty trash.

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
- **Built (wave 6):** the state machines are the core's (`queue`, `history`,
  `undo`, `conflict`, `preflight`, `optext`), reached through `src/ops_ffi.rs`
  by `cpp/kio/OperationQueue.*` (a list model for the popover; it turns the
  core's actions into KIO jobs: `CopyJob`, `DeleteJob`, `KIO::trash`,
  `moveAs`, `mkdir`, `rmdir`, `restoreFromTrash`, `emptyTrash`, all with
  `HideProgressInfo`, so KIO shows no dialog and no Plasma tracker) and
  `OperationAsker`, the `AskUserActionInterface` that is a child of each job's
  delegate. `FileActions` only asks the queue; nothing else changes files.
  Progress is read from the jobs every 250 ms and handed to the core, which
  computes speed and time left over 5 s.
  - **Ring and popover:** `qml/OperationsButton.qml` in the command bar shows
    while any operation is listed: a ring that fills (a quarter turns while
    the total is not known; grey while everything is paused), a tick when all
    is done, a warning mark when the last one failed. The popover has one row
    per operation: label, bar, "186 MiB of 781 MiB, 38.7 MiB/s, 16 s left",
    Pause or Resume, Cancel, Run Now on a waiting transfer, Undo on the last
    one done, Dismiss on finished ones, and Clear Finished.
  - **Conflicts:** `qml/ConflictDialog.qml` (a Telamon.Ui `TelamonDialog`)
    shows the file there and the one coming side by side (thumbnail or icon,
    size, date, "Newer" on the later one) and the answers the core allows
    (`conflict::choices`): a file gets Replace, Skip and Keep Both with a
    suggested, editable name; a folder gets Merge or Skip; a file and a
    folder, or a paste onto itself, only Skip and Keep Both. "Do this for all
    conflicts" is remembered per kind by the core's queue; a remembered answer
    that doesn't fit a conflict skips it. Escape cancels the operation. A
    problem a job reports (a file that can't be copied) opens
    `qml/ProblemDialog.qml`: Retry, Skip, Skip All, Cancel; so does KIO's
    "can't go to the Trash, delete instead?".
  - **Cut:** items cut here or in another app (`application/x-kde-cutselection`)
    are drawn at half strength (`FolderModel` role `isCut`) and the command
    bar says "2 items waiting to move". The clipboard is cleared only after
    the move finished, so a refused or cancelled paste leaves them waiting.
  - **Undo and redo:** Ctrl+Z, Ctrl+Shift+Z and Ctrl+Y (not while a text field
    has the keys). The core keeps the last 20 entries on each side with the
    title the user sees ("Move 3 Items to Backup"); the toast says "Undid:
    Move 3 Items to Backup". Before an undo the files are looked at (workers,
    and KIO stat jobs for the Trash and servers) and `Undo::check` refuses in
    words when something changed since; the entry is then dropped. Running an
    undo produces the record that reverses it (`Undo::inverse`), which is
    what Redo runs: copies are trashed and restored, moves go back and
    forth, a new folder is removed (`rmdir`, which refuses a folder that
    holds anything) and made again. Undo never deletes: it only trashes,
    moves, restores or `rmdir`s. **An operation that replaced or merged
    files is not recorded** and empties both lists (what it "created" holds
    files that were there before), and the window says so. Permanent delete
    has no record; its dialog says "This can't be undone."
  - **Refused up front:** a folder moved or copied into itself (or into one
    inside it) is refused before it is queued, from the URLs; for local files
    the check runs again on a worker with real paths (links), together with
    the room at the destination (`statvfs` against the sources' size; a move
    on the same disk needs none). The dialog gives the numbers: "There isn't
    enough space in "Backup". Copying "big.iso" needs 4.2 GiB, and only 1.1
    GiB is free."
  - **Drops:** the path bar's and the sidebar's drops, the tab strip's and the
    folder view's, and the search results' trash, cut, copy and delete all
    call the queue. A drop with Shift moves, with Ctrl copies, with
    Ctrl+Shift links; with no key a menu offers Move Here, Copy Here, Link
    Here.
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

- **Journal** (not built: the Reliable phase; wave 6 keeps its undo
  lists in memory only). Before an operation starts, a record (operation, sources,
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
- **File content in Quick Look and the pane:** shown only as a picture from
  the thumbnailers or as plain text, never as HTML, Markdown or any markup
  (`TextEdit.PlainText`; the same goes for every name and detail). The text
  reader (`atlas_explorer_core::preview::read_text`) opens the file without
  blocking and without taking a controlling terminal, refuses anything that is
  not a regular file once it is open (a named pipe, a device, a socket, a
  folder; a symlink to a regular file is followed, a link to a device is not),
  reads at most 256 KiB, refuses binary content (a NUL byte, mostly controls
  or invalid UTF-8) and shows control, bidi and invisible characters as the
  markers names get (an escape sequence is shown, never sent), with lines cut
  at 2,000 characters. Only a path on this computer is read: a `file:` URL, or
  the real place the Trash's worker reports for a `trash:` item; no other
  worker's answer is used as a path. Pictures are decoded by the thumbnailer
  process, only their header is read here (`QImageReader::size`, SVG
  excepted). Audio and video play only when the user presses Play.
- **D-Bus callers** (FileManager1, Window1, Search1): any process in the
  session. Arguments are capped and parsed like launch arguments; nothing
  they send is executed or opened with an app.
- **Drops** from other apps: URL lists are validated; raw data (text,
  images) is saved only after the user names the file.
- **Remote servers:** credentials go through KIO's password server
  (kiod, KWallet); Explorer never stores or logs them. Connect to Server has
  no password field, builds its address from checked fields (hostile input is
  refused, see "Home, Connect to Server and Network"), and the recent servers,
  the sidebar place, Frequent Folders, the restored tabs and the per-folder
  views keep addresses without a password. URLs shown or logged have the
  user name's password removed. A name found on the network (Avahi, SMB) is
  untrusted text: it is shown through the core's display names, and the
  address Files opens for it is the core's. FTP, plain WebDAV and NFS are
  marked "Not encrypted".
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
| Server doesn't answer in 10 s, refuses, has no route or doesn't resolve | "Can't Reach the Server" with Retry; the listing is stopped; other tabs are unaffected |
| Server accepts the connection and then says nothing | The spinner and Stop stay (KIO's own timeouts end it); a password prompt is never cut off |
| Wrong password, prompt cancelled | "Couldn't sign in. Check the user name and password." / "The connection was canceled." with Retry |
| No Avahi on the system bus | The Network page says Files can't look for computers (the SMB worker still looks); Connect to Server works |
| No activity service (Recent) | Home's Recent files are the freedesktop list only |
| Thumbnailer crashes or hangs | That file falls back to its icon; PreviewJob's timeout |
| A file can't be previewed (no thumbnailer, binary, unreadable, special, gone) | The icon and one plain line saying why; Quick Look and the pane stay usable |
| Media can't be played | The player says "This file can't be played."; nothing else changes |
| Index missing, corrupt, older version | Rebuilt; Status says "scanning"; queries answer from what is ready |
| inotify watches exhausted | Lazy mtime rechecks on query (above) |
| Disk full, drive pulled, crash mid-copy | See "No data loss" |
| Settings file unreadable | Defaults, with a warning in the log |
| Index service not installed, won't start, or doesn't answer | A search that needs the index shows "Search Isn't Available" (and the chip "Search isn't available") with Try Again; a search of a folder the index doesn't hold is a live walk and works |
| Index service busy (too many searches at once) | The search is asked again after 150 ms |
| Live walk of a folder that can't be read | The results area says "Can't Search This Folder" |
| Peer app missing (Archive, Backups, Disks) | Its menu items are hidden; Open on an archive and the Extract button in an archive still work through KIO |
| Archive busy, not running, or a job it runs fails | The popover row says why in Archive's words (or "Archive is busy, try again when a job finishes."); nothing is retried |
| An archive KIO can't read, or a zip that needs a password | "This archive couldn't be read…" or "\"a.zip\" needs a password, which Files can't enter."; nothing is extracted |
| An archive with an entry that would leave the folder | The whole extraction is refused, in a dialog, before anything is written |

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
