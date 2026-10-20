//! Experimental "Shader pack" render mode: runs a Java OptiFine/Iris shader pack, translated
//! by the `shaderpack` crate, on Cinnabar's world. Compiled with the `shaderpacks` feature;
//! without it [`ShaderPackRenderPlugin`] does nothing.
//!
//! Flow: the app writes `ShaderPackRequest`; a worker thread translates the pack and builds
//! raw wgpu pipelines from the naga modules (see `pipelines`), publishing `ShaderPackStatus`
//! and the loaded pack. Cameras marked `ShaderPackRendering` then run the pack's full-screen
//! programs as Iris does (see `passes`) over the Vanilla scene, which is copied into
//! `colortex0` and `depthtex*`: `prepare*`, `deferred*`, `composite*` and `final`, before
//! tonemapping and the UI. The block atlas (`atlas`) and the terrain geometry adapter
//! (`geometry`) are not drawn yet.
//!
//! Conventions: pack buffers are stored bottom-up and gamma-encoded as in OpenGL Minecraft
//! (the translator flips clip-space Y and remaps clip depth), uniforms use OpenGL matrices,
//! and each texture is read with the sampler class Iris gives its name.

#[cfg(feature = "shaderpacks")]
pub(crate) mod atlas;
#[cfg(feature = "shaderpacks")]
mod dispatch;
#[cfg(feature = "shaderpacks")]
pub(crate) mod geometry;
#[cfg(feature = "shaderpacks")]
mod host_pipelines;
#[cfg(feature = "shaderpacks")]
mod ids;
#[cfg(feature = "shaderpacks")]
mod loader;
#[cfg(feature = "shaderpacks")]
mod passes;
#[cfg(feature = "shaderpacks")]
mod pipeline_cache;
#[cfg(feature = "shaderpacks")]
mod pipelines;
#[cfg(feature = "shaderpacks")]
mod plan;
#[cfg(feature = "shaderpacks")]
pub(crate) mod plugin;
#[cfg(feature = "shaderpacks")]
mod resources;
#[cfg(feature = "shaderpacks")]
mod scene_bindings;
#[cfg(feature = "shaderpacks")]
mod targets;
#[cfg(all(test, feature = "shaderpacks"))]
mod tests;
#[cfg(feature = "shaderpacks")]
mod uniform_audit;
#[cfg(feature = "shaderpacks")]
#[expect(
    dead_code,
    reason = "per-draw uniforms are consumed by the pack's geometry and actor draws"
)]
mod uniforms;

use bevy::prelude::{App, Plugin};

/// Loads the requested shader pack and runs it on opted-in cameras.
#[derive(Debug, Clone, Copy, Default)]
pub struct ShaderPackRenderPlugin;

impl Plugin for ShaderPackRenderPlugin {
    #[cfg(feature = "shaderpacks")]
    fn build(&self, app: &mut App) {
        plugin::build(app);
    }

    #[cfg(not(feature = "shaderpacks"))]
    fn build(&self, _app: &mut App) {}

    #[cfg(feature = "shaderpacks")]
    fn finish(&self, app: &mut App) {
        plugin::finish(app);
    }
}
