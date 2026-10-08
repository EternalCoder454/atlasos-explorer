# Telamon Explorer (Files)

The file manager of [Telamon OS](https://github.com/EternalCoder454/AtlasOS),
shown as **Files**. It replaces Dolphin: Windows 11's File Explorer and macOS
Finder, built on KDE's KIO.

What works today (0.2.0 plus waves 1 to 5): tabs, each with one folder in Details, Icons and Compact
views, a breadcrumb path bar (click a segment, a chevron lists its subfolders, drop files on a segment, type a path with completion),
Back and Forward history menus, a status line (items, selection size, free space), search (type in the toolbar: ranked name matches from the index within a blink, This Folder or Everywhere, filters for kind, date and size, results as a Details view with a Path column, Open File Location, a live walk with Stop for folders the index doesn't hold), any KIO location (local, SMB, SFTP, Trash, Recent, Network and so on),
a sidebar built from KDE's places (Home and the standard folders, pins you drag in or add from the menu, drives with usage bars and an eject button, phones, Network, the Trash with its count and Empty Trash), sorting, a command bar with copy, cut, paste, rename
and trash, Quick Look (Space: a large preview of images, PDFs, video, audio, fonts, documents and text, arrow keys to browse, Enter to open), a preview pane (Alt+P), Columns and Gallery views, Group by, a view each folder remembers, Ctrl+scroll zoom, KIO's own conflict and delete dialogs with single-step undo, drag
and drop, the context menu with Open With and service menus, single instance,
`org.freedesktop.FileManager1`, and the file-name index service
(`telamon-explorer-indexd`) that Atlas Launcher uses.

Tabs (new, close, reopen, reorder, middle-click and Ctrl+Enter to open folders in the background, drop files on a tab, per-tab history, optional restore on start) are built. Planned (see `docs/ROADMAP.md`, in waves): one operation queue with a conflict dialog and undo/redo,
split view, archives, batch rename, tags and a
Settings page.

Built with Rust, Qt 6 Quick and Kirigami on
[atlas-framework](https://github.com/EternalCoder454/atlas-framework). See
`CLAUDE.md` for how to build and test it, and `docs/DESIGN.md` for how it
works.

Licence: MIT.
