//! The window's entry point: what a launch asks for. `main.cpp` calls
//! `activate` with the first launch's arguments and with each forwarded
//! second launch's (and, later, FileManager1's); QML listens to `open` and
//! opens the locations.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;
    }

    extern "RustQt" {
        #[qobject]
        #[namespace = "atlas_explorer"]
        type Backend = super::BackendRust;

        /// Handles a launch's arguments (without the program name), relative
        /// paths read against `cwd` (empty for D-Bus callers).
        #[qinvokable]
        fn activate(self: Pin<&mut Backend>, args: &QStringList, cwd: &QString);

        /// Locations to open (`file://` or other KIO URLs, in order): in new
        /// tabs, or a new window; `select` opens each one's parent with it
        /// selected; `split` puts the first two side by side.
        #[qsignal]
        fn open(
            self: Pin<&mut Backend>,
            locations: QStringList,
            select: bool,
            new_window: bool,
            split: bool,
        );

        /// FileManager1's ShowItemProperties: parses `args` like a launch and
        /// emits `inspected` with the locations; nothing is opened or run.
        #[qinvokable]
        fn inspect(self: Pin<&mut Backend>, args: &QStringList);

        /// Locations whose Properties dialog was asked for.
        #[qsignal]
        fn inspected(self: Pin<&mut Backend>, locations: QStringList);

        /// What a launch asked for that Explorer won't do, one line per
        /// argument ("argument: reason"), already made safe to show as plain
        /// text.
        #[qsignal]
        fn refused(self: Pin<&mut Backend>, text: QString);
    }

    impl cxx_qt::Threading for Backend {}

    #[namespace = "rust::cxxqtlib1"]
    unsafe extern "C++" {
        include!("cxx-qt-lib/common.h");

        #[cxx_name = "make_unique"]
        fn backend_make_unique() -> UniquePtr<Backend>;
    }
}

use atlas_explorer_core::launch;
use core::pin::Pin;
use cxx_qt_lib::{QString, QStringList};
use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct BackendRust {}

/// Reads a launch's arguments (the Rust side looks at `MAX_ARGS`, counting
/// the rest) and the lines to show for what was refused.
fn read_launch(args: &QStringList, cwd: &Path) -> (launch::Launch, Vec<String>) {
    let total = args.len().max(0) as usize;
    let args: Vec<String> = args
        .iter()
        .take(launch::MAX_ARGS)
        .map(|a| a.to_string())
        .collect();
    let mut launch = launch::parse(&args, cwd);
    launch.dropped += total.saturating_sub(args.len());
    let mut lines: Vec<String> = launch
        .refused
        .iter()
        .map(|r| format!("{}: {}", r.arg, r.reason))
        .collect();
    if launch.dropped > 0 {
        lines.push(format!("{} more not looked at", launch.dropped));
    }
    (launch, lines)
}

fn to_list(locations: &[String]) -> QStringList {
    let mut list = QStringList::default();
    for l in locations {
        list.append(QString::from(l.as_str()));
    }
    list
}

impl qobject::Backend {
    pub fn inspect(mut self: Pin<&mut Self>, args: &QStringList) {
        let (launch, lines) = read_launch(args, Path::new(""));
        if !lines.is_empty() {
            log::warn!("ignored properties arguments: {lines:?}");
            self.as_mut()
                .refused(QString::from(lines.join("\n").as_str()));
        }
        if !launch.locations.is_empty() {
            self.as_mut().inspected(to_list(&launch.locations));
        }
    }

    pub fn activate(mut self: Pin<&mut Self>, args: &QStringList, cwd: &QString) {
        let cwd = PathBuf::from(cwd.to_string());
        let (launch, lines) = read_launch(args, &cwd);
        if !lines.is_empty() {
            // {:?} keeps each argument on its line in the journal.
            log::warn!("ignored launch arguments: {lines:?}");
            self.as_mut()
                .refused(QString::from(lines.join("\n").as_str()));
        }
        self.as_mut().open(
            to_list(&launch.locations),
            launch.select,
            launch.new_window,
            launch.split,
        );
    }
}
