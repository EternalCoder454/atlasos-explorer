//! A minimal `log` logger for the service and the CLI: one line per record on
//! stderr (the journal collects the service's). The level comes from
//! `TELAMON_EXPLORER_LOG` (`error`, `warn`, `info` (default), `debug`, `trace`);
//! `ATLAS_EXPLORER_LOG`, its name before the rename, is read when that is not set.

use std::io::Write;

use log::{Level, LevelFilter, Log, Metadata, Record};

struct Stderr(LevelFilter);

impl Log for Stderr {
    fn enabled(&self, m: &Metadata) -> bool {
        m.level() <= self.0
    }

    fn log(&self, r: &Record) {
        if self.enabled(r.metadata()) {
            let tag = match r.level() {
                Level::Error => "error",
                Level::Warn => "warning",
                Level::Info => "info",
                Level::Debug => "debug",
                Level::Trace => "trace",
            };
            // a closed stderr must not panic the service
            let _ = writeln!(
                std::io::stderr(),
                "{}: {}: {}",
                r.target().split("::").next().unwrap_or("atlas"),
                tag,
                r.args()
            );
        }
    }

    fn flush(&self) {}
}

/// Install the logger; `default` is used when the variable is unset or bad.
pub fn init(default: LevelFilter) {
    let level = std::env::var("TELAMON_EXPLORER_LOG")
        .or_else(|_| std::env::var("ATLAS_EXPLORER_LOG"))
        .ok()
        .and_then(|v| v.parse::<LevelFilter>().ok())
        .unwrap_or(default);
    if log::set_boxed_logger(Box::new(Stderr(level))).is_ok() {
        log::set_max_level(level);
    }
}
