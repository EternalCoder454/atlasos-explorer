# Atlas Explorer: design

What this file fixes: the layout, the threading rule, what is trusted, the
APIs Explorer exports and consumes, who owns what, the failure modes and the
budgets. Change it together with the code that changes them. The full plan
and its reasons are the Atlas Notes note "AtlasOS/Explorer/Plan"; the
checklist is "AtlasOS/Explorer/Roadmap".

App ID `net.eterneon.atlas.explorer`, binary `atlas-explorer`, shown name
**Files** (the name the image already gives Dolphin). Rust + Qt 6.11 Quick +
Kirigami over CXX-Qt, Atlas.Ui from the installed `atlas-ui`, KF6 6.30 (KIO,
Solid, KService, KCoreAddons, KDBusAddons, KWindowSystem).

## Scope

Explorer replaces Dolphin completely.

| Dolphin today on AtlasOS | Who covers it |
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
- `apps/atlas-explorer`: the app.
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
  - `cpp/main.cpp`: Qt start, atlas-framework-ui startup, single instance,
    FileManager1.
  - `qml/`: the window, views and dialogs, all from Atlas.Ui.
- `apps/atlas-explorer-indexd`: the index service binary (zbus), plus its
  systemd user unit and D-Bus activation file.
- `apps/atlas-explorer-search`: the index CLI.

## Window

- Title bar with Windows 11 caption buttons (the AtlasOS decoration), then
  the **tab strip**: closable, reorderable by drag, drop files on a tab to
  move them into it, middle-click closes, Ctrl+T, Ctrl+W, Ctrl+Shift+T
  reopens, Ctrl+Tab cycles, a folder dragged out of the window opens there.
- **Toolbar row:** Back, Forward, Up, Refresh; the **address bar**; the
  **search field** ("Search Documents").
  - Address bar: an Atlas breadcrumb (segments with chevrons; a chevron opens
    the subfolder menu; drop files on a segment). A click on the empty part,
    Ctrl+L, F4 or Alt+D turns it into a text field holding the URL or path,
    with completion of folder names (local: a worker lists the folder; remote:
    a KIO listDir with a timeout), `~` and environment-free shortcuts
    (`~`, `trash:`, `recent:`, `network:`), Enter goes, Esc returns to the
    breadcrumb.
- **Command bar:** New (folder, text file, templates from
  `~/Templates` and KNewFileMenu's system templates), Cut, Copy, Paste,
  Rename, Share (a portal-free menu: email via `mailto:`, KDE Connect when
  installed, copy location), Delete, then Sort and View menus, and "…"
  (select all, invert selection, hidden files, file extensions, Properties,
  Open Terminal Here). Disabled states follow the selection and the folder's
  write access.
- **Sidebar** (AtlasSidebar): Home (a Windows-style home: pinned folders,
  recent files, frequent folders), Recent (`recentlyused:/`), pinned
  favourites (`user-places.xbel`, drag to pin and reorder), Desktop,
  Documents, Downloads, Pictures, Music, Videos, then Drives (Solid: internal
  partitions, USB, phones over MTP, with eject buttons and usage bars), then
  Network (`network:/`, saved servers, "Connect to Server…"), then Trash
  (with its item count). The sidebar shares `user-places.xbel` with every KDE
  file dialog, so pins show in Open and Save dialogs too.
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
- **Operations:** a button in the toolbar shows a ring while the queue runs.
  It opens a popover with one row per operation: title ("Copying 1,204
  items to Backup"), a speed graph, bytes and items done, speed, time left,
  Pause/Resume and Cancel, and conflicts waiting for an answer. A finished
  operation stays in the list with Undo until the next one replaces it.

## Exported interfaces

### Launch and single instance

`atlas-explorer [--new-window] [--select] [--split] [URL|PATH ...]`. One
process (KDBusService Unique); a second launch hands its arguments and
working directory to the first, which opens them in new tabs of the active
window (or a new window with `--new-window`) and raises it with the launcher's
activation token. Arguments are parsed in Rust (`atlas_explorer_core::launch`):
at most 64 read, local paths resolved against the caller's working directory,
URLs must have a scheme KIO knows (`KProtocolInfo::isKnownProtocol`) and no
control or bidi characters. Anything else is refused and shown as plain text.

### org.freedesktop.FileManager1

Bus name `org.freedesktop.FileManager1`, object `/org/freedesktop/FileManager1`,
interface `org.freedesktop.FileManager1`, registered by the running app and
D-Bus activated through `/usr/share/dbus-1/services/org.freedesktop.FileManager1.service`
(Exec=`atlas-explorer --daemon-activation`; KDBusService takes the name).

| Method | Behaviour |
|---|---|
| `ShowFolders(as URIs, s StartupId)` | Each URI opens in a new tab of the active window |
| `ShowItems(as URIs, s StartupId)` | Opens each item's parent folder (one tab per parent) with the items selected and scrolled into view |
| `ShowItemProperties(as URIs, s StartupId)` | Opens the Properties dialog for the URIs |

Any process in the session can call these, so: at most 64 URIs, each parsed
by the same launch rules, never executed or opened with an app, only shown.
StartupId is used to activate the window (KWindowSystem) and otherwise
ignored.

### Explorer's own D-Bus API (net.eterneon.atlas.explorer)

Object `/net/eterneon/atlas/explorer`, interface
`net.eterneon.atlas.explorer.Window1`, for the other Atlas apps:

- `OpenLocation(s uri, as select, a{sv} options)` (options: `new_window` b,
  `split` b, `view` s)
- `ShowOperations()` (raise the operations popover; Archive may call it after
  handing over a job)

### File index: net.eterneon.atlas.explorer.Search1

A separate user service, `atlas-explorer-indexd`, so search works when the
window is closed and Atlas Launcher can use it. It replaces Baloo's file
indexer for file names and metadata. No content indexing.

- Bus name `net.eterneon.atlas.explorer.Search`, object
  `/net/eterneon/atlas/explorer/Search`, interface
  `net.eterneon.atlas.explorer.Search1`. D-Bus activated
  (`/usr/share/dbus-1/services/net.eterneon.atlas.explorer.Search.service`,
  `SystemdService=atlas-explorer-indexd.service`).
- The introspection XML is installed in `/usr/share/dbus-1/interfaces/`
  and kept in the repo at `data/dbus/net.eterneon.atlas.explorer.Search1.xml`.

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
  `~/.config/atlas-explorer/indexrc` (`[Index] Roots=`, `Exclude=`), a plain
  INI file.
- **Freshness with no idle CPU:** the service blocks on inotify (no timers,
  no polling). It watches every indexed folder up to a budget (64k watches
  or half of `fs.inotify.max_user_watches`, whichever is less). Folders past
  the budget are rechecked when a query arrives and the last check is older
  than 5 minutes: a walk that compares folder mtimes and rescans only the
  folders that changed, run at idle priority while the query is answered
  from the current index. On start it loads the last snapshot (answering at
  once) and does the same mtime walk.
- **Storage:** a snapshot file at `~/.cache/atlas-explorer/index/v1.idx`
  (folder 0700, file 0600, written atomically), a documented flat format:
  header with version, checksum and counts, then a string arena and fixed-size
  records. It is a cache: deleting it only costs a rescan. No private data
  leaves the user's cache folder.
- **CLI:** `atlas-explorer-search [--kind K] [--in DIR] [--modified 7d]
  [--larger 10M] [--smaller 1G] [--limit N] [--json] QUERY`, a thin client of
  the D-Bus API.
- **Service unit:** a systemd user unit, `Type=dbus`, `Nice=19`,
  `CPUSchedulingPolicy=idle`, `IOSchedulingClass=idle`, `MemoryHigh=96M`,
  `NoNewPrivileges=yes`, `PrivateNetwork=yes`, `ProtectSystem=strict`,
  `ReadWritePaths=%C/atlas-explorer`, `RestrictAddressFamilies=AF_UNIX`,
  `Restart=on-failure` with backoff. Not started at login; the first query
  starts it, and it stays to keep the watches.

## Consumed interfaces

| Peer | Interface (owner proposes, both review) | Used for |
|---|---|---|
| Atlas Archive | `net.eterneon.atlas.archive`, interface `net.eterneon.atlas.Archive1` (agreed 2026-10-05; Archive's DESIGN.md is the source) | "Extract Here", "Extract To…", "Compress to ZIP", "Compress…", drops of archive entries. Explorer never links libarchive |
| Atlas Backups (later) | `net.eterneon.atlas.backups`: a `ShowVersions(s uri)`-style deep link | "Restore Previous Versions" in the context menu and Properties, shown only when the service is installed (activatable name present) |
| Atlas Disks (later) | `net.eterneon.atlas.disks`: a `ShowDevice(s udisks_object_path)`-style deep link | "Open in Disks" for drives in the sidebar and in Properties, shown only when installed |
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
  `~/.local/state/atlas-explorer/journal` (0600, fsync'd). The file being
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
  text, shows invalid UTF-8 bytes as `\xNN`, and caps the shown length. Every
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

Logging: `atlas-framework-ui` logging to the journal as `atlas-explorer` and
`atlas-explorer-indexd`: every operation's start, end, error and recovery;
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
