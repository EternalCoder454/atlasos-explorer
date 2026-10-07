# Telamon Explorer (Files)

The file manager of [Telamon OS](https://github.com/EternalCoder454/AtlasOS),
shown as **Files**. It replaces Dolphin: Windows 11's File Explorer and macOS
Finder, built on KDE's KIO.

- Tabs, a sidebar of your places, drives and network, an address bar that is
  a breadcrumb until you click it, and a command bar.
- Icons, list, details, columns and gallery views, a preview pane, a details
  pane, split view, and Quick Look on Space.
- Local drives, phones (MTP), SMB and SFTP shares, the Trash and every other
  KIO location; thumbnails from KIO's thumbnailers.
- One queue for copies, moves and deletes, with speed, time left, pause,
  cancel, a clear conflict dialog and undo. A full disk, a pulled drive or a
  crash never loses your files.
- Fast search with filters for kind, date and size, through its own light
  file-name index (`telamon-explorer-indexd`), which Atlas Launcher uses too.

Built with Rust, Qt 6 Quick and Kirigami on
[atlas-framework](https://github.com/EternalCoder454/atlas-framework). See
`CLAUDE.md` for how to build and test it, and `docs/DESIGN.md` for how it
works.

Licence: MIT.
