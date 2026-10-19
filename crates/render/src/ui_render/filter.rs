//! Element filters: a mod-surface subtree blurred as one, like CSS `filter: blur()`.
//!
//! While a layer redraws, each filter group's batches draw into an offscreen layer instead of
//! the retained one. The group's [`UI_BLEND_FILTER`] batch then downsamples that layer into a
//! pyramid, within the group's drawn bounds plus three standard deviations, and composites it
//! blurred into the retained layer in the group's place. A retained layer that stays valid
//! reuses its composited pixels, so a static filter costs nothing after its first frame. Layers
//! without filter batches draw exactly as before.

use super::target::UiTarget;
use super::*;
use super::{
    backdrop::{Pyramid, UiBackdropPipeline, UiBackdropStore, pyramid},
    composite::{UI_LAYER_FORMAT, UiLayerTexture},
    layer::{LayerDrawn, UiLayerDraw, draw_batches, draw_ui_layer},
};
use bevy::render::{
    render_resource::{
        BindGroupLayout, Extent3d, LoadOp, Operations, RenderPassColorAttachment,
        RenderPassDescriptor, StoreOp, TextureDescriptor, TextureDimension, TextureUsages,
        TextureViewDescriptor,
    },
    renderer::RenderContext,
};
use render_model::{UI_BLEND_FILTER, UiScissor, UiTextureLocation};

/// Whether a batch composites a filter group.
pub(crate) fn is_filter_composite(batch: &UiRenderBatch) -> bool {
    batch.blend_mode == UI_BLEND_FILTER
}

/// Whether a batch belongs to a filter group or composites one.
fn filtered(batch: &UiRenderBatch) -> bool {
    batch.filter_group.is_some() || is_filter_composite(batch)
}

/// One view's offscreen filter layer and the pyramid that blurs it.
pub(crate) struct FilterTargets {
    offscreen: UiLayerTexture,
    pyramid: Pyramid,
    /// Downsample and composite pipelines.
    pipelines: [CachedRenderPipelineId; 2],
}

/// Allocates each view's filter targets at its main-texture size and queues their pipelines.
pub(super) fn prepare<'a>(
    store: &mut UiBackdropStore,
    pipeline: &mut UiBackdropPipeline,
    cache: &PipelineCache,
    device: &RenderDevice,
    layouts: [&BindGroupLayout; 2],
    sampler: &Sampler,
    views: impl Iterator<Item = (Entity, UiTarget<'a>)>,
) {
    let pipelines = [pipeline.downsample(cache), pipeline.filter_composite(cache)];
    for (entity, target) in views {
        let size = target.main_texture().size();
        let extent = [size.width, size.height];
        if store
            .filters
            .get(&entity)
            .is_some_and(|targets| targets.pyramid.scene == extent)
        {
            continue;
        }
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("UI element filter group"),
            size: Extent3d {
                width: extent[0],
                height: extent[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: UI_LAYER_FORMAT,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&TextureViewDescriptor::default());
        let mut blur = pyramid(device, layouts[0], sampler, extent);
        blur.bind_source(device, layouts, sampler, &view);
        store.filters.insert(
            entity,
            FilterTargets {
                offscreen: UiLayerTexture::detached(texture, view),
                pyramid: blur,
                pipelines,
            },
        );
    }
}

/// Draws `batches` into the layer, routing filter groups through their offscreen layer. Falls
/// back to drawing groups unblurred, and reports the layer incomplete so it redraws, while a
/// filter target or pipeline is not ready.
pub(super) fn draw_layer(
    context: &mut RenderContext,
    draw: &UiLayerDraw<'_>,
    batches: &[(usize, &UiRenderBatch, UiTextureLocation)],
    lifetime: &mut super::model_depth::ModelDepthLifetime,
    damage: Option<UiScissor>,
    publication: Option<&UiRenderInput>,
) -> LayerDrawn {
    if !batches.iter().any(|(_, batch, _)| filtered(batch)) {
        return draw_ui_layer(context, draw, batches, lifetime, damage);
    }
    let ready = ready(draw, publication);
    let mut encoded = false;
    let mut nonempty = false;
    let mut complete = ready.is_some();
    let mut start = 0;
    while start < batches.len() {
        let group = batches[start].1.filter_group;
        if is_filter_composite(batches[start].1) {
            start += 1;
            continue;
        }
        let end = start
            + batches[start..]
                .iter()
                .take_while(|(_, batch, _)| {
                    batch.filter_group == group && !is_filter_composite(batch)
                })
                .count();
        let run = &batches[start..end];
        let composite = batches
            .get(end)
            .filter(|(_, batch, _)| group.is_some() && is_filter_composite(batch));
        let next = end + usize::from(composite.is_some());
        start = next;
        if let (Some(composite), Some((targets, input, [downsample, apply]))) =
            (composite, ready.as_ref())
        {
            let offscreen = UiLayerDraw {
                layer: &targets.offscreen,
                keep: false,
                ..*draw
            };
            let drawn = draw_ui_layer(context, &offscreen, run, lifetime, None);
            complete &= drawn.complete;
            if drawn.encoded {
                let blur = Blur {
                    draw,
                    targets,
                    input,
                    pipelines: [downsample, apply],
                    clear: !encoded && damage.is_none(),
                };
                let (applied, content) = blur.composite(context, run, composite);
                encoded |= applied;
                nonempty |= content && drawn.nonempty;
            }
            continue;
        }
        let drawn = draw_ui_layer(
            context,
            &UiLayerDraw {
                keep: encoded,
                ..*draw
            },
            run,
            lifetime,
            damage,
        );
        encoded |= drawn.encoded;
        nonempty |= drawn.nonempty;
        complete &= drawn.complete;
    }
    LayerDrawn {
        encoded,
        nonempty,
        complete,
    }
}

/// The view's filter targets, the publication their bounds come from, and their compiled
/// pipelines, when all are available.
fn ready<'a>(
    draw: &UiLayerDraw<'a>,
    publication: Option<&'a UiRenderInput>,
) -> Option<(
    &'a FilterTargets,
    &'a UiRenderInput,
    [&'a RenderPipeline; 2],
)> {
    let targets = draw
        .world
        .get_resource::<UiBackdropStore>()?
        .filters
        .get(&draw.owner)?;
    let extent = crate::render_bounds::extent(&draw.layer.view);
    if targets.pyramid.scene != extent {
        return None;
    }
    let [Some(downsample), Some(apply)] = targets
        .pipelines
        .map(|id| draw.pipeline_cache.get_render_pipeline(id))
    else {
        return None;
    };
    Some((targets, publication?, [downsample, apply]))
}

/// One filter group's blur and composite into the retained layer.
struct Blur<'a> {
    draw: &'a UiLayerDraw<'a>,
    targets: &'a FilterTargets,
    input: &'a UiRenderInput,
    pipelines: [&'a RenderPipeline; 2],
    /// Clears the retained layer first, when nothing has drawn into it yet.
    clear: bool,
}

impl Blur<'_> {
    /// Blurs what `run` drew offscreen and composites it through `composite`'s geometry, within
    /// the run's bounds plus three standard deviations. Returns whether the layer was written.
    fn composite(
        &self,
        context: &mut RenderContext,
        run: &[(usize, &UiRenderBatch, UiTextureLocation)],
        composite: &(usize, &UiRenderBatch, UiTextureLocation),
    ) -> (bool, bool) {
        let stage = render_scene::RuntimeStage::GpuUiBackdrop;
        let profile = self.draw.world.get_resource::<profile::UiProfile>();
        let extent = self.targets.pyramid.scene;
        let sigma = super::backdrop_levels::batch_maximum(self.input, composite.1);
        let Some(bounds) = group_bounds(self.input, run, sigma, extent, self.draw.output_extent)
        else {
            return (false, false);
        };
        let pyramid = &self.targets.pyramid;
        let Some((_, source, shapes)) = pyramid.sources.first() else {
            return (false, false);
        };
        let levels = super::backdrop_levels::count(sigma, pyramid.levels.len());
        for (level, destination) in pyramid.levels.iter().take(levels).enumerate() {
            let input = if level == 0 {
                source
            } else {
                &pyramid.chain[level - 1]
            };
            let attachments = [Some(RenderPassColorAttachment {
                view: destination,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(Default::default()),
                    store: StoreOp::Store,
                },
            })];
            let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
                label: Some("UI element filter downsample"),
                color_attachments: &attachments,
                depth_stencil_attachment: None,
                timestamp_writes: crate::gpu_timing::ui_pass_timestamps(self.draw.world, stage),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let size = crate::render_bounds::extent(destination);
            let Some(rect) = level_rect(bounds, level as u32 + 1, size) else {
                continue;
            };
            pass.set_scissor_rect(rect.x, rect.y, rect.width, rect.height);
            pass.set_render_pipeline(self.pipelines[0]);
            pass.set_bind_group(0, input, &[]);
            pass.draw(0..3, 0..1);
            if let Some(profile) = profile {
                profile.record_pass(stage);
                profile.record_draw(stage, 0);
            }
        }
        let attachments = [Some(RenderPassColorAttachment {
            view: &self.draw.layer.view,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: if self.clear {
                    LoadOp::Clear(Default::default())
                } else {
                    LoadOp::Load
                },
                store: StoreOp::Store,
            },
        })];
        let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("UI element filter composite"),
            color_attachments: &attachments,
            depth_stencil_attachment: None,
            timestamp_writes: crate::gpu_timing::ui_pass_timestamps(self.draw.world, stage),
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_render_pipeline(self.pipelines[1]);
        pass.set_bind_group(1, shapes, &[]);
        let nonempty = draw_batches(
            &mut pass,
            self.draw.gpu,
            self.draw.vertices,
            self.draw.indices,
            self.draw.viewport,
            extent,
            self.draw.output_extent,
            std::slice::from_ref(composite),
            None,
            Some(bounds),
            profile.map(|profile| (profile, stage)),
        );
        (true, nonempty)
    }
}

/// The physical pixels `run` drew, grown by three standard deviations of `sigma` and kept
/// within `extent`.
pub(super) fn group_bounds(
    input: &UiRenderInput,
    run: &[(usize, &UiRenderBatch, UiTextureLocation)],
    sigma: f32,
    extent: [u32; 2],
    output_extent: Option<[u32; 2]>,
) -> Option<UiScissor> {
    let mut bounds = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for (_, batch, _) in run {
        let first = batch.first_index as usize;
        let indices = input
            .indices
            .get(first..first + batch.index_count as usize)?;
        for &index in indices {
            let vertex = input.vertices.get(index as usize)?;
            if vertex.clip_w <= 0.0 {
                continue;
            }
            for axis in 0..2 {
                let pixel = vertex.position[axis] / vertex.clip_w;
                bounds[axis] = bounds[axis].min(pixel);
                bounds[axis + 2] = bounds[axis + 2].max(pixel);
            }
        }
    }
    if let Some(output) = output_extent {
        if output.contains(&0) {
            return None;
        }
        for axis in 0..2 {
            let scale = extent[axis] as f32 / output[axis] as f32;
            bounds[axis] *= scale;
            bounds[axis + 2] *= scale;
        }
    }
    // The shader's blur radius remains in target pixels, including for scaled views.
    let spread = sigma.max(0.0) * 3.0 + 1.0;
    let left = (bounds[0] - spread).floor().max(0.0);
    let top = (bounds[1] - spread).floor().max(0.0);
    let right = (bounds[2] + spread).ceil().min(extent[0] as f32);
    let bottom = (bounds[3] + spread).ceil().min(extent[1] as f32);
    (left < right && top < bottom).then(|| {
        UiScissor::new(
            left as u32,
            top as u32,
            (right - left) as u32,
            (bottom - top) as u32,
        )
    })
}

/// `bounds` at pyramid level `level`, where each level halves the one above, grown by a
/// texel for the downsample's reach and kept within `size`.
fn level_rect(bounds: UiScissor, level: u32, size: [u32; 2]) -> Option<UiScissor> {
    let scale = 1u32 << level;
    let left = (bounds.x / scale).saturating_sub(1);
    let top = (bounds.y / scale).saturating_sub(1);
    let right = (bounds.x + bounds.width)
        .div_ceil(scale)
        .saturating_add(1)
        .min(size[0]);
    let bottom = (bounds.y + bounds.height)
        .div_ceil(scale)
        .saturating_add(1)
        .min(size[1]);
    (left < right && top < bottom).then(|| UiScissor::new(left, top, right - left, bottom - top))
}

#[cfg(test)]
#[path = "filter_tests.rs"]
mod tests;
