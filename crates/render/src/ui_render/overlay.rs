//! Ordered world-projected UI and depth-free HUD overlay with exact hand coverage.
use super::*;
#[path = "world.rs"]
mod world;
use crate::ui::UI_BLEND_ALPHA;
use bevy::{
    camera::{MainPassResolutionOverride, Viewport},
    core_pipeline::core_3d::graph::{Core3d, Node3d},
    ecs::query::QueryItem,
    render::{
        camera::ExtractedCamera,
        render_graph::{
            NodeRunError, RenderGraph, RenderGraphContext, RenderLabel, ViewNode, ViewNodeRunner,
        },
        render_resource::{
            LoadOp, Operations, RenderPassDepthStencilAttachment, RenderPassDescriptor, StoreOp,
        },
        renderer::RenderContext,
        view::ViewDepthTexture,
    },
};
use std::{collections::BTreeMap, ops::Range, sync::Mutex};
use world::UiWorldNode;

#[derive(Debug, Clone, Hash, Eq, PartialEq, RenderLabel)]
pub(crate) struct UiOverlayLabel;
#[derive(Debug, Clone, Hash, Eq, PartialEq, RenderLabel)]
pub(crate) struct UiWorldLabel;
#[derive(Debug, Clone, Hash, Eq, PartialEq, RenderLabel)]
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
/// Runs `N` only on views whose Enhanced opt-in equals `POST`, so one pass draws.
pub(crate) struct GradeStage<N, const POST: bool>(pub(crate) N);

impl<N: ViewNode, const POST: bool> ViewNode for GradeStage<N, POST> {
    type ViewQuery = (Has<crate::EnhancedRendering>, N::ViewQuery);

    fn update(&mut self, world: &mut World) {
        self.0.update(world);
    }

    fn run<'w>(
        &self,
        graph: &mut RenderGraphContext,
        render_context: &mut RenderContext<'w>,
        (after_grade, view): QueryItem<'w, '_, Self::ViewQuery>,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        if after_grade != POST {
            return Ok(());
        }
        self.0.run(graph, render_context, view, world)
    }
}

pub(crate) fn install_overlay_graph(world: &mut World) {
    let runner = ViewNodeRunner::new(GradeStage::<_, false>(UiOverlayNode), world);
    // Graded views' HUD pass; the Enhanced graph orders it after Bloom and grading.
    let post_runner = ViewNodeRunner::new(GradeStage::<_, true>(UiOverlayNode), world);
    let world_runner = ViewNodeRunner::<UiWorldNode>::new(UiWorldNode, world);
    let Some(mut graphs) = world.get_resource_mut::<RenderGraph>() else {
        return;
    };
    let Some(graph) = graphs.get_sub_graph_mut(Core3d) else {
        return;
    };
    if graph.get_node_state(UiOverlayLabel).is_err() {
        graph.add_node(UiOverlayLabel, runner);
    }
    if graph.get_node_state(UiWorldLabel).is_err() {
        graph.add_node(UiWorldLabel, world_runner);
    }
    if graph.get_node_state(UiOverlayPostLabel).is_err() {
        graph.add_node(UiOverlayPostLabel, post_runner);
    }
    graph.add_node_edges((
        Node3d::MainTransparentPass,
        UiWorldLabel,
        UiOverlayLabel,
        Node3d::EndMainPass,
    ));
}

type UiOverlayView = (
    Entity,
    &'static ExtractedView,
    &'static Msaa,
    Option<&'static ViewDepthTexture>,
    Option<&'static super::composite::UiLayerTexture>,
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
    for (view_entity, view, msaa, depth, layer) in &views {
        let Ok(pipeline_id) = pipeline.variants.specialize(
            &pipeline_cache,
            UiPipelineKey {
                msaa: *msaa,
                hdr: view.hdr,
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
        let Ok(invert_pipeline_id) = pipeline.variants.specialize(
            &pipeline_cache,
            UiPipelineKey {
                msaa: *msaa,
                hdr: view.hdr,
                invert_blend: true,
                layer: false,
                depth_test: false,
                depth_write: false,
                isolated_depth: false,
            },
        ) else {
            gpu.view_pipelines.remove(&view_entity);
            continue;
        };
        cache_view_pipeline_pair(
            &mut gpu.view_pipelines,
            view_entity,
            (pipeline_id, invert_pipeline_id),
        );
        match composite.specialize(
            &pipeline_cache,
            super::composite::UiCompositeKey { hdr: view.hdr },
        ) {
            Some(id) => {
                gpu.composite_pipelines.insert(view_entity, id);
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
                    hdr: view.hdr,
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
                hdr: view.hdr,
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

struct UiOverlayNode;
impl ViewNode for UiOverlayNode {
    type ViewQuery = (
        &'static ViewTarget,
        &'static MainEntity,
        &'static ExtractedCamera,
        Option<&'static MainPassResolutionOverride>,
        Option<&'static super::composite::UiLayerTexture>,
    );
    fn run(
        &self,
        graph: &mut RenderGraphContext,
        context: &mut RenderContext,
        (target, main, camera, resolution_override, layer): QueryItem<Self::ViewQuery>,
        world: &World,
    ) -> Result<(), NodeRunError> {
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
            overlay_pipeline_pair(&gpu.batches, &gpu.view_pipelines, graph.view_entity()),
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
                .get(&graph.view_entity())
                .and_then(|id| pipeline_cache.get_render_pipeline(*id)),
        ) else {
            return Ok(());
        };
        let composite_layout = pipeline_cache.get_bind_group_layout(&composite.layout);
        let viewport = overlay_viewport(camera.viewport.as_ref(), resolution_override);
        let skip = world.get_resource::<UiHandCoverage>().and_then(|coverage| {
            coverage.range(
                graph.view_entity(),
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
            .and_then(|depths| depths.compatible(graph.view_entity(), layer));
        let layer_draw = UiLayerDraw {
            gpu,
            pipeline_cache,
            alpha: layer_pipeline,
            vertices,
            indices,
            owner: graph.view_entity(),
            layer,
            model_depth,
            viewport: viewport.as_ref(),
            skip: skip.as_ref(),
        };
        let mut model_lifetime = super::model_depth::ModelDepthLifetime::default();
        // Alpha batches blend in the gamma-space layer; an invert batch (the
        // crosshair) must see the scene, so the layer composites before it.
        for segment in batches.split_inclusive(|(_, batch, _)| batch.blend_mode == UI_BLEND_INVERT)
        {
            let (layered, inverted) = match segment.split_last() {
                Some((last, rest)) if last.1.blend_mode == UI_BLEND_INVERT => (rest, Some(last)),
                _ => (segment, None),
            };
            if draw_ui_layer(context, &layer_draw, layered, &mut model_lifetime) {
                super::composite::composite(
                    context,
                    target,
                    &layer.0.default_view,
                    composite_pipeline,
                    &composite_layout,
                );
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
                        .get(&(
                            graph.view_entity(),
                            batch.depth_test != 0,
                            batch.depth_write != 0,
                        ))
                        .map(|pair| pair.1)
                } else {
                    Some(*invert)
                };
                let Some(pipeline) = id.and_then(|id| pipeline_cache.get_render_pipeline(id))
                else {
                    // Still compiling: skip the crosshair rather than blend it wrong.
                    continue;
                };
                // Model depth follows the single-sample UI layer. An inverted model
                // addresses the resolved scene directly, never the world MSAA depth.
                let attachments = [Some(if scoped {
                    bevy::render::render_resource::RenderPassColorAttachment {
                        view: target.main_texture_view(),
                        depth_slice: None,
                        resolve_target: None,
                        ops: Operations {
                            load: LoadOp::Load,
                            store: StoreOp::Store,
                        },
                    }
                } else {
                    target.get_color_attachment()
                })];
                let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
                    label: Some("retained depth-free HUD invert"),
                    color_attachments: &attachments,
                    depth_stencil_attachment: (scoped && needs_depth)
                        .then(|| model_depth_attachment(model_depth.unwrap(), &model_lifetime)),
                    timestamp_writes: None,
                    occlusion_query_set: None,
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
                    std::slice::from_ref(inverted),
                    skip.as_ref(),
                );
            }
        }
        Ok(())
    }
}

struct UiLayerDraw<'a> {
    gpu: &'a UiGpu,
    pipeline_cache: &'a PipelineCache,
    alpha: &'a RenderPipeline,
    vertices: &'a Buffer,
    indices: &'a Buffer,
    owner: Entity,
    layer: &'a super::composite::UiLayerTexture,
    model_depth: Option<&'a super::model_depth::UiModelDepth>,
    viewport: Option<&'a Viewport>,
    skip: Option<&'a Range<u32>>,
}

fn model_depth_attachment<'a>(
    depth: &'a super::model_depth::UiModelDepth,
    lifetime: &super::model_depth::ModelDepthLifetime,
) -> RenderPassDepthStencilAttachment<'a> {
    RenderPassDepthStencilAttachment {
        view: &depth.view,
        depth_ops: Some(Operations {
            load: if lifetime.cleared() {
                LoadOp::Load
            } else {
                LoadOp::Clear(0.0)
            },
            store: StoreOp::Store,
        }),
        stencil_ops: None,
    }
}

/// Material passes load the same gamma layer in authored order; each control owns
/// a fresh model-depth clear, shared by its later translucent/read-only materials.
fn draw_ui_layer(
    context: &mut RenderContext,
    draw: &UiLayerDraw<'_>,
    batches: &[(usize, &UiRenderBatch, crate::UiTextureLocation)],
    lifetime: &mut super::model_depth::ModelDepthLifetime,
) -> bool {
    let mut encoded = false;
    let mut start = 0;
    while let Some((_, first, _)) = batches.get(start) {
        let mode = (
            first.isolated_depth_scope,
            first.depth_test,
            first.depth_write,
        );
        let length = batches[start..]
            .iter()
            .take_while(|(_, batch, _)| {
                (
                    batch.isolated_depth_scope,
                    batch.depth_test,
                    batch.depth_write,
                ) == mode
            })
            .count();
        let group = &batches[start..start + length];
        start += length;
        lifetime.enter(mode.0);
        let needs_depth = mode.1 != 0 || mode.2 != 0;
        let pipeline = if needs_depth {
            draw.model_depth
                .and_then(|_| {
                    draw.gpu
                        .model_view_pipelines
                        .get(&(draw.owner, mode.1 != 0, mode.2 != 0))
                })
                .and_then(|pair| draw.pipeline_cache.get_render_pipeline(pair.0))
        } else {
            Some(draw.alpha)
        };
        let Some(pipeline) = pipeline else {
            continue;
        };
        let attachments = [Some(
            bevy::render::render_resource::RenderPassColorAttachment {
                view: &draw.layer.0.default_view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: if encoded {
                        LoadOp::Load
                    } else {
                        LoadOp::Clear(Default::default())
                    },
                    store: StoreOp::Store,
                },
            },
        )];
        let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("ordered gamma-space UI/model material layer"),
            color_attachments: &attachments,
            depth_stencil_attachment: needs_depth
                .then(|| model_depth_attachment(draw.model_depth.unwrap(), lifetime)),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_render_pipeline(pipeline);
        draw_batches(
            &mut pass,
            draw.gpu,
            draw.vertices,
            draw.indices,
            draw.viewport,
            group,
            draw.skip,
        );
        if needs_depth {
            lifetime.encoded();
        }
        encoded = true;
    }
    encoded
}

/// Draw `batches` into `pass`, each under its own scissor and page bind group.
fn draw_batches<'w>(
    pass: &mut bevy::render::render_phase::TrackedRenderPass<'w>,
    gpu: &'w UiGpu,
    vertices: &'w Buffer,
    indices: &'w Buffer,
    viewport: Option<&Viewport>,
    batches: &[(usize, &UiRenderBatch, crate::UiTextureLocation)],
    skip: Option<&Range<u32>>,
) {
    if let Some(viewport) = viewport {
        pass.set_camera_viewport(viewport);
    }
    pass.set_vertex_buffer(0, vertices.slice(..));
    pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
    for (_, batch, location) in batches {
        let binding = gpu.textures.buckets[location.bucket]
            .bind_group
            .as_ref()
            .unwrap();
        pass.set_bind_group(0, binding, &[]);
        let scissor = batch.scissor;
        pass.set_scissor_rect(scissor.x, scissor.y, scissor.width, scissor.height);
        for range in retained_batch_ranges(batch, skip).into_iter().flatten() {
            pass.draw_indexed(range, 0, location.layer..location.layer + 1);
        }
    }
    pass.set_scissor_rect(0, 0, gpu.viewport_size[0], gpu.viewport_size[1]);
}

pub(crate) fn overlay_viewport(
    viewport: Option<&Viewport>,
    resolution_override: Option<&MainPassResolutionOverride>,
) -> Option<Viewport> {
    Viewport::from_viewport_and_override(viewport, resolution_override)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_hand_split_keeps_mixed_bucket_layer_blend_and_shadow_fill_order() {
        let plan =
            crate::UiTexturePlan::new(&[[1024, 1024], [2048, 2048], [256, 256], [2048, 2048]])
                .unwrap();
        let batches = [1, 0, 3, 2, 1]
            .into_iter()
            .enumerate()
            .map(|(index, page)| {
                UiRenderBatch::new(
                    page,
                    crate::ui::UiScissor::new(index as u32, 0, 20, 20),
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
