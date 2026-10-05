//! The file index that replaces Baloo's for names and metadata (no
//! content). It is a library so the service (`atlas-explorer-indexd`), its
//! CLI and the tests share one implementation. See docs/DESIGN.md, "File
//! index".
//!
//! - [`index`]: the in-memory records and string arena.
//! - [`scan`]: builds and rebuilds an index from the file system.
//! - [`query`]: matching, filters, ranking.
//! - [`snapshot`]: the on-disk cache, read as untrusted input.
//! - [`engine`]: the worker thread with inotify, debounce and publishing.

pub mod category;
pub mod config;
pub mod engine;
pub mod index;
pub mod logger;
pub mod query;
pub mod recent;
pub mod scan;
pub mod snapshot;
pub mod sys;
pub mod text;
pub mod uri;
pub mod watch;

#[doc(hidden)]
pub mod testdir;

pub use engine::{Engine, EngineConfig, State, Status};
pub use query::{Hit, KindFilter, Options};
