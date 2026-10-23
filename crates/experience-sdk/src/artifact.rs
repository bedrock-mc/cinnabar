//! Stable layout shared by the server artifact writer and runtime loader.

/// The server artifact's generated manifest.
pub const MANIFEST_FILE: &str = "experience.toml";
/// The server guest's core WebAssembly module.
pub const SERVER_WASM: &str = "server.wasm";
/// Assets named by server registration are relative to this directory.
pub const ASSETS_DIR: &str = "assets";
/// The server block-data schema supported by current artifacts.
pub const DATA_SCHEMA: u32 = 1;
