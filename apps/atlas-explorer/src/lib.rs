//! Rust side of Atlas Explorer. `cpp/main.cpp` starts Qt and the
//! single-instance service, and `cpp/kio/` adapts KF6; the app's state lives
//! here as QObjects exposed to QML, over the Qt-free `atlas-explorer-core`.

mod backend;

atlas_framework_ui::app! {
    name: "Files",
    id: "net.eterneon.atlas.explorer",
    repo: "atlasos-explorer",
    ui: "1.4.0",
}

use std::ffi::c_void;

/// Called once from `main.cpp`. Returns the `Backend` QObject, which C++ hands
/// to the QML engine. Ownership passes to the caller (a QObject with no parent).
#[unsafe(no_mangle)]
pub extern "C" fn atlas_backend_new() -> *mut c_void {
    backend::qobject::backend_make_unique().into_raw().cast()
}
