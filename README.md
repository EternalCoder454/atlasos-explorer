# Telamon Explorer (Files)

The file manager of [Telamon OS](https://github.com/EternalCoder454/AtlasOS),
shown as **Files**. It replaces Dolphin: Windows 11's File Explorer and macOS
Finder, built on KDE's KIO.

What works today (0.2.0 plus waves 1 and 2): tabs, each with one folder in Details, Icons and Compact
views, a breadcrumb path bar (click a segment, a chevron lists its subfolders, drop files on a segment, type a path with completion),
Back and Forward history menus, a status line (items, selection size, free space), any KIO location (local, SMB, SFTP, Trash, Recent, Network and so on),
a sidebar of ten places, sorting, a command bar with copy, cut, paste, rename
and trash, KIO's own conflict and delete dialogs with single-step undo, drag
and drop, the context menu with Open With and service menus, single instance,
`org.freedesktop.FileManager1`, and the file-name index service
(`telamon-explorer-indexd`) that Atlas Launcher uses.

Tabs (new, close, reopen, reorder, middle-click and Ctrl+Enter to open folders in the background, drop files on a tab, per-tab history, optional restore on start) are built. Planned (see `docs/ROADMAP.md`, in waves): drives and pins in the sidebar, search with filters, Quick Look and
a preview pane, one operation queue with a conflict dialog and undo/redo,
columns and gallery views, split view, archives, batch rename, tags and a
Settings page.

Built with Rust, Qt 6 Quick and Kirigami on
[atlas-framework](https://github.com/EternalCoder454/atlas-framework). See
`CLAUDE.md` for how to build and test it, and `docs/DESIGN.md` for how it
works.

Licence: MIT.
