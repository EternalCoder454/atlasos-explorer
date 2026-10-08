//! Telamon Explorer's core, with no Qt and no KF6: everything that can be
//! decided from data alone, so it is tested without a display and shared by
//! the app's C++ adapters through one bridge. See docs/DESIGN.md, "Layout".

pub mod address;
pub mod display;
pub mod launch;
pub mod legacy;
pub mod location;
pub mod names;
pub mod places;
pub mod queue;
pub mod sort;
pub mod tabs;
pub mod undo;

pub use display::{MAX_DISPLAY_CHARS, NameBytes, display_name};
