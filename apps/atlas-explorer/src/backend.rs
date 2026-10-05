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
use std::path::PathBuf;

#[derive(Default)]
pub struct BackendRust {}

impl qobject::Backend {
    pub fn activate(mut self: Pin<&mut Self>, args: &QStringList, cwd: &QString) {
        // Only what parse looks at is copied; the rest is counted.
        let total = args.len().max(0) as usize;
        let args: Vec<String> = args
            .iter()
            .take(launch::MAX_ARGS)
            .map(|a| a.to_string())
            .collect();
        let cwd = PathBuf::from(cwd.to_string());
        let mut launch = launch::parse(&args, &cwd);
        launch.dropped += total.saturating_sub(args.len());
        let mut lines: Vec<String> = launch
            .refused
            .iter()
            .map(|r| format!("{}: {}", r.arg, r.reason))
            .collect();
        if launch.dropped > 0 {
            lines.push(format!("{} more not looked at", launch.dropped));
        }
        if !lines.is_empty() {
            // {:?} keeps each argument on its line in the journal.
            log::warn!("ignored launch arguments: {lines:?}");
            self.as_mut()
                .refused(QString::from(lines.join("\n").as_str()));
        }
        let mut locations = QStringList::default();
        for l in &launch.locations {
            locations.append(QString::from(l.as_str()));
        }
        self.as_mut()
            .open(locations, launch.select, launch.new_window, launch.split);
    }
}
