pub mod args;
pub mod asset_bindings;
mod audio;
mod audio_observations;
mod block_entities;
mod block_selection;
mod block_use;
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
mod window_icon;

mod acceptance;
mod app;
mod presentation;
mod runtime;

pub use app::run;

#[cfg(test)]
mod tests;

#[cfg(test)]
#[global_allocator]
static ALLOCATOR: test_alloc::CountingAllocator = test_alloc::CountingAllocator;

mod render_distance;
mod shell_adapters;
#[cfg(test)]
mod shell_tests;
