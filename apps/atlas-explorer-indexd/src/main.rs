//! `atlas-explorer-indexd`: the file-name index service. It serves
//! `net.eterneon.atlas.explorer.Search1` on the session bus (see docs/DESIGN.md,
//! "File index"), is D-Bus activated, and keeps running to hold its watches.
//!
//! Start-up order: load the snapshot, claim the bus name, answer from the
//! snapshot while the scanner brings it up to date. With no usable snapshot the
//! name is claimed at once and `Search` answers with an empty array while
//! `Status` says "scanning". A call never waits on a scan.

mod options;
mod service;

use atlas_file_index::{Engine, EngineConfig, Status, logger};
use log::LevelFilter;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use zbus::blocking::connection;

/// Set when the index worker failed for good (see `Status::fatal`).
static FATAL: AtomicBool = AtomicBool::new(false);

pub const BUS_NAME: &str = "net.eterneon.atlas.explorer.Search";
pub const OBJECT_PATH: &str = "/net/eterneon/atlas/explorer/Search";
pub const INTERFACE: &str = "net.eterneon.atlas.explorer.Search1";

/// Block SIGTERM and SIGINT in this thread (and so in every thread started
/// later) so `wait_for_signal` can take them.
fn block_signals() {
    // SAFETY: sigset_t is plain data; the calls only read and write it.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGTERM);
        libc::sigaddset(&mut set, libc::SIGINT);
        libc::sigaddset(&mut set, libc::SIGHUP);
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
    }
}

fn wait_for_signal() -> i32 {
    // SAFETY: as above; sigwait writes the signal number to `sig`.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGTERM);
        libc::sigaddset(&mut set, libc::SIGINT);
        libc::sigaddset(&mut set, libc::SIGHUP);
        let mut sig = 0;
        libc::sigwait(&set, &mut sig);
        sig
    }
}

fn run() -> Result<(), String> {
    block_signals();
    #[allow(unused_mut)]
    let mut cfg = EngineConfig::from_env()?;
    // For the D-Bus tests: hold the first scan back so "scanning" can be
    // observed. Only in test builds (debug, or the `test-hooks` feature):
    // a release build does not read the variable.
    #[cfg(any(debug_assertions, feature = "test-hooks"))]
    if let Some(ms) = std::env::var("ATLAS_EXPLORER_TEST_SCAN_DELAY_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
    {
        cfg.scan_delay = std::time::Duration::from_millis(ms.min(60_000));
    }
    // StatusChanged goes out from its own thread, so a slow bus never stalls the scanner
    let (tx, rx) = mpsc::channel::<Status>();
    let engine = Arc::new(
        Engine::start(
            cfg,
            Arc::new(move |s: &Status| {
                let _ = tx.send(s.clone());
                if s.fatal {
                    // the index cannot recover: leave through the signal path,
                    // exit non-zero, and let systemd start a fresh service
                    FATAL.store(true, Ordering::SeqCst);
                    // SAFETY: kill on our own pid with a valid signal number.
                    unsafe { libc::kill(libc::getpid(), libc::SIGTERM) };
                }
            }),
        )
        .map_err(|e| format!("cannot start the index: {e}"))?,
    );

    // the snapshot is in memory: now claim the name
    let conn = connection::Builder::session()
        .and_then(|b| b.name(BUS_NAME))
        .and_then(|b| b.serve_at(OBJECT_PATH, service::Search1::new(engine.clone())))
        .and_then(|b| b.build())
        .map_err(|e| {
            format!("cannot serve {BUS_NAME} on the session bus (is another copy running?): {e}")
        })?;
    log::info!("serving {INTERFACE} as {BUS_NAME}");

    let emit_conn = conn.clone();
    let emitter = std::thread::Builder::new()
        .name("signals".into())
        .spawn(move || {
            for st in rx {
                let body = (service::status_map(&st),);
                if let Err(e) = emit_conn.emit_signal(
                    None::<&str>,
                    OBJECT_PATH,
                    INTERFACE,
                    "StatusChanged",
                    &body,
                ) {
                    log::warn!("StatusChanged not sent: {e}");
                }
            }
        })
        .map_err(|e| format!("cannot start the signal thread: {e}"))?;

    let sig = wait_for_signal();
    log::info!("signal {sig}, shutting down");
    engine.shutdown();
    drop(conn);
    drop(engine); // closes the status channel: the emitter ends
    let _ = emitter.join();
    if FATAL.load(Ordering::SeqCst) {
        return Err("the index failed and could not be restarted".into());
    }
    Ok(())
}

fn main() -> ExitCode {
    logger::init(LevelFilter::Info);
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            log::error!("{e}");
            ExitCode::FAILURE
        }
    }
}
