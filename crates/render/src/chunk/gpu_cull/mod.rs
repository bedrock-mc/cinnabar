//! GPU-driven opaque terrain culling: persistent per-slot records, a compute cull with
//! two-phase Hi-Z occlusion, and compacted multi-draw-indirect submission.
//!
//! Count-capable backends consume the compacted count directly. Other indirect backends
//! submit cleared fixed-size regions whose unused commands draw zero instances. Direct-draw
//! devices (Metal) instead read occlusion bits back for later frames to skip; see [`direct`].

#[cfg(test)]
pub(super) mod app_tests;
mod direct;
#[cfg(test)]
mod direct_tests;
pub(in crate::chunk) mod kernels;
pub(in crate::chunk) mod model;
mod node;
pub(in crate::chunk) mod occlusion;
mod prepare;
mod slots;
#[cfg(test)]
mod tests;

use bevy::{
    camera::{primitives::Frustum, visibility::RenderLayers},
    render::{
        ExtractSchedule, render_phase::DrawFunctionId, render_resource::CachedRenderPipelineId,
    },
};

use crate::chunk::*;
pub(crate) use direct::TerrainPassLabel;
use direct::{DirectOcclusion, direct_occlusion_supported, reset_direct_occlusion_frame};
pub(in crate::chunk) use direct::{DirectOcclusionFrame, SkipOccludedTerrain};
use model::STREAM_COUNT;
pub(crate) use node::GpuCullLateLabel;
pub(in crate::chunk) use node::{DrawGpuCulledCommands, draw_function_ids, install_commands};
use prepare::{ChunkHiddenEntities, GpuCull, extract_hidden_chunks, prepare_gpu_cull};

/// Forces the CPU culling path for A/B measurement.
const CPU_CULLING_ENV: &str = "RUST_MCBE_CPU_CULLING";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::chunk) enum GpuCullSubmission {
    Count,
    Fixed,
}

/// Whether opaque terrain is culled on the GPU on this device.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::chunk) struct GpuCullSupport(pub(in crate::chunk) bool);

/// Whether direct terrain draws skip what read-back occlusion bits hid.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::chunk) struct DirectOcclusionSupport(pub(in crate::chunk) bool);

pub(in crate::chunk) fn gpu_cull_submission(
    draw_mode: ChunkDrawMode,
    features: WgpuFeatures,
    downlevel: DownlevelFlags,
    backend: wgpu::Backend,
    forced_cpu: bool,
) -> Option<GpuCullSubmission> {
    if forced_cpu
        || draw_mode != ChunkDrawMode::MultiDrawIndirect
        || !features.contains(WgpuFeatures::INDIRECT_FIRST_INSTANCE)
        || !downlevel.contains(DownlevelFlags::COMPUTE_SHADERS)
    {
        return None;
    }
    if model::count_draw_offsets_supported(backend)
        && features.contains(WgpuFeatures::MULTI_DRAW_INDIRECT_COUNT)
    {
        Some(GpuCullSubmission::Count)
    } else {
        Some(GpuCullSubmission::Fixed)
    }
}

#[cfg(test)]
pub(in crate::chunk) fn gpu_cull_supported(
    draw_mode: ChunkDrawMode,
    features: WgpuFeatures,
    downlevel: DownlevelFlags,
    backend: wgpu::Backend,
    forced_cpu: bool,
) -> bool {
    gpu_cull_submission(draw_mode, features, downlevel, backend, forced_cpu).is_some()
}

/// The view queued for GPU culling this frame, with the pipelines its late pass reuses.
#[derive(Clone, Copy, Debug)]
pub(in crate::chunk) struct GpuCullView {
    pub(in crate::chunk) entity: Entity,
    pub(in crate::chunk) main: MainEntity,
    pub(in crate::chunk) pipelines: [CachedRenderPipelineId; STREAM_COUNT],
    pub(in crate::chunk) late_draws: [DrawFunctionId; STREAM_COUNT],
}

#[derive(Resource, Default)]
pub(in crate::chunk) struct GpuCullFrame {
    pub(in crate::chunk) view: Option<GpuCullView>,
}

/// Queue-time access: chooses the GPU-culled view and records its pipelines.
#[derive(SystemParam)]
pub(in crate::chunk) struct GpuCullQueue<'w, 's> {
    support: Option<Res<'w, GpuCullSupport>>,
    frame: Option<ResMut<'w, GpuCullFrame>>,
    direct_support: Option<Res<'w, DirectOcclusionSupport>>,
    direct: Option<ResMut<'w, DirectOcclusionFrame>>,
    direct_state: Option<Res<'w, DirectOcclusion>>,
    views: Query<'w, 's, (Option<&'static Frustum>, Option<&'static RenderLayers>)>,
}

impl GpuCullQueue<'_, '_> {
    /// The view to cull on the GPU this frame; probes keep every view on the CPU path.
    pub(in crate::chunk) fn select<'a>(
        &mut self,
        draw_mode: ChunkDrawMode,
        probing: bool,
        candidates: impl IntoIterator<Item = (Entity, &'a MainEntity, &'a ExtractedView, bool)>,
    ) -> Option<Entity> {
        if let Some(frame) = self.frame.as_deref_mut() {
            frame.view = None;
        }
        let supported = self.support.as_deref().is_some_and(|support| support.0);
        if probing || !supported || draw_mode != ChunkDrawMode::MultiDrawIndirect {
            return None;
        }
        select_gpu_cull_view(candidates, |entity| {
            self.views
                .get(entity)
                .is_ok_and(|(frustum, layers)| gpu_cull_view_eligible(frustum, layers))
        })
    }

    pub(in crate::chunk) fn set_view(&mut self, view: GpuCullView) {
        if let Some(frame) = self.frame.as_deref_mut() {
            frame.view = Some(view);
        }
    }

    /// The direct-drawn view whose terrain occlusion is tested; probes keep it off.
    pub(in crate::chunk) fn select_direct<'a>(
        &mut self,
        draw_mode: ChunkDrawMode,
        probing: bool,
        candidates: impl IntoIterator<Item = (Entity, &'a MainEntity, &'a ExtractedView, bool)>,
    ) -> Option<Entity> {
        self.direct.as_deref_mut()?.clear();
        let supported = self
            .direct_support
            .as_deref()
            .is_some_and(|support| support.0);
        if probing || !supported || draw_mode != ChunkDrawMode::Direct {
            return None;
        }
        select_gpu_cull_view(candidates, |entity| {
            self.views
                .get(entity)
                .is_ok_and(|(frustum, layers)| gpu_cull_view_eligible(frustum, layers))
        })
    }

    /// Starts the selected direct view's frame; `Some(true)` routes its solid terrain to the
    /// terrain pass.
    pub(in crate::chunk) fn begin_direct(
        &mut self,
        entity: Entity,
        view: &ExtractedView,
        msaa: Msaa,
        solid: (CachedRenderPipelineId, DrawFunctionId),
    ) -> Option<bool> {
        let wants = self
            .direct_state
            .as_deref()
            .is_some_and(|state| state.wants_verdict(entity, view, msaa));
        Some(
            self.direct
                .as_deref_mut()?
                .begin(entity, view, solid, wants),
        )
    }

    /// The selected direct view's frame; `None` when the path is not installed.
    pub(in crate::chunk) fn direct_frame(&mut self) -> Option<&mut DirectOcclusionFrame> {
        self.direct.as_deref_mut()
    }
}

pub(in crate::chunk) fn install(app: &mut App) {
    let forced_cpu = std::env::var_os(CPU_CULLING_ENV).is_some_and(|value| value != "0");
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    let (Some(device), Some(adapter)) = (
        render_app.world().get_resource::<RenderDevice>().cloned(),
        render_app.world().get_resource::<RenderAdapter>(),
    ) else {
        return;
    };
    let draw_mode = select_chunk_draw_mode(
        adapter.get_downlevel_capabilities().flags,
        device.features(),
        Backends::from(adapter.get_info().backend),
        apple_gpu(&adapter.get_info()),
    );
    let submission = gpu_cull_submission(
        draw_mode,
        device.features(),
        adapter.get_downlevel_capabilities().flags,
        adapter.get_info().backend,
        forced_cpu,
    );
    let support = GpuCullSupport(submission.is_some());
    let direct = DirectOcclusionSupport(direct_occlusion_supported(
        draw_mode,
        adapter.get_downlevel_capabilities().flags,
        forced_cpu,
    ));
    render_app
        .insert_resource(support)
        .insert_resource(direct)
        .init_resource::<GpuCullFrame>()
        .init_resource::<DirectOcclusionFrame>()
        .add_systems(
            Render,
            (reset_gpu_cull_frame, reset_direct_occlusion_frame).in_set(RenderSystems::Cleanup),
        );
    if direct.0 {
        direct::install(render_app, &device);
        app.add_systems(Last, admit_depth_sampling);
        return;
    }
    let Some(submission) = submission else {
        return;
    };
    render_app
        .insert_resource(GpuCull::new(&device, submission))
        .init_resource::<ChunkHiddenEntities>()
        .add_systems(ExtractSchedule, extract_hidden_chunks)
        .add_systems(
            Render,
            prepare_gpu_cull
                .in_set(RenderSystems::PrepareResources)
                .after(prepare_gpu_chunks)
                .after(bevy::core_pipeline::core_3d::prepare_core_3d_depth_textures),
        );
    node::install_graph(render_app.world_mut());
    app.insert_resource(support)
        .add_systems(Last, admit_depth_sampling);
}

/// The late pass and entity shadows read the main depth target.
pub(crate) fn admit_depth_sampling(mut cameras: Query<&mut Camera3d>) {
    for mut camera in &mut cameras {
        let usage = TextureUsages::from(camera.depth_texture_usages);
        if !usage.contains(TextureUsages::TEXTURE_BINDING) {
            camera.depth_texture_usages = (usage | TextureUsages::TEXTURE_BINDING).into();
        }
    }
}

fn reset_gpu_cull_frame(mut frame: ResMut<GpuCullFrame>, cull: Option<ResMut<GpuCull>>) {
    frame.view = None;
    if let Some(mut cull) = cull {
        cull.prepared_view = None;
    }
}

/// Picks the GPU-culled view: the lowest-id unmirrored perspective view on render layer 0.
pub(in crate::chunk) fn select_gpu_cull_view<'a>(
    candidates: impl IntoIterator<Item = (Entity, &'a MainEntity, &'a ExtractedView, bool)>,
    eligible: impl Fn(Entity) -> bool,
) -> Option<Entity> {
    candidates
        .into_iter()
        .filter(|&(entity, _, view, enhanced)| {
            pipeline::solid::solid_cull_camera(view, enhanced).is_some() && eligible(entity)
        })
        .min_by_key(|(_, main, _, _)| main.id().to_bits())
        .map(|(entity, ..)| entity)
}

/// Whether the view can host the GPU cull: it has a frustum and sees render layer 0.
pub(in crate::chunk) fn gpu_cull_view_eligible(
    frustum: Option<&Frustum>,
    layers: Option<&RenderLayers>,
) -> bool {
    frustum.is_some() && layers.is_none_or(|layers| layers.intersects(&RenderLayers::default()))
}
