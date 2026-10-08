//! `telamon-explorer-search`: command-line client of the file index
//! (`net.eterneon.telamon.explorer.Search1` over the session bus).
//!
//!   telamon-explorer-search [--kind K] [--in DIR] [--modified 7d] [--larger 10M]
//!                         [--smaller 1G] [--tag NAME] [--limit N] [--json] QUERY
//!
//! File names are untrusted: they are printed as display names (controls and
//! bidi characters made visible), never raw.

mod args;
mod output;

use args::{Args, Parsed};
use atlas_file_index::logger;
use log::LevelFilter;
use std::collections::HashMap;
use std::process::ExitCode;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedValue, Value};

const BUS_NAME: &str = "net.eterneon.telamon.explorer.Search";
const OBJECT_PATH: &str = "/net/eterneon/telamon/explorer/Search";
const INTERFACE: &str = "net.eterneon.telamon.explorer.Search1";

type Hit = (String, String, String, String, String, i64, u64, f64);

fn options(a: &Args) -> HashMap<String, Value<'static>> {
    let mut o: HashMap<String, Value<'static>> = HashMap::new();
    if let Some(k) = &a.kind {
        o.insert("kind".into(), Value::from(k.clone()));
    }
    if !a.kinds.is_empty() {
        o.insert("kinds".into(), Value::new(a.kinds.clone()));
    }
    if let Some(uri) = &a.root_uri {
        o.insert("root".into(), Value::from(uri.clone()));
    }
    if let Some(t) = a.modified_after {
        o.insert("modified_after".into(), Value::from(t));
    }
    if let Some(n) = a.size_min {
        o.insert("size_min".into(), Value::from(n));
    }
    if let Some(n) = a.size_max {
        o.insert("size_max".into(), Value::from(n));
    }
    if let Some(t) = &a.tag {
        o.insert("tag".into(), Value::from(t.clone()));
    }
    o
}

fn run(a: &Args) -> Result<(), String> {
    let conn = Connection::session().map_err(|e| format!("cannot reach the session bus: {e}"))?;
    let proxy = Proxy::new(&conn, BUS_NAME, OBJECT_PATH, INTERFACE)
        .map_err(|e| format!("cannot reach the file index service: {e}"))?;
    let hits: Vec<Hit> = proxy
        .call("Search", &(a.query.as_str(), a.limit, options(a)))
        .map_err(|e| format!("the file index service did not answer the search: {e}"))?;
    let mut out = std::io::stdout().lock();
    output::write(&mut out, &hits, a.json).map_err(|e| format!("cannot write the results: {e}"))?;
    if hits.is_empty() && !a.json {
        // an empty answer while the first scan runs is not "nothing found"
        if let Ok(st) = proxy.call::<_, _, HashMap<String, OwnedValue>>("Status", &()) {
            let state = st
                .get("state")
                .and_then(|v| <&str>::try_from(v).ok())
                .unwrap_or("");
            if state == "scanning" || state == "stale" {
                eprintln!(
                    "telamon-explorer-search: the index is still being built, so results may be missing"
                );
            } else if state == "disabled" {
                eprintln!(
                    "telamon-explorer-search: the file index is turned off (no folders to index in indexrc)"
                );
            } else if state == "error" {
                eprintln!(
                    "telamon-explorer-search: the file index has a problem and cannot search"
                );
            }
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    logger::init(LevelFilter::Warn);
    match args::parse(std::env::args_os().skip(1)) {
        Ok(Parsed::Help) => {
            print!("{}", args::USAGE);
            ExitCode::SUCCESS
        }
        Ok(Parsed::Run(a)) => match run(&a) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("telamon-explorer-search: {e}");
                ExitCode::from(1)
            }
        },
        Err(e) => {
            eprintln!("telamon-explorer-search: {e}\nTry 'telamon-explorer-search --help'.");
            ExitCode::from(2)
        }
    }
}
