#[cfg(not(target_arch = "wasm32"))]
pub mod args;
pub mod asset_bindings;
mod audio;
mod audio_observations;
mod block_entities;
mod block_selection;
mod block_use;
pub mod camera;

#[cfg(feature = "developer-control")]
#[cfg(not(target_arch = "wasm32"))]
mod developer_control;
#[cfg(feature = "enhanced-diagnostics")]
#[cfg(not(target_arch = "wasm32"))]
mod enhanced_diagnostics;
mod environment;
#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod first_run_tests;
#[cfg(not(target_arch = "wasm32"))]
mod frame_pacing;
#[cfg(not(target_arch = "wasm32"))]
mod global_resources;
mod hotbar;
#[cfg(not(target_arch = "wasm32"))]
mod hud_tools;
#[cfg(not(target_arch = "wasm32"))]
mod install_layout;
mod interaction_authority;
mod interaction_override;
mod item_use;
#[cfg(not(target_arch = "wasm32"))]
pub mod lifecycle;
pub mod local_player;
mod melee;
#[cfg(not(target_arch = "wasm32"))]
mod modding;
pub mod movement;
mod named_audio;
mod ordered_interactions;
pub mod startup_feedback;

mod particles;
mod pick_block;
#[cfg(not(target_arch = "wasm32"))]
mod present_mode;
mod primitive_shapes;
#[cfg(not(target_arch = "wasm32"))]
mod render_mode;
pub mod semantic_controls;
#[cfg(not(target_arch = "wasm32"))]
mod server_experiences;
#[cfg(not(target_arch = "wasm32"))]
mod session;
pub mod session_audio;
#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod session_cleanup_tests;
mod shader_pack_inputs;
mod survival_mining;
#[cfg(not(target_arch = "wasm32"))]
mod thread_budget;
#[cfg(feature = "tracy")]
#[cfg(not(target_arch = "wasm32"))]
mod tracy;
pub mod ui_runtime;
#[cfg(not(target_arch = "wasm32"))]
mod window_icon;

mod acceptance;
#[cfg(not(target_arch = "wasm32"))]
mod app;
mod presentation;
mod runtime;

#[cfg(not(target_arch = "wasm32"))]
pub use app::run;

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests;

#[cfg(test)]
#[global_allocator]
#[cfg(not(target_arch = "wasm32"))]
static ALLOCATOR: test_alloc::CountingAllocator = test_alloc::CountingAllocator;

#[cfg(not(target_arch = "wasm32"))]
mod far_terrain;
#[cfg(not(target_arch = "wasm32"))]
mod render_distance;
#[cfg(not(target_arch = "wasm32"))]
mod shell_adapters;
#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod shell_tests;

#[cfg(target_arch = "wasm32")]
pub mod browser;
#[cfg(any(target_arch = "wasm32", test))]
mod browser_input;

mod host_memory;

mod game_frame;

mod game_session;
