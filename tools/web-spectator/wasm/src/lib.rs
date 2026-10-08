//! Read-only browser spectator using Cinnabar's native renderer and compiled assets.
//!
//! The WebGPU viewer owns native terrain, actors and HUD drawing. It never opens
//! a game connection or accepts gameplay input. The old flat mesh binding remains
//! a diagnostic geometry tool only and is not a viewer fallback.

#[cfg(any(target_arch = "wasm32", test))]
mod browser_entity_catalog;
#[cfg(any(target_arch = "wasm32", test))]
mod browser_hud_playback;
#[cfg(any(target_arch = "wasm32", test))]
mod browser_interpolation;
#[cfg(any(target_arch = "wasm32", test))]
// Host interpolation tests parse the same model without using graphics-only fields.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
mod browser_model;
mod canonical;
mod geometry;
mod materials;
mod model;
mod terrain;
#[cfg(any(target_arch = "wasm32", test))]
mod terrain_runtime;
mod textured_assets;

#[cfg(target_arch = "wasm32")]
mod browser;
#[cfg(target_arch = "wasm32")]
mod browser_actor;
#[cfg(target_arch = "wasm32")]
mod browser_audio;
#[cfg(target_arch = "wasm32")]
mod browser_camera;
#[cfg(target_arch = "wasm32")]
mod browser_diagnostics;
#[cfg(target_arch = "wasm32")]
mod browser_effects;
#[cfg(target_arch = "wasm32")]
mod browser_hud;
#[cfg(target_arch = "wasm32")]
pub use browser_audio::SoundRoutes;
#[cfg(target_arch = "wasm32")]
mod browser_items;

#[cfg(target_arch = "wasm32")]
pub use browser::Viewer;
pub use textured_assets::TerrainAssets;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod textured_tests;

/// Builds triangle vertices in world coordinates from a bounded arena snapshot.
///
/// Each vertex has nine floats: position XYZ, normal XYZ, linear color RGB.
/// The JavaScript binding returns a `Float32Array` and throws on invalid input.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub fn mesh_arena(input: &str) -> Result<Vec<f32>, String> {
    let arena = model::Arena::parse(input)?;
    terrain::mesh(&arena)
}
