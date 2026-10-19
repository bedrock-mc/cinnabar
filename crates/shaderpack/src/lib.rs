//! Java Edition OptiFine/Iris shader-pack loading from folders, ZIP archives or memory.
//!
//! Open a pack with [`PackSource::open`], inspect its sorted text with [`PackSource::files`]
//! and read binary assets with [`PackSource::read_bytes`]. Filesystem and archive ingestion
//! enforce finite byte, entry, path and archive work budgets.
//!
//! This crate currently loads source files only. Configuration parsing, shader translation
//! and render-host integration remain unimplemented.

mod source;

pub use source::{PackError, PackOrigin, PackSource};
