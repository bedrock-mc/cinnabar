//! Distant terrain rendering, an opt-in Cinnabar extension: draws the sections the frame
//! hands over through [`render_scene::distant_terrain::DistantTerrainLink`], after opaque
//! terrain in the same reverse-Z target.
//!
//! - One geometry pool, sized from the device tier, the quality and the drawing distance and
//!   capped by the adapter's limits, holds every drawn section's packed biome grid and quads, sub-allocated
//!   per section; cluster records, section records and the per-frame lists live in fixed
//!   buffers beside it. They are allocated, with every pipeline compiled, on a thread of
//!   their own when drawing turns on, never grown, and freed when it turns off.
//! - Uploads stage one frame's writes with one queue write and apply them with one scatter
//!   dispatch, within a budget that grows from 2 MiB toward 8 MiB while frames are fast
//!   ([`budget`]); an update shows only once all its meshes are uploaded ([`table`]).
//! - Two-phase occlusion: pass A draws last frame's visible clusters; a depth pyramid over
//!   the near terrain and pass A ([`crate::chunk::HizPyramid`], tested with the terrain
//!   cull's `hiz_test.wgsl`) culls sections, then clusters by crack list, facing, resident
//!   chunk columns, frustum and Hi-Z, compacting them in order; pass B draws the newly
//!   visible ones. Water follows far to near ([`translucent`]). Each pass is one indexed
//!   indirect draw over a static 384-index buffer with vertices pulled from the pool.
//! - Every bit layout the shaders read is generated from its Rust source ([`layout`]).

mod budget;
mod gpu;
mod layout;
mod node;
mod one_build;
mod ranges;
mod readback;
mod renderer;
mod staging;
mod table;
mod translucent;
mod view;

#[cfg(test)]
mod app_tests;
#[cfg(test)]
mod gpu_feature_tests;
#[cfg(test)]
mod gpu_fixture;
#[cfg(test)]
mod gpu_tests;
#[cfg(test)]
mod test_meshes;

use bevy::{
    app::{App, Last, Plugin},
    ecs::schedule::IntoScheduleConfigs,
    render::{
        Render, RenderApp, RenderSystems,
        renderer::{RenderAdapter, RenderDevice},
    },
};
use render_api::{
    DistantTerrainQuality, DistantTerrainTier, distant_terrain::MIN_DISTANT_TERRAIN_CHUNKS,
};
use render_scene::distant_terrain::DistantTerrainLink;

pub use node::DistantTerrainPassLabel;
use renderer::{LodRenderer, prepare_distant_terrain, submit_distant_terrain_readback};

/// Installs the renderer; the app shares the [`DistantTerrainLink`] resource it inserts.
pub struct DistantTerrainRenderPlugin;

impl Plugin for DistantTerrainRenderPlugin {
    /// Hi-Z culling reads the main view's depth, which must be sampleable whether or not the
    /// near terrain's culling also asks for it.
    fn build(&self, app: &mut App) {
        app.init_resource::<DistantTerrainLink>()
            .add_systems(Last, crate::chunk::admit_depth_sampling);
    }

    /// The render device exists only once every plugin is built.
    fn finish(&self, app: &mut App) {
        let link = app.world().resource::<DistantTerrainLink>().clone();
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        let (Some(device), Some(adapter)) = (
            render_app.world().get_resource::<RenderDevice>().cloned(),
            render_app.world().get_resource::<RenderAdapter>(),
        ) else {
            return;
        };
        let limits = device.limits();
        let tier = device_tier(adapter.get_info().device_type, &limits);
        let supported = supported(adapter.get_downlevel_capabilities().flags, &limits, tier);
        bevy::log::info!(
            supported,
            max_distance_chunks = tier.max_distance_chunks,
            "distant terrain renderer"
        );
        render_app
            .insert_resource(LodRenderer::new(link, supported, tier))
            .add_systems(
                Render,
                (
                    prepare_distant_terrain
                        .in_set(RenderSystems::PrepareResources)
                        .after(bevy::core_pipeline::core_3d::prepare_core_3d_depth_textures),
                    submit_distant_terrain_readback.in_set(crate::device_poll::FrameSubmissions),
                ),
            );
        crate::device_poll::install(render_app);
        node::install_graph(render_app.world_mut());
    }
}

/// True when a device with `downlevel` capabilities and `limits` can cull and draw distant
/// terrain with `tier`'s smallest pool.
pub(crate) fn supported(
    downlevel: wgpu::DownlevelFlags,
    limits: &wgpu::Limits,
    tier: DistantTerrainTier,
) -> bool {
    use wgpu::DownlevelFlags as D;
    let smallest = tier.pool_bytes(DistantTerrainQuality::Low, MIN_DISTANT_TERRAIN_CHUNKS);
    downlevel.contains(D::COMPUTE_SHADERS | D::INDIRECT_EXECUTION | D::VERTEX_STORAGE)
        && limits.max_storage_buffers_per_shader_stage >= gpu::storage_buffers_per_stage()
        && layout::Capacity::new(smallest, limits).is_some()
}

/// The caps of a device of `device_type` with `limits`: the build's tier, or
/// [`DistantTerrainTier::LOW_END`] for a software or virtual adapter, or one whose storage
/// bindings cannot hold the build tier's Medium pool. Provisional: no low-end desktop
/// capture has measured where the line lies.
pub(crate) fn device_tier(
    device_type: wgpu::DeviceType,
    limits: &wgpu::Limits,
) -> DistantTerrainTier {
    let build = DistantTerrainTier::CURRENT;
    let binding = limits
        .max_storage_buffer_binding_size
        .min(limits.max_buffer_size);
    let medium = build.pool_bytes(DistantTerrainQuality::Medium, build.max_distance_chunks);
    let low_end = matches!(
        device_type,
        wgpu::DeviceType::Cpu | wgpu::DeviceType::VirtualGpu
    ) || binding < medium;
    if low_end {
        DistantTerrainTier::LOW_END
    } else {
        build
    }
}
