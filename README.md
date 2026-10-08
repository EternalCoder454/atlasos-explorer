# Telamon Files

The file manager of [Telamon OS](https://github.com/EternalCoder454/AtlasOS),
shown as **Files** (the crate and binary are `telamon-explorer`). It replaces
Dolphin: the simple parts of Windows 11's File Explorer and macOS Finder, on
KDE's KIO. Rust, Qt 6 Quick and the Telamon.Ui look (from
[atlas-framework](https://github.com/EternalCoder454/atlas-framework)), with thin
C++ adapters over KIO, Solid and KService.

The Functionable phase is complete (waves 1 to 16). What Files does today:

**Browsing**
- Tabs (new, close, reopen, reorder, duplicate, drag files onto a tab, optional
  restore on start) and a split view of two folders with Copy and Move to the
  other pane.
- A path bar you click through (a menu per folder, drops on a segment) that
  becomes a text field with completion; Back and Forward with history menus;
  a status line with items, selection size and free space.
- Details, Icons, Compact, Columns and Gallery views, Group by, a view each
  folder remembers (or one for all), zoom, Quick Look on Space, a preview pane.
- A sidebar of KDE's places: Home and the standard folders, pins, drives with
  usage bars and eject, phones, Network, tags, saved searches and the Trash.
- Any KIO location (local, SMB, SFTP, FTP, WebDAV ...), Connect to Server, a
  Home page (pinned, recent and frequent) and a Network page; zip, tar and 7z
  files open as read-only folders.

**Doing things**
- Copy, move, link, trash and delete through one queue with a progress ring,
  conflict dialog, problems dialog, undo and redo; drag and drop.
- Rename in place, Batch Rename (find and replace, numbers, case, text), new
  folders and files from templates, Properties with permissions, checksums, tags
  and a star rating, quick actions on pictures (rotate, convert, combine to PDF),
  extract and compress through Telamon Archive.
- The Trash: Restore, Empty Trash, original place and date deleted, and
  "Empty items older than N days".

**Finding things**
- A search field with ranked name matches from the file index within a blink
  (`telamon-explorer-indexd`, which Atlas Launcher also uses), filters for kind,
  date, size and tag, a live walk for folders the index doesn't hold, search
  inside text files and PDFs, name patterns (regular expressions), saved
  searches, and a filter for the folder shown (Ctrl+F).

**Yours** (one Settings window, Ctrl+,)
- Five short pages: General, View, Search, Context Menu and Actions, Trash.
- Your own actions under More Actions: a program, arguments with `%f %F %u %U %d`,
  the file types, ask first. They are started with an argument list, never a shell.
- A switch for every context-menu entry and every service menu.
- Git status badges on modified, new and ignored files (off by default).
- Keyboard-only use, a screen-reader description on every row, right-to-left
  layouts, reduced motion and high contrast.

See `docs/DESIGN.md` for how it works, `docs/ROADMAP.md` for what each wave did
and what is left for the Secure, Reliable and Performant phases, and `CLAUDE.md`
for how to build and test it (in the dev container, never on the host).

Licence: MIT.
