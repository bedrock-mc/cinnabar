//! Ordered world-projected UI and depth-free HUD overlay with exact hand coverage.
use super::pipeline::{UiPipeline, UiPipelineKey};
use super::*;
use super::{
    damage::UiDamage,
    layer::{LayerDrawn, UiLayerDraw, draw_batches, draw_ui_layer, model_depth_attachment},
};
#[path = "world.rs"]
mod world;
use bevy::prelude::SystemSet;
use bevy::{
    camera::{MainPassResolutionOverride, Viewport},
    core_pipeline::{Core3d, Core3dSystems},
    ecs::query::QueryItem,
    render::{
        camera::ExtractedCamera,
        render_resource::{
            LoadOp, Operations, RenderPassDepthStencilAttachment, RenderPassDescriptor, StoreOp,
        },
        renderer::RenderContext,
        view::ViewDepthStencilTexture,
    },
};
use render_model::UI_BLEND_ALPHA;
use std::{collections::BTreeMap, ops::Range, sync::Mutex};

#[derive(Debug, Clone, Hash, Eq, PartialEq, SystemSet)]
pub(crate) struct UiOverlayLabel;
#[derive(Debug, Clone, Hash, Eq, PartialEq, SystemSet)]
pub(crate) struct UiWorldLabel;
#[derive(Debug, Clone, Hash, Eq, PartialEq, SystemSet)]
pub(crate) struct UiOverlayPostLabel;
/// Per-frame draw encoding coverage, not queue completion or presentation.
/// The optional producer must independently require its prior completion gate.
#[derive(Default, Resource)]
pub(crate) struct UiHandCoverage(Mutex<HandCoverageState>);
#[derive(Default)]
struct HandCoverageState {
    epoch: u64,
    exhausted: bool,
    draw: Option<(u64, Entity, Entity, u64, u32, u32)>,
}
impl UiHandCoverage {
    pub(crate) fn clear(&self) {
        let mut state = self.0.lock().expect("HUD hand coverage lock");
        state.draw = None;
        if let Some(next) = state.epoch.checked_add(1) {
            state.epoch = next;
        } else {
            state.exhausted = true;
        }
    }
    pub(crate) fn record(&self, view: Entity, main: Entity, revision: u64, first: u32, page: u32) {
        let mut state = self.0.lock().expect("HUD hand coverage lock");
        if !state.exhausted {
            state.draw = Some((state.epoch, view, main, revision, first, page));
        }
    }
    pub(crate) fn range(
        &self,
        view: Entity,
        main: Entity,
        revision: Option<u64>,
        batches: &[UiRenderBatch],
        index_count: usize,
    ) -> Option<Range<u32>> {
        let state = self.0.lock().expect("HUD hand coverage lock");
        let (epoch, owner, main_owner, expected, first, page) = state.draw?;
        if state.exhausted
            || epoch != state.epoch
            || owner != view
            || main_owner != main
            || revision != Some(expected)
            || first % 3 != 0
        {
            return None;
        }
        let end = first.checked_add(6)?;
        if end as usize > index_count {
            return None;
        }
        let mut containing = batches.iter().filter(|b| {
            first >= b.first_index
                && b.first_index
                    .checked_add(b.index_count)
                    .is_some_and(|last| end <= last)
        });
        let batch = containing.next()?;
        if batch.texture_page != page || batch.blend_mode != UI_BLEND_ALPHA {
            return None;
        }
        if batch.world_projection != 0 {
            return None;
        }
        if containing.next().is_some() {
            return None;
        }
        Some(first..end)
    }
}
pub(crate) fn retained_batch_ranges(
    batch: &UiRenderBatch,
    skip: Option<&Range<u32>>,
) -> [Option<Range<u32>>; 2] {
    let end = batch.first_index + batch.index_count;
    if let Some(skip) = skip
        && skip.start >= batch.first_index
        && skip.end <= end
    {
        [
            (batch.first_index < skip.start).then_some(batch.first_index..skip.start),
            (skip.end < end).then_some(skip.end..end),
        ]
    } else {
        [Some(batch.first_index..end), None]
    }
}
/// Selects the ordinary or enhanced stage for the current camera.
pub(crate) fn grade_stage<const POST: bool>(
    view: bevy::render::renderer::ViewQuery<Has<crate::EnhancedRendering>>,
) -> bool {
    (render_model::enhanced_rendering_enabled() && view.into_inner()) == POST
}

/// Orders projected UI before hands and the HUD after all world post-processing.
pub(crate) fn install_overlay_graph(world: &mut World) {
    if world.contains_resource::<OverlayPassesInstalled>() {
        return;
    }
    let installed = world
        .try_schedule_scope(Core3d, |_, schedule| {
            schedule.add_systems((
                crate::gpu_timing::profiled(
                    world::ui_world,
                    Some(crate::RuntimeStage::GpuUi),
                    "UiWorldLabel",
                )
                .in_set(UiWorldLabel)
                .after(crate::scene_target::ScenePass::Transparent)
                .before(crate::scene_target::ScenePass::Finish)
                .in_set(Core3dSystems::MainPass),
                crate::gpu_timing::profiled(
                    ui_overlay,
                    Some(crate::RuntimeStage::GpuUi),
                    "UiOverlayLabel",
                )
                .in_set(UiOverlayLabel)
                .after(Core3dSystems::PostProcess)
                .before(bevy::core_pipeline::upscaling::upscaling)
                .run_if(grade_stage::<false>),
                crate::gpu_timing::profiled(
                    ui_overlay,
                    Some(crate::RuntimeStage::GpuUi),
                    "UiOverlayPostLabel",
                )
                .in_set(UiOverlayPostLabel)
                .after(Core3dSystems::PostProcess)
                .before(bevy::core_pipeline::upscaling::upscaling)
                .run_if(grade_stage::<true>),
            ));
        })
        .is_ok();
    if installed {
        world.insert_resource(OverlayPassesInstalled);
        super::composite::install_present_node(world);
    }
}

#[derive(Resource)]
struct OverlayPassesInstalled;

type UiOverlayView = (
    Entity,
    &'static ExtractedView,
    &'static bevy::render::camera::ExtractedCamera,
    &'static Msaa,
    Option<&'static ViewDepthStencilTexture>,
    Option<&'static super::composite::UiLayerTexture>,
    Option<&'static ViewTarget>,
);

#[allow(clippy::too_many_arguments)] // Independent Bevy render resources and view query.
pub(super) fn queue_ui_overlay(
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<UiPipeline>,
    mut composite: ResMut<super::composite::UiCompositePipeline>,
    mut gpu: ResMut<UiGpu>,
    views: Query<'_, '_, UiOverlayView>,
    render_device: Res<RenderDevice>,
    mut model_depths: ResMut<super::model_depth::UiModelDepths>,
    coverage: Option<Res<UiHandCoverage>>,
) {
    // Always clear the previous render-frame coverage, including empty UI and
    // unchanged accepted revisions, before any preparation/queue early return.
    if let Some(coverage) = coverage {
        coverage.clear();
    }
    // Retain unchanged view entries rather than freeing/reallocating tree nodes
    // every frame; only departed views release their cached pair.
    retain_view_pipeline_entries(&mut gpu.view_pipelines, |view| views.contains(view));
    gpu.composite_pipelines
        .retain(|view, _| views.contains(*view));
    gpu.world_view_pipelines
        .retain(|(view, _, _), _| views.contains(*view));
    gpu.model_view_pipelines
        .retain(|(view, _, _), _| views.contains(*view));
    model_depths.synchronize_device(&render_device);
    model_depths.views.retain(|view, _| views.contains(*view));
    if gpu.batches.is_empty()
        || gpu
            .textures
            .buckets
            .iter()
            .any(|bucket| bucket.bind_group.is_none())
        || gpu.vertex_buffer.is_none()
        || gpu.index_buffer.is_none()
    {
        return;
    }
    let needs_models = gpu
        .batches
        .iter()
        .any(|batch| batch.isolated_depth_scope.is_some());
    let needs_model_depth = gpu.batches.iter().any(|batch| {
        batch.isolated_depth_scope.is_some() && (batch.depth_test != 0 || batch.depth_write != 0)
    });
    if !needs_model_depth {
        model_depths.views.clear();
    }
    for (view_entity, _view, camera, msaa, depth, layer, target) in &views {
        let Ok(pipeline_id) = pipeline.variants.specialize(
            &pipeline_cache,
            UiPipelineKey {
                msaa: *msaa,
                hdr: camera.hdr,
                invert_blend: false,
                layer: true,
                depth_test: false,
                depth_write: false,
                isolated_depth: false,
            },
        ) else {
            gpu.view_pipelines.remove(&view_entity);
            continue;
        };
        let Ok(invert_pipeline_id) = pipeline
            .variants
            .specialize(&pipeline_cache, hud_invert_pipeline_key(camera.hdr))
        else {
            gpu.view_pipelines.remove(&view_entity);
            continue;
        };
        cache_view_pipeline_pair(
            &mut gpu.view_pipelines,
            view_entity,
            (pipeline_id, invert_pipeline_id),
        );
        match composite.view_pipelines(
            &pipeline_cache,
            camera.hdr,
            target.and_then(ViewTarget::out_texture_view_format),
        ) {
            Some(pipelines) => {
                gpu.composite_pipelines.insert(view_entity, pipelines);
            }
            None => {
                gpu.composite_pipelines.remove(&view_entity);
            }
        }
        if needs_model_depth && let Some(layer) = layer {
            model_depths.ensure(view_entity, layer, &render_device);
        }
        if needs_models {
            for (depth_test, depth_write) in
                [(false, false), (true, false), (false, true), (true, true)]
            {
                let key = UiPipelineKey {
                    msaa: *msaa,
                    hdr: camera.hdr,
                    invert_blend: false,
                    layer: true,
                    depth_test,
                    depth_write,
                    isolated_depth: true,
                };
                let pair = pipeline
                    .variants
                    .specialize(&pipeline_cache, key)
                    .and_then(|alpha| {
                        pipeline
                            .variants
                            .specialize(
                                &pipeline_cache,
                                UiPipelineKey {
                                    msaa: Msaa::Off,
                                    invert_blend: true,
                                    layer: false,
                                    ..key
                                },
                            )
                            .map(|invert| (alpha, invert))
                    });
                if let Ok(pair) = pair {
                    gpu.model_view_pipelines
                        .insert((view_entity, depth_test, depth_write), pair);
                } else {
                    gpu.model_view_pipelines
                        .remove(&(view_entity, depth_test, depth_write));
                }
            }
        } else {
            gpu.model_view_pipelines
                .retain(|(owner, _, _), _| *owner != view_entity);
        }
        if !gpu.batches.iter().any(|batch| batch.world_projection != 0) {
            gpu.world_view_pipelines
                .retain(|(owner, _, _), _| *owner != view_entity);
            continue;
        }
        for (depth_test, depth_write) in
            [(false, false), (true, false), (false, true), (true, true)]
        {
            if ((depth_test || depth_write) && depth.is_none())
                || !gpu.batches.iter().any(|batch| {
                    batch.world_projection != 0
                        && (batch.depth_test != 0, batch.depth_write != 0)
                            == (depth_test, depth_write)
                })
            {
                continue;
            }
            let key = UiPipelineKey {
                msaa: *msaa,
                hdr: camera.hdr,
                invert_blend: false,
                layer: false,
                depth_test,
                depth_write,
                isolated_depth: false,
            };
            let pair = pipeline
                .variants
                .specialize(&pipeline_cache, key)
                .and_then(|alpha| {
                    pipeline
                        .variants
                        .specialize(
                            &pipeline_cache,
                            UiPipelineKey {
                                invert_blend: true,
                                ..key
                            },
                        )
                        .map(|invert| (alpha, invert))
                });
            match pair {
                Ok(pair) => {
                    gpu.world_view_pipelines
                        .insert((view_entity, depth_test, depth_write), pair);
                }
                Err(_) => {
                    gpu.world_view_pipelines
                        .remove(&(view_entity, depth_test, depth_write));
                }
            }
        }
    }
}

pub(crate) fn retain_view_pipeline_entries(
    entries: &mut BTreeMap<Entity, (CachedRenderPipelineId, CachedRenderPipelineId)>,
    mut live: impl FnMut(Entity) -> bool,
) {
    entries.retain(|view, _| live(*view));
}
pub(crate) fn cache_view_pipeline_pair(
    entries: &mut BTreeMap<Entity, (CachedRenderPipelineId, CachedRenderPipelineId)>,
    view: Entity,
    pair: (CachedRenderPipelineId, CachedRenderPipelineId),
) {
    if let Some(existing) = entries.get_mut(&view) {
        *existing = pair;
    } else {
        entries.insert(view, pair);
    }
}
pub(crate) fn overlay_pipeline_pair<'a, T>(
    batches: &[UiRenderBatch],
    entries: &'a BTreeMap<Entity, T>,
    view: Entity,
) -> Option<&'a T> {
    // Empty UI can retain an old format/sample cache pair, but must never bind
    // it against a changed target, even in a pass with zero draw commands.
    if batches.is_empty() {
        None
    } else {
        entries.get(&view)
    }
}

type UiOverlayQuery = (
    &'static ViewTarget,
    &'static MainEntity,
    &'static ExtractedCamera,
    Option<&'static MainPassResolutionOverride>,
    Option<&'static super::composite::UiLayerTexture>,
);

/// Draws the retained HUD after world post-processing.
pub(crate) fn ui_overlay(
    world: &World,
    query: bevy::render::renderer::ViewQuery<UiOverlayQuery>,
    mut context: RenderContext,
) -> bevy::ecs::error::Result {
    let view_entity = query.entity();
    let (target, main, camera, resolution_override, layer) = query.into_inner();
    let context = &mut context;
    crate::screen_overlay_render::draw_before_hud(
        view_entity,
        target,
        camera,
        resolution_override,
        context,
        world,
    );
    let (Some(gpu), Some(pipeline_cache), Some(composite)) = (
        world.get_resource::<UiGpu>(),
        world.get_resource::<PipelineCache>(),
        world.get_resource::<super::composite::UiCompositePipeline>(),
    ) else {
        return Ok(());
    };
    let (Some(vertices), Some(indices), Some((alpha, invert)), Some(layer)) = (
        &gpu.vertex_buffer,
        &gpu.index_buffer,
        overlay_pipeline_pair(&gpu.batches, &gpu.view_pipelines, view_entity),
        layer,
    ) else {
        return Ok(());
    };
    if gpu.textures.buckets.len() != gpu.textures.allocated_buckets().len()
        || gpu
            .textures
            .buckets
            .iter()
            .any(|bucket| bucket.bind_group.is_none())
    {
        return Ok(());
    }
    let Some(batches) = resolved_batches(
        gpu.accepted_revision,
        &gpu.batches,
        &gpu.textures.locations,
        gpu.textures.allocated_buckets(),
    ) else {
        return Ok(());
    };
    let (Some(layer_pipeline), Some(composite_pipeline)) = (
        pipeline_cache.get_render_pipeline(*alpha),
        gpu.composite_pipelines
            .get(&view_entity)
            .and_then(|ids| pipeline_cache.get_render_pipeline(ids.main)),
    ) else {
        return Ok(());
    };
    let composite_layout = pipeline_cache.get_bind_group_layout(&composite.layout);
    let viewport = overlay_viewport(camera.viewport.as_ref(), resolution_override);
    let skip = world.get_resource::<UiHandCoverage>().and_then(|coverage| {
        coverage.range(
            view_entity,
            main.id(),
            gpu.accepted_revision,
            &gpu.batches,
            gpu.index_count,
        )
    });
    let batches: Vec<_> = batches
        .filter(|(_, batch, _)| batch.world_projection == 0)
        .collect();
    let model_depth = world
        .get_resource::<super::model_depth::UiModelDepths>()
        .and_then(|depths| depths.compatible(view_entity, layer));
    let layer_draw = UiLayerDraw {
        world,
        gpu,
        pipeline_cache,
        alpha: layer_pipeline,
        vertices,
        indices,
        owner: view_entity,
        layer,
        model_depth,
        viewport: viewport.as_ref(),
        skip: skip.as_ref(),
        clear: composite.clear_pipeline(pipeline_cache),
    };
    let mut model_lifetime = super::model_depth::ModelDepthLifetime::default();
    let plan = plan_ui_passes(
        &batches,
        world.contains_resource::<super::composite::UiPresentInstalled>(),
    );
    // Only a frame's single layer survives to the next frame; animated glint never does.
    let content = (plan.retainable && !gpu.animated)
        .then_some(gpu.accepted_revision)
        .flatten()
        .map(|revision| super::composite::UiLayerContent {
            revision,
            skip: skip.clone(),
            viewport: viewport
                .as_ref()
                .map(|viewport| (viewport.physical_position, viewport.physical_size)),
            model_depth: model_depth.is_some(),
        });
    let publication = gpu
        .last_admitted_publication
        .upgrade()
        .filter(|input| Some(input.revision) == gpu.accepted_revision);
    let profile = world.get_resource::<super::profile::UiProfile>();
    // Alpha batches blend in the gamma-space layer; an invert batch (the
    // crosshair) must see the scene, so the layer composites before it.
    for segment in plan.segments {
        let layered = &batches[segment.layered];
        let inverted = segment.inverted.map(|index| &batches[index]);
        let held = content.as_ref().and_then(|content| layer.holds(content));
        let proposed = match (content.as_ref(), publication.as_ref()) {
            _ if held.is_some() => UiDamage::Unchanged,
            _ if profile.is_some_and(|profile| profile.baseline_replay) => UiDamage::Full,
            (Some(content), Some(input)) => layer.damage(content, input),
            _ => UiDamage::Full,
        };
        let damage = super::layer::damage_for_passes(
            proposed,
            layered.first().map(|(_, batch, _)| *batch),
            layer_draw.clear.is_some(),
            !matches!(proposed, UiDamage::Rect(_))
                || layered
                    .iter()
                    .all(|(_, batch, _)| layer_draw.pipeline(batch).is_some()),
        );
        if let Some(profile) = profile {
            profile.record_layer(
                damage != UiDamage::Unchanged,
                [layer.texture.width(), layer.texture.height()],
                layer.texture.format(),
                layer.texture.sample_count(),
            );
            if let UiDamage::Rect(rect) = damage {
                profile.record_damage(rect);
            }
        }
        let encoded = match held {
            Some(encoded) => encoded,
            None => {
                let drawn = match damage {
                    UiDamage::Unchanged => LayerDrawn {
                        encoded: true,
                        complete: true,
                    },
                    redraw => draw_ui_layer(
                        context,
                        &layer_draw,
                        layered,
                        &mut model_lifetime,
                        if let UiDamage::Rect(rect) = redraw {
                            Some(rect)
                        } else {
                            None
                        },
                    ),
                };
                layer.hold_publication(
                    content
                        .clone()
                        .filter(|_| drawn.complete)
                        .map(|content| (content, drawn.encoded)),
                    publication.clone(),
                );
                drawn.encoded
            }
        };
        if encoded {
            if segment.present {
                layer.defer_present();
            } else {
                super::composite::composite(
                    context,
                    world,
                    target,
                    &layer.view,
                    composite_pipeline,
                    &composite_layout,
                );
            }
        }
        if let Some(inverted) = inverted {
            let batch = inverted.1;
            let scoped = batch.isolated_depth_scope.is_some();
            model_lifetime.enter(batch.isolated_depth_scope);
            let needs_depth = batch.depth_test != 0 || batch.depth_write != 0;
            if scoped && needs_depth && model_depth.is_none() {
                continue;
            }
            let id = if scoped {
                gpu.model_view_pipelines
                    .get(&(view_entity, batch.depth_test != 0, batch.depth_write != 0))
                    .map(|pair| pair.1)
            } else {
                Some(*invert)
            };
            let Some(pipeline) = id.and_then(|id| pipeline_cache.get_render_pipeline(id)) else {
                // Still compiling: skip the crosshair rather than blend it wrong.
                continue;
            };
            // HUD layers and invert batches share the current resolved scene texture.
            let attachments = [Some(
                bevy::render::render_resource::RenderPassColorAttachment {
                    view: target.main_texture_view(),
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Load,
                        store: StoreOp::Store,
                    },
                },
            )];
            let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
                label: Some("retained depth-free HUD invert"),
                color_attachments: &attachments,
                depth_stencil_attachment: (scoped && needs_depth)
                    .then(|| model_depth_attachment(model_depth.unwrap(), &model_lifetime)),
                timestamp_writes: crate::gpu_timing::ui_pass_timestamps(
                    world,
                    crate::RuntimeStage::GpuUiInvert,
                ),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if scoped && needs_depth {
                model_lifetime.encoded();
            }
            pass.set_render_pipeline(pipeline);
            draw_batches(
                &mut pass,
                gpu,
                vertices,
                indices,
                viewport.as_ref(),
                crate::render_bounds::extent(target.main_texture_view()),
                std::slice::from_ref(inverted),
                skip.as_ref(),
                None,
                world
                    .get_resource::<super::profile::UiProfile>()
                    .map(|profile| (profile, crate::RuntimeStage::GpuUiInvert)),
            );
        }
    }
    Ok(())
}

/// One gamma-layer draw: its layered batch range, then an optional invert batch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UiSegment {
    pub(crate) layered: Range<usize>,
    pub(crate) inverted: Option<usize>,
    /// The layer composites in the output pass instead of over the main texture.
    pub(crate) present: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct UiPassPlan {
    pub(crate) segments: Vec<UiSegment>,
    /// One layer per frame, so it can be kept for the next frame.
    pub(crate) retainable: bool,
}

/// Splits the HUD into layer and invert passes; the last layer waits for the output pass.
pub(crate) fn plan_ui_passes<T: std::borrow::Borrow<UiRenderBatch>, L>(
    batches: &[(usize, T, L)],
    deferred_present: bool,
) -> UiPassPlan {
    let mut segments = Vec::new();
    let mut start = 0;
    for (index, (_, batch, _)) in batches.iter().enumerate() {
        if batch.borrow().blend_mode == UI_BLEND_INVERT {
            segments.push(UiSegment {
                layered: start..index,
                inverted: Some(index),
                present: false,
            });
            start = index + 1;
        }
    }
    if start < batches.len() {
        segments.push(UiSegment {
            layered: start..batches.len(),
            inverted: None,
            present: deferred_present,
        });
    }
    UiPassPlan {
        retainable: segments.len() == 1,
        segments,
    }
}

pub(crate) fn overlay_viewport(
    viewport: Option<&Viewport>,
    resolution_override: Option<&MainPassResolutionOverride>,
) -> Option<Viewport> {
    Viewport::from_viewport_and_override(viewport, resolution_override)
}

/// Selects the ordinary HUD invert pipeline independently of world depth modes.
pub(super) fn hud_invert_pipeline_key(hdr: bool) -> UiPipelineKey {
    UiPipelineKey {
        msaa: Msaa::Off,
        hdr,
        invert_blend: true,
        layer: false,
        depth_test: false,
        depth_write: false,
        isolated_depth: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_hand_split_keeps_mixed_bucket_layer_blend_and_shadow_fill_order() {
        let plan = render_model::UiTexturePlan::new(&[
            [1024, 1024],
            [2048, 2048],
            [256, 256],
            [2048, 2048],
        ])
        .unwrap();
        let batches = [1, 0, 3, 2, 1]
            .into_iter()
            .enumerate()
            .map(|(index, page)| {
                UiRenderBatch::new(
                    page,
                    render_model::UiScissor::new(index as u32, 0, 20, 20),
                    index as u32 * 18,
                    18,
                    if index == 2 {
                        UI_BLEND_INVERT
                    } else {
                        UI_BLEND_ALPHA
                    },
                )
            })
            .collect::<Vec<_>>();
        let trace = resolved_batches(Some(7), &batches, plan.locations(), plan.buckets())
            .unwrap()
            .flat_map(|(index, batch, location)| {
                retained_batch_ranges(batch, Some(&(60..66)))
                    .into_iter()
                    .flatten()
                    .map(move |range| {
                        (
                            index,
                            location.bucket,
                            location.layer,
                            batch.blend_mode,
                            batch.scissor.x,
                            range,
                        )
                    })
            })
            .collect::<Vec<_>>();
        assert_eq!(
            trace,
            vec![
                (0, 1, 0, UI_BLEND_ALPHA, 0, 0..18),
                (1, 0, 0, UI_BLEND_ALPHA, 1, 18..36),
                (2, 1, 1, UI_BLEND_INVERT, 2, 36..54),
                (3, 2, 0, UI_BLEND_ALPHA, 3, 54..60),
                (3, 2, 0, UI_BLEND_ALPHA, 3, 66..72),
                (4, 1, 0, UI_BLEND_ALPHA, 4, 72..90),
            ]
        );
        let full = resolved_batches(Some(7), &batches, plan.locations(), plan.buckets())
            .unwrap()
            .flat_map(|(_, batch, _)| retained_batch_ranges(batch, None).into_iter().flatten())
            .collect::<Vec<_>>();
        assert_eq!(full, vec![0..18, 18..36, 36..54, 54..72, 72..90]);
        assert!(resolved_batches(None, &batches, plan.locations(), plan.buckets()).is_none());
        let mut locations = plan.locations().to_vec();
        locations[1].layer = 2;
        assert!(
            resolved_batches(Some(7), &batches, &locations, plan.buckets()).is_none(),
            "late invalid physical layer must emit no prefix"
        );
    }
}

#[cfg(test)]
#[path = "frame_pass_tests.rs"]
mod frame_pass_tests;

#[cfg(test)]
mod review_tests {
    use super::*;
    #[test]
    fn review_render_hud_invert_uses_the_resolved_scene_sample_count() {
        use super::super::pipeline::{
            UiPipelineSpecializer, ui_bind_group_layout, ui_pipeline_descriptor,
        };
        let mut descriptor = ui_pipeline_descriptor(ui_bind_group_layout());
        UiPipelineSpecializer
            .specialize(hud_invert_pipeline_key(false), &mut descriptor)
            .unwrap();
        assert_eq!(descriptor.multisample.count, 1);
    }
}
