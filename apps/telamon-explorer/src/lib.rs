//! Rust side of Telamon Explorer. `cpp/main.cpp` starts Qt and the
//! single-instance service, and `cpp/kio/` adapts KF6; the app's state lives
//! here as QObjects exposed to QML, over the Qt-free `atlas-explorer-core`.

mod backend;
mod batch_ffi;
mod ffi;
mod home_ffi;
mod image_ffi;
mod ops_ffi;
mod preview_ffi;
mod props_ffi;
mod search_ffi;
mod trash_ffi;
mod views_ffi;

telamon_framework_ui::app! {
    name: "Files",
    id: "net.eterneon.telamon.explorer",
    repo: "atlasos-explorer",
    ui: "2.0.0",
}

use std::ffi::c_void;

/// Called once from `main.cpp`. Returns the `Backend` QObject, which C++ hands
/// to the QML engine. Ownership passes to the caller (a QObject with no parent).
#[unsafe(no_mangle)]
pub extern "C" fn telamon_backend_new() -> *mut c_void {
    backend::qobject::backend_make_unique().into_raw().cast()
}

/// Called once from `main.cpp`, before anything reads a setting: what
/// Explorer kept under its old name (`atlas-explorer`) comes over to the new
/// one, once, and nothing is replaced. Settings are the framework's (the
/// old `atlas-explorerrc` is copied to `telamon-explorerrc`); the file-index
/// settings (`indexrc`) and the index snapshot are moved. Failures are logged
/// and never stop the app.
#[unsafe(no_mangle)]
pub extern "C" fn telamon_adopt_legacy() {
    use atlas_explorer_core::legacy;
    use std::path::PathBuf;

    // Reading the settings file of this app is what makes the framework copy
    // the old one.
    let rc = telamon_framework_ui::telamon_framework_core::settings::Settings::for_app(
        telamon_framework_ui::app_info(),
    );
    log::debug!("settings: {}", rc.path().display());
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|h| h.is_absolute());
    let xdg = |var: &str, default: &str| {
        std::env::var_os(var)
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| home.as_ref().map(|h| h.join(default)))
    };
    if let Some(dir) = xdg("XDG_CONFIG_HOME", ".config") {
        match legacy::adopt_config(&dir) {
            Ok(true) => log::info!(
                "index settings: moved {}/atlas-explorer to telamon-explorer",
                dir.display()
            ),
            Ok(false) => {}
            Err(e) => log::warn!(
                "index settings: could not move {}/atlas-explorer: {e}",
                dir.display()
            ),
        }
    }
    if let Some(dir) = xdg("XDG_CACHE_HOME", ".cache") {
        match legacy::adopt_cache(&dir) {
            Ok(true) => log::info!(
                "index: moved {}/atlas-explorer to telamon-explorer",
                dir.display()
            ),
            Ok(false) => {}
            Err(e) => log::warn!(
                "index: could not move {}/atlas-explorer: {e}",
                dir.display()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// What atlas-explorer kept comes over, whole and once. The only test of
    /// this crate that sets the environment.
    #[test]
    fn adopts_what_atlas_explorer_kept() {
        let dir =
            std::env::temp_dir().join(format!("telamon-explorer-adopt-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let (config, cache) = (dir.join("config"), dir.join("cache"));
        fs::create_dir_all(config.join("atlas-explorer")).unwrap();
        fs::create_dir_all(cache.join("atlas-explorer/index")).unwrap();
        let rc = "[View]\nShowHidden=true\n";
        fs::write(config.join("atlas-explorerrc"), rc).unwrap();
        fs::write(
            config.join("atlas-explorer/indexrc"),
            "[Index]\nRoots=/srv/fixture\n",
        )
        .unwrap();
        fs::write(
            cache.join("atlas-explorer/index/v1.idx"),
            b"fixture snapshot",
        )
        .unwrap();
        // SAFETY: no other test in this crate reads or sets the environment.
        unsafe {
            std::env::set_var("HOME", &dir);
            std::env::set_var("XDG_CONFIG_HOME", &config);
            std::env::set_var("XDG_CACHE_HOME", &cache);
        }

        telamon_adopt_legacy();

        // The settings were copied by the framework: the keys are as they were.
        let new = fs::read_to_string(config.join("telamon-explorerrc")).unwrap();
        assert!(new.contains("[View]\nShowHidden=true\n"), "{new}");
        assert_eq!(
            fs::read_to_string(config.join("atlas-explorerrc")).unwrap(),
            rc
        );
        // The index settings and the snapshot were moved, with nothing left behind.
        assert_eq!(
            fs::read_to_string(config.join("telamon-explorer/indexrc")).unwrap(),
            "[Index]\nRoots=/srv/fixture\n"
        );
        assert!(!config.join("atlas-explorer").exists());
        assert_eq!(
            fs::read(cache.join("telamon-explorer/index/v1.idx")).unwrap(),
            b"fixture snapshot"
        );
        assert!(!cache.join("atlas-explorer").exists());

        // A second start changes nothing, and what the new files say stays.
        fs::write(
            config.join("telamon-explorerrc"),
            "[View]\nShowHidden=false\n",
        )
        .unwrap();
        fs::create_dir_all(config.join("atlas-explorer")).unwrap();
        fs::write(
            config.join("atlas-explorer/indexrc"),
            "[Index]\nRoots=/stale\n",
        )
        .unwrap();
        telamon_adopt_legacy();
        assert_eq!(
            fs::read_to_string(config.join("telamon-explorerrc")).unwrap(),
            "[View]\nShowHidden=false\n"
        );
        assert_eq!(
            fs::read_to_string(config.join("telamon-explorer/indexrc")).unwrap(),
            "[Index]\nRoots=/srv/fixture\n"
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
