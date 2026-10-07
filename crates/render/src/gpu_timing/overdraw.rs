//! Explicit raster diagnostics count alpha-surviving submitted terrain layers without depth.
//! Its extra draw and readback work is excluded from performance comparisons.

mod raster;
mod readback;
#[cfg(test)]
mod tests;

use std::{collections::HashMap, sync::atomic::Ordering};

use bevy::{
    app::SubApp,
    camera::{MainPassResolutionOverride, Viewport},
    core_pipeline::core_3d::Opaque3d,
    prelude::*,
    render::{
        Render, RenderStartup, RenderSystems,
        camera::ExtractedCamera,
        render_phase::{DrawFunctions, ViewBinnedRenderPhases},
        render_resource::{
            BlendComponent, BlendFactor, BlendOperation, BlendState, CachedRenderPipelineId,
            ColorTargetState, ColorWrites, PipelineCache, RenderPipelineDescriptor, TextureFormat,
        },
        renderer::RenderDevice,
        view::{ExtractedView, ViewTarget},
    },
};

use crate::RuntimeStage;
use readback::{IDLE, Target};

const CAPTURE_INTERVAL_FRAMES: u64 = 120;
const FORMAT: TextureFormat = TextureFormat::Rgba16Float;

/// Reads only the explicit raster-diagnostic opt-in flag.
pub(super) fn requested() -> bool {
    std::env::var_os("RUST_MCBE_OPAQUE_LAYERS").is_some_and(|value| value == "1")
}

/// Installs no resources, draws or systems until the caller has accepted the diagnostic cost.
pub(super) fn install(app: &mut SubApp) {
    app.init_resource::<Probe>()
        .add_systems(RenderStartup, raster::install_graph)
        .add_systems(Render, prepare.in_set(RenderSystems::PrepareBindGroups));
}

#[derive(Resource, Default)]
struct Probe {
    variants: HashMap<CachedRenderPipelineId, CachedRenderPipelineId>,
    target: Option<Target>,
    frame: u64,
    next_capture: u64,
    view: Option<Entity>,
    unsupported_reported: bool,
}

/// Only these streams have a diagnostic fragment entry with their original alpha test.
fn counted(stage: RuntimeStage) -> bool {
    matches!(
        stage,
        RuntimeStage::GpuTerrainSolid
            | RuntimeStage::GpuTerrainCutout
            | RuntimeStage::GpuTerrainModel
    )
}

/// Retains geometry and alpha coverage while summing one into every surviving covered pixel.
fn descriptor(mut source: RenderPipelineDescriptor) -> RenderPipelineDescriptor {
    source.label = Some("opaque terrain layer diagnostic".into());
    source.depth_stencil = None;
    source.multisample = Default::default();
    source.vertex.shader_defs.push("OPAQUE_OVERDRAW".into());
    let fragment = source
        .fragment
        .as_mut()
        .expect("terrain has a fragment shader");
    fragment.shader_defs.push("OPAQUE_OVERDRAW".into());
    let add = BlendComponent {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::One,
        operation: BlendOperation::Add,
    };
    fragment.targets = vec![Some(ColorTargetState {
        format: FORMAT,
        blend: Some(BlendState {
            color: add,
            alpha: add,
        }),
        write_mask: ColorWrites::RED,
    })];
    source
}

/// Reuses pipelines and targets, and admits one asynchronous capture at a time.
fn prepare(
    mut probe: ResMut<Probe>,
    device: Res<RenderDevice>,
    cache: Res<PipelineCache>,
    functions: Res<DrawFunctions<Opaque3d>>,
    phases: Res<ViewBinnedRenderPhases<Opaque3d>>,
    views: Query<(
        Entity,
        &ExtractedCamera,
        &ExtractedView,
        &ViewTarget,
        Option<&MainPassResolutionOverride>,
    )>,
) {
    probe.frame += 1;
    probe.view = None;
    if let Some(target) = &probe.target {
        let _ = device.poll(wgpu::PollType::Poll);
        target.report();
        if target.state.load(Ordering::Acquire) != IDLE {
            return;
        }
    }
    if probe.frame < probe.next_capture {
        return;
    }
    let Some((entity, camera, view, output, resolution)) = views
        .iter()
        .filter(|(_, camera, view, _, _)| {
            camera.physical_viewport_size.is_some()
                && phases
                    .get(&view.retained_view_entity)
                    .is_some_and(|phase| !phase.non_mesh_items.is_empty())
        })
        .min_by_key(|(entity, _, _, _, _)| entity.to_bits())
    else {
        return;
    };
    let Some(phase) = phases.get(&view.retained_view_entity) else {
        return;
    };
    let functions = functions.read();
    let mut ready = true;
    let mut items = 0;
    for ((batch, _), entries) in &phase.non_mesh_items {
        if !counted(crate::chunk::pipeline::opaque::timing_category(
            &functions,
            batch.draw_function,
        )) {
            continue;
        }
        if !crate::chunk::pipeline::opaque::supports_layer_probe(&functions, batch.draw_function) {
            if !probe.unsupported_reported {
                eprintln!("RUST_MCBE_OPAQUE_LAYERS unsupported_gpu_cull_late_phase");
                probe.unsupported_reported = true;
            }
            return;
        }
        items += entries.entities.len();
        if cache.get_render_pipeline(batch.pipeline).is_none() {
            ready = false;
            continue;
        }
        let variant = probe.variants.entry(batch.pipeline).or_insert_with(|| {
            cache.queue_render_pipeline(descriptor(
                cache.get_render_pipeline_descriptor(batch.pipeline).clone(),
            ))
        });
        ready &= cache.get_render_pipeline(*variant).is_some();
    }
    if !ready || items == 0 {
        return;
    }
    let extent = output.main_texture().size();
    if extent.width == 0 || extent.height == 0 {
        return;
    }
    let viewport = Viewport::from_viewport_and_override(camera.viewport.as_ref(), resolution)
        .unwrap_or_else(|| Viewport {
            physical_position: UVec2::ZERO,
            physical_size: UVec2::new(extent.width, extent.height),
            depth: 0.0..1.0,
        });
    let end = viewport.physical_position.as_u64vec2() + viewport.physical_size.as_u64vec2();
    if end.x > u64::from(extent.width) || end.y > u64::from(extent.height) {
        return;
    }
    if probe
        .target
        .as_ref()
        .is_none_or(|target| target.size != [extent.width, extent.height])
    {
        probe.target = Some(Target::new(&device, extent.width, extent.height));
    }
    let frame = probe.frame;
    let target = probe.target.as_mut().unwrap();
    target.frame = frame;
    target.items = items;
    target.viewport = viewport;
    probe.view = Some(entity);
    probe.next_capture = frame + CAPTURE_INTERVAL_FRAMES;
}
