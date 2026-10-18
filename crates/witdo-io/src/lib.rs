//! File I/O: task files, the workspace layout, scanning, safe writes, trash and watching, and
//! profile configuration. All of witdo's file I/O goes through this crate.

pub mod file_name;
pub mod front_matter;
pub mod profile;
pub mod watch;
pub mod workspace;
pub mod write;
