# Telamon Files: security

Files replaces Dolphin on Telamon OS, so it opens other people's folders, drives,
shares and archives for every user. This file is the threat model, what the
code does about it, where the tests are, and what is left. It is written for
the Secure phase (F.S.R.P phase 2) and changes with the code: a change to a
defence below changes this file in the same commit. `docs/DESIGN.md` ("Trust")
has the rules in the form the code is held to; the audit's findings are at the
end.

## Assets

- The user's files: their contents, their modes and tags, what is in the Trash.
- The user's session: Files runs as the user with no privilege, but the user
  can start programs, so "run a program" is the worst outcome of everything
  below.
- The user's credentials: server passwords (KIO's password server keeps them,
  never Files), and what is in the clipboard.
- The window itself: a crash loses queued operations (there is no journal yet,
  Reliable phase).

## Who is trusted

| | Trust |
|---|---|
| The person at the keyboard | Trusted. What they type is checked for mistakes, not for malice. |
| Telamon OS packages (KIO workers, thumbnailers, Telamon Archive, the index service) | Trusted code that Files calls; their *answers* are treated as data. |
| Other programs of the user on the session bus | **Not** trusted with more than the interface offers: any of them may be a sandboxed app with a bus grant, or a compromised one. |
| File names, file contents, metadata, archive entries, `.trashinfo`, `.desktop` files, `.hidden`, drag payloads, clipboard, server listings and answers, Avahi/SMB names | **Untrusted**, wherever they come from (USB stick, download, share). |
| The settings file (`telamon-explorerrc`), `indexrc`, the index snapshot | Read as untrusted text: another program of the user, or a copy from elsewhere, may have written them. |

## Attacker-controlled inputs, and what stands between them and the user

| Input | Where it enters | Defence | Tests |
|---|---|---|---|
| File and folder names (control, bidi, zero-width, newline, invalid UTF-8, runs of spaces, very long) | every listing, search hit, Trash, server, archive, D-Bus | Shown only through `display_name` (controls, bidi, format characters and long runs of spaces made visible; a cut name keeps its end so the extension shows); every QML `Text`/`Label` sets `textFormat: Text.PlainText`; renames are checked (`/`, NUL, `.`/`..`, 255 bytes) | `display.rs`, `names.rs` tests; `tests/properties.rs`; `scripts/check-qml-plaintext.py` (CI) |
| Symlinks, hard links, races in copy, move, trash, delete | KIO jobs run by `OperationQueue` | Files starts KIO jobs; KIO's file worker writes to `.part` and renames, never writes through a link it created. Files' own file changes (modes, `.hidden`, Trash emptying, PDF/image output) open with `O_NOFOLLOW`/`O_PATH` and work through the descriptor | `attrs.rs`, `trash.rs`, `pdfmerge.rs` tests |
| Archives opened as folders (kio-extras `zip:`, `tar:`, `sevenz:`, `ar:`) | `ArchiveGuard` before any copy out | The whole extraction is refused when any entry is absolute, holds `..` (either separator), a drive letter, a NUL, is over 256 components, 4096 bytes, has a link with such a target or a target over 4096 bytes, or the archive lists over 1,000,000 entries; encrypted zips are not opened. Telamon Archive, which sandboxes the parser, is offered | `archive.rs` tests, `tests/properties.rs`, `fuzz/archive_entries` |
| `.desktop` files, scripts, executables | Open | `OpenUrlJob` with `setRunExecutables(false)` and KIO's "Run or open?" and untrusted-launcher prompts (default: do not run) | by KIO; Files never runs anything on its own |
| Service menus and custom actions | context menu | Service menus are KIO's. Custom actions are argument lists only: no shell, wrappers (`sh`, `env`, `sudo`, `systemd-run`, `xargs`, 40 names, also through links) refused as the program, a placeholder inside the code an interpreter is told to run is refused (`python -c '...%f...'`, `perl -lane`, an awk program; a heuristic for the usual forms, not a parser of every interpreter), the whole command is shown in "Ask first" | `actions.rs` tests, `fuzz/settings` |
| Thumbnails, metadata and previews | folder view, preview pane, Quick Look, Properties | Thumbnails: KIO `PreviewJob` out of process with its caps. Text preview: bounded reader on regular files only. Images: header only in process (`QImageReader::size`). Media: **not opened until Play is pressed** (Qt Multimedia parses on open). Details columns and Properties read KFileMetaData **in process** (opt-in columns) | `preview.rs` tests; left open: see below |
| Network shares and KIO URLs | address bar, Connect to Server, launch arguments, D-Bus | Server addresses are rebuilt from checked parts (`servers::build`), a typed or passed password is dropped (KIO asks), hosts and users that could read as options (`-oProxyCommand=…`) are refused, only the places Files browses may be typed (`admin:/`, `man:`, `applications:` are not), percent-encoded controls refused, credentials never stored, logged, shown or copied (`Copy Path` removes them). FTP, WebDAV and NFS are marked "Not encrypted" | `launch.rs`, `address.rs`, `servers.rs` tests, `tests/properties.rs`, `fuzz/addresses` |
| D-Bus callers of FileManager1 and `org.freedesktop.Application` | session bus | At most 64 arguments, each parsed by the launch rules, nothing opened or run, no server or device opened for a bus caller (`--bus`), `/MainApplication` (`quit()`, `closeAllWindows()`, `setStyleSheet()`) is not exported. `org.kde.KDBusService.CommandLine` (how a second launch forwards its arguments) cannot be told from a launch and so opens what a command line may open, still through the address and credential checks | `launch.rs` tests, `tests/gui/bus-surface.sh` |
| Drag and drop, clipboard | views, path bar, sidebar, tabs | Raw data (text, images) is saved only after the user names the file; URL lists are capped at 100,000 items per operation; a drop into an archive is refused | `OperationQueue` limit (C++) |
| `.trashinfo` and the mount table | Trash view, Restore, auto-empty | Strict parser, 64 KiB, `O_NOFOLLOW`, regular files only; trash folders must be real directories owned by the user with mode 0700; items are removed as links, never followed; a drive's Trash is not restored into hidden places of the home folder (the target is checked as written and with its links resolved, against both spellings of the home folder) | `trash.rs` tests, `fuzz/trashinfo` |
| Git repositories (Git badges) | folder view | Cleared environment, `GIT_DIR`/`GIT_WORK_TREE` pinned, filters/includes/hooks/fsmonitor off, repositories of other users skipped, 5 s, 16 MiB, `RLIMIT_AS/CPU/CORE`, own process group | `gitstatus.rs`, `childlimits.rs` tests |
| PDF and image tools (Combine, Rotate, Convert), Search Inside Files | context menu, search | `lopdf` and the JPEG code run in process behind size, page and object caps and a panic guard; `pdftotext` runs with `RLIMIT_AS/CPU/CORE`, a time and output cap, an absolute path and no environment | `pdfmerge.rs`, `imageops.rs`, `content.rs` tests |
| Settings, saved searches, actions, recents, view memory | rc file | Every list is parsed as untrusted, bounded, each record validated (a saved search's folder under the launch rules; ids that the text would refuse again are not handed out) | `saved.rs`, `actions.rs`, `home.rs`, `views.rs` tests, `fuzz/settings` |
| The index (`telamon-explorer-indexd`) and its snapshot | Search1 on the session bus, `~/.cache/telamon-explorer` | Snapshot: every length bounds-checked before allocation, limits that fit `MemoryMax` (160 MiB of names, 4M records, 384 MiB file), CRC for damage only, owner and mode checked, cache folder by descriptor; one huge folder cannot exhaust memory; `CACHEDIR.TAG`, `indexrc`, `recently-used.xbel` opened without blocking; unit: `NoNewPrivileges`, `PrivateNetwork`, `ProtectSystem=strict`, `AF_UNIX` only, `MemoryDenyWriteExecute`, `@system-service` minus `@privileged`, `KeyringMode=private`, `PrivateIPC`, `LimitCORE=0`, `UMask=0077` | `snapshot.rs`, `scan.rs`, `config.rs`, `walk.rs` tests, `fuzz/snapshot`; `%check` in the spec |

## Trust boundaries

1. **Files process ↔ everything it opens.** One process holds the UI, KIO's
   job delegates and the Rust core. Untrusted bytes reach it through KIO
   (listings, archive listings), `QImageReader` (headers), KFileMetaData
   (opt-in), `lopdf` and the JPEG code (on request), and `regex`.
   Mitigations: caps everywhere, `catch_unwind` on the FFI entries that read
   untrusted input (about 100 of the 220; the rest take numbers, flags and
   pointers from the program itself), resource-limited children for the programs it starts.
2. **Files ↔ the session bus.** FileManager1 and `org.freedesktop.Application`
   (in), Search1 and Archive1 (out). Callers are authenticated by nothing
   but "same user"; the interfaces are therefore weak by design (show, never
   do). Replies from Search1 and Archive1 are untrusted data (local `file:`
   URLs only, caps, display names).
3. **Index service ↔ the user's tree.** Reads everything the user can read,
   exposes names only to the session bus; sandboxed by its unit.
4. **Files ↔ KIO workers and thumbnailers.** Out of process; Files uses their
   answers as data.

## What Files does not defend against

- A process of the same user that can already do everything the user can
  (it can run `rm`). The bus interfaces limit what a *sandboxed or confused*
  caller gets, not a malicious program running unsandboxed as the user.
- Bugs in KIO, kio-extras, KArchive, Qt, KFileMetaData and the libraries they
  load, beyond what the caps and the out-of-process workers contain.
- A hostile server's *content* (it can send you any file; Files shows its
  names safely and never runs it).

## Build hardening

- RPM builds use Fedora's flags: `-fstack-protector-strong`,
  `_FORTIFY_SOURCE=3`, `-D_GLIBCXX_ASSERTIONS`, `-fstack-clash-protection`,
  `-fcf-protection`, PIE, `-z relro -z now` (the CMake C++ program); Rust
  programs link with rustc's PIE and full RELRO. The spec's `%check` runs
  `packaging/check-hardening.sh` on every installed program (PIE, RELRO with
  BIND_NOW, non-executable stack, no text relocations, no RPATH; the stack
  protector and fortified calls in the C++ program) and keeps the index unit's
  sandbox directives.
- Rust release profile: `overflow-checks = true` (sizes, counts and offsets
  come from outside). `panic` stays `unwind`: FFI entries and worker threads
  contain panics; an abort would end the program and lose queued operations.
- `cargo-deny` (advisories, licences, sources, bans; `deny.toml`) and the
  fuzz targets run in `.github/workflows/security.yml`, on changes and weekly.

## Testing

- `cargo test --workspace --locked` includes property tests
  (`crates/atlas-explorer-core/tests/properties.rs`, 2,000 cases each; CI
  runs 20,000) for the archive guard, link targets, `.trashinfo`, dates,
  restore rules, launch arguments, server addresses, display names, name
  checks and batch rename.
- `fuzz/` (cargo-fuzz, `cargo fuzz run --sanitizer none <target>` with
  `RUSTC_BOOTSTRAP=1` on the stable toolchain): `archive_entries`,
  `trashinfo`, `addresses`, `names`, `settings`, `snapshot`. CI runs each for
  20 s; run longer by hand.
- `tests/gui/bus-surface.sh`: what another bus client can reach.

## Findings of the Secure phase

See the pull request for the table with severities, fixes and tests; in short
(IDs are the PR's):

| | Severity | Finding | Fix |
|---|---|---|---|
| F1 | high | `split_ext` panicked on names like `日本.gz` / `façade.xz` (rename, batch rename, templates): the app aborted | byte comparison; property tests |
| F2 | medium | a panic in any FFI entry aborted the program | `guarded()` on the entries reading untrusted input |
| F3 | medium | selecting a media file made Qt Multimedia parse it | opened on first Play |
| F4 | medium | any bus peer could make Files open servers (hosts that read as ssh options, passwords kept) | `--bus`, address validation, passwords dropped |
| F5 | medium | `/MainApplication` exposed `quit()` and friends to the bus | not exported; `bus-surface.sh` |
| F6 | medium | index service could be OOM-killed on every start (snapshot limits, one huge folder) | limits fit `MemoryMax`; per-folder cap |
| F7 | medium | typed `admin:///…` and other schemes passed to KIO | allow-list |
| F8 | medium | recursive chmod followed links swapped in after the walk | `O_PATH|O_NOFOLLOW` handle |
| … | low | see the PR | |

## What is left (for later phases or other components)

- KFileMetaData (Details columns, Properties) and `QImageReader::size` still
  parse in the Files process; a helper process with a seccomp/Landlock profile
  is the fix. `lopdf` and the JPEG code are bounded and panic-contained but in
  process. Archives opened as folders are parsed in process by kio-extras.
- Files extracted from an archive through KIO keep the archive's mode bits
  (setuid and setgid included) and the room check trusts the sizes the archive
  declares. Telamon Archive does neither.
- `pdftotext` and `git` have resource limits but no Landlock/bubblewrap.
- Names `net.eterneon.telamon.Archive1` and `…Search` are not authenticated
  (any same-user process can own them first).
- Search1 `Refresh()`/`NotifyChanged()`/`Reload()` accept any session process.
- Recursive chmod opens the final path component without following links but
  the parent path is resolved by name.
- No operation journal; framework text components (`Telamon.Ui`) are plain
  text by default, a few (`TelamonNavigationStack` titles, `TelamonViewSwitcher`
  labels, header bars) do not set the format: Files does not feed them
  untrusted text; the framework should set it.
