//! Telamon Explorer's core, with no Qt and no KF6: everything that can be
//! decided from data alone, so it is tested without a display and shared by
//! the app's C++ adapters through one bridge. See docs/DESIGN.md, "Layout".

pub mod actions;
pub mod address;
pub mod archive;
pub mod attrs;
pub mod batch;
pub mod checksum;
pub mod childlimits;
pub mod conflict;
pub mod content;
pub mod display;
pub mod foldersize;
pub mod gitstatus;
pub mod group;
pub mod history;
pub mod home;
pub mod imageops;
pub mod launch;
pub mod legacy;
pub mod location;
pub mod menu;
pub mod menuprefs;
pub mod names;
pub mod optext;
pub mod pattern;
pub mod pdfmerge;
pub mod perms;
pub mod places;
pub mod preflight;
pub mod preview;
pub mod queue;
pub mod saved;
pub mod search;
pub mod servers;
pub mod sort;
pub mod tabs;
pub mod tags;
pub mod trash;
pub mod undo;
pub mod views;
pub mod xattr;
pub mod zoom;

pub use display::{MAX_DISPLAY_CHARS, NameBytes, display_name};
