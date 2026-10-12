// Browser builds leave out the native launcher, Discord and installer flows; the code they share
// with the browser still compiles there, so it is reported unused only in that build.
#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

pub mod args;
pub mod asset_bindings;
mod audio;
mod audio_observations;
mod block_entities;
mod block_selection;
mod block_use;
#[cfg(target_arch = "wasm32")]
mod browser_links;
pub mod camera;

#[cfg(feature = "developer-control")]
mod developer_control;
#[cfg(feature = "enhanced-diagnostics")]
mod enhanced_diagnostics;
mod environment;
#[cfg(test)]
mod first_run_tests;
mod frame_pacing;
mod global_resources;
mod hotbar;
mod hud_tools;
mod install_layout;
mod interaction_authority;
mod item_use;
pub mod lifecycle;
pub mod local_player;
mod melee;
mod modding;
pub mod movement;
mod named_audio;

mod particles;
mod pick_block;
mod present_mode;
mod primitive_shapes;
mod render_mode;
pub mod semantic_controls;
mod server_experiences;
mod session;
pub mod session_audio;
#[cfg(test)]
mod session_cleanup_tests;
pub mod startup_progress;
#[allow(
    dead_code,
    unused_imports,
    reason = "consumed by the store screens as they land"
)]
mod store;
mod survival_mining;
mod thread_budget;
#[cfg(feature = "tracy")]
mod tracy;
pub mod ui_runtime;
#[cfg(not(target_arch = "wasm32"))]
mod window_icon;

mod acceptance;
mod app;
mod presentation;
mod runtime;

pub use app::{CoreLink, run, run_with};

/// Imports the resource pack file `name`, whose `bytes` the player dropped on a browser page,
/// into the Global Resources library.
#[cfg(target_arch = "wasm32")]
pub fn import_resource_pack(name: String, bytes: Vec<u8>) {
    global_resources::import_dropped(name, bytes);
}

/// The file extensions a resource pack import accepts.
#[must_use]
pub fn resource_pack_extensions() -> &'static [&'static str] {
    &resource_pack::PACK_IMPORT_EXTENSIONS
}

#[cfg(test)]
mod tests;

mod render_distance;
mod shell_adapters;
#[cfg(test)]
mod shell_tests;
