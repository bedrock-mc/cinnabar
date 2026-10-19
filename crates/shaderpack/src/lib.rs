//! Java Edition OptiFine/Iris shader-pack loading from folders, ZIP archives or memory.
//!
//! Configuration parsing, shader translation and render-host integration are not implemented yet.

mod source;

pub use source::{PackError, PackOrigin, PackSource};
