# Telamon Explorer (Telamon OS)

Rust + Qt 6.11 + Kirigami (CXX-Qt) file manager for Telamon OS, a Fedora
Kinoite 44 bootc image (repo `~/Documents/Projects/AtlasOS/AtlasOS`), with
thin C++ adapters over KF6 6.30 (KIO, Solid, KService). It replaces Dolphin
entirely, and shows as **Files**. It also owns the file-name index that
replaces Baloo's (`telamon-explorer-indexd`, D-Bus `net.eterneon.telamon.explorer.Search1`),
which Atlas Launcher uses.
Read `docs/DESIGN.md` first: it fixes the layout, the exported and consumed
interfaces, the threading rule, what is trusted, the failure modes and the
budgets. Change it only together with the code that implements the change.
The plan and roadmap are the Atlas Notes notes "AtlasOS/Explorer/Plan" and
"AtlasOS/Explorer/Roadmap".

The stack, build and look are the other Atlas apps' (Store, Wizard, Monitor).
When in doubt, do what they do, except for what atlas-framework provides
(startup, settings file, logging, crash reports), which Explorer takes from
there.

## Hard rules

- **Build and test inside the `fedora:44` dev container**, never on the host:
  `scripts/dev.sh <command>`. The repo is at `/src`; all build output goes to
  `/work` (`~/.cache/claude-builds/telamon-explorer` on the host), never into
  the repo or `/tmp`. Use a separate target dir per agent or task
  (`CARGO_TARGET_DIR=/work/target/<name> scripts/dev.sh ...`). Iterative
  compiles go through `~/.claude/heavy/run.sh` with `-j 8` at most; the first
  dev image build, full suites, RPMs and UI stress runs go to the "Telamon OS"
  coordinator session. run.sh stops a job after 4 hours (`HEAVY_TIMEOUT=<s>`
  for a longer one); every GUI launch, spawned process and wait in a test or
  script has its own timeout, so a hang fails instead of blocking the queue.
- **Never touch Zach's files.** Tests use generated trees and disk images
  under `/work`, with `HOME` and every `XDG_*` dir pointed there. Never the
  real home, drives, Trash, `user-places.xbel`, mounts or KIO settings.
- **Never run the GUI on the user's display.** Smoke runs use
  `QT_QPA_PLATFORM=offscreen`, or `xvfb-run -a -s "-screen 0 1920x1080x24"`,
  inside `dbus-run-session`. Real end-to-end tests happen in the Telamon OS test
  VM, which the Telamon OS session runs.
- **File names are untrusted input**, from any filesystem or server: shown
  only through `atlas_explorer_core` display names (controls, bidi and invalid
  UTF-8 made visible), and every QML `Text`/`Label` showing a name, path,
  file content, D-Bus or launch text sets `textFormat: Text.PlainText`.
- **Nothing runs on a double-click without the usual prompts**: executables
  get "Run or open?", untrusted `.desktop` files KIO's trust prompt.
- **No privilege.** No root, no polkit actions of Explorer's own, no
  `admin:/` (Zach, 2026-10-05). Mount, unmount and eject go through udisks2
  (Solid). Never call or extend `atlas-system-helper`.
- **The GUI thread never blocks.** KIO runs asynchronously on it, in
  batches; stat, statfs, MIME sniffing, image decoding, sorting, search,
  checksums and the journal's fsync run on workers, with results back through
  `qt_thread().queue` or queued signals.
- **One operation queue** runs every copy, move, link, trash, delete, rename,
  new folder, new file and hide (the `.hidden` file); nothing changes files
  outside it.
- **Telamon.Ui is the installed `telamon-ui` package** from atlas-framework
  (`~/Documents/Atlas Framework`, read-only from here). Never fork Telamon.Ui
  components into this repo: ask the "Telamon OS Framework" session. Pieces it
  hasn't shipped yet live in `qml/` with the requested API shape, and move
  upstream later.
- **Other apps' repos are read-only.** Archive's API is Archive's
  (`net.eterneon.atlas.archive`); Explorer never links libarchive. The
  Launcher consumes Search1: never change its signature without the Launcher
  session agreeing.
- Tests assert invariants and use fixtures or generated trees, never this
  machine's files.
- Commits are authored as
  `EternalHell <77252745+EternalCoder454@users.noreply.github.com>`. Commit
  only the paths you own (`git commit -- <paths>`). Don't push unless the
  lead asked.
- Licence: MIT. App ID `net.eterneon.telamon.explorer`, shown name Files.
  Wording follows KDE: Title Case buttons and titles, US spelling.

## Commands

| Task | Command (from the repo root on the host) |
|---|---|
| Format | `scripts/dev.sh cargo fmt --all --check` |
| Lint | `scripts/dev.sh cargo clippy --workspace --all-targets --locked -- -D warnings` |
| Tests | `scripts/dev.sh cargo test --workspace --locked` |
| App build | `scripts/dev.sh bash -c 'cmake -S apps/telamon-explorer -B /work/cmake/dev -G Ninja && cmake --build /work/cmake/dev -j 8'` |
| Smoke run | `scripts/dev.sh dbus-run-session -- env QT_QPA_PLATFORM=offscreen HOME=/work/home XDG_CONFIG_HOME=/work/home/.config XDG_DATA_HOME=/work/home/.local/share XDG_CACHE_HOME=/work/home/.cache XDG_STATE_HOME=/work/home/.local/state /work/cmake/dev/telamon-explorer` |
| RPM | `scripts/dev.sh packaging/build-rpm.sh /work/out` (builds HEAD; `ATLAS_RPM_WORKTREE=1` for the working tree) |
| Atlas checks | `scripts/dev.sh bash -c '"$ATLAS_FRAMEWORK/tools/lint-app.sh" . && "$ATLAS_FRAMEWORK/tools/check-app-names.sh" .'` |

`scripts/dev.sh` builds `localhost/telamon-explorer-dev:44` from
`ci/Containerfile` (target `dev`) on first use and whenever the Containerfile,
the spec's BuildRequires or the framework tag change; CI runs in the same
file's `ci` target, published as `ghcr.io/eternalcoder454/telamon-explorer-dev:44`.

## Moving the atlas-framework pin

1. Change `tag` in `Cargo.toml`, then
   `scripts/dev.sh cargo update -p telamon-framework-ui`.
2. Move the pin in `.github/workflows/ci.yml` if it names one, and when the
   app uses something new in Telamon.Ui, `ui:` in `apps/telamon-explorer/src/lib.rs`
   and `telamon-ui >=` in the spec (Requires and BuildRequires).
3. `scripts/dev.sh` rebuilds the dev image (the tag is part of its hash).
4. Commit `Cargo.toml` and `Cargo.lock` together.
