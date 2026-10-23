//! Native-resolution UI composition after the reduced world has finished rendering.

use super::upscale::{UpscaleKey, UpscalePipeline};
use crate::gpu_allocation::{self, FailedRequests};
use bevy::{
    core_pipeline::blit::{BlitPipeline, BlitPipelineKey},
    prelude::*,
    render::{
        render_resource::*,
        renderer::{RenderContext, RenderDevice},
        view::{PostProcessWrite, ViewTarget},
    },
};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

/// An output-sized scene pair used only by screen-space UI on a reduced world view.
#[derive(Component)]
pub(crate) struct NativeUiTarget {
    device: wgpu::Device,
    textures: [Texture; 2],
    views: [TextureView; 2],
    current: AtomicUsize,
    ready: AtomicBool,
    presented: AtomicBool,
    output_format: TextureFormat,
    sampler: Sampler,
    seed_pipelines: [CachedRenderPipelineId; 2],
    present_pipeline: CachedRenderPipelineId,
    seed_bindings: Mutex<Vec<(TextureViewId, BindGroup)>>,
    present_bindings: [BindGroup; 2],
}

impl NativeUiTarget {
    /// Allocates an independent scene pair, retaining the actual output's format for presentation.
    fn new(
        device: &RenderDevice,
        extent: [u32; 2],
        formats: [TextureFormat; 2],
        pipelines: [CachedRenderPipelineId; 3],
        blit: &BlitPipeline,
        cache: &PipelineCache,
    ) -> Option<Self> {
        let [scene_format, output_format] = formats;
        let bytes = u64::from(extent[0])
            * u64::from(extent[1])
            * u64::from(scene_format.block_copy_size(None)?);
        let allocate = || {
            gpu_allocation::checked(device, "native-resolution UI scene", bytes, || {
                device.create_texture(&TextureDescriptor {
                    label: Some("native-resolution UI scene"),
                    size: Extent3d {
                        width: extent[0],
                        height: extent[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format: scene_format,
                    usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
            })
        };
        let textures = [allocate()?, allocate()?];
        let views = textures
            .each_ref()
            .map(|texture| texture.create_view(&default()));
        let present_bindings = views
            .each_ref()
            .map(|view| blit.create_bind_group(device, view, cache));
        Some(Self {
            device: device.wgpu_device().clone(),
            textures,
            views,
            current: AtomicUsize::new(0),
            ready: AtomicBool::new(false),
            presented: AtomicBool::new(false),
            output_format,
            sampler: device.create_sampler(&SamplerDescriptor {
                label: Some("bilinear world upscale"),
                mag_filter: FilterMode::Linear,
                min_filter: FilterMode::Linear,
                ..default()
            }),
            seed_pipelines: [pipelines[0], pipelines[1]],
            present_pipeline: pipelines[2],
            seed_bindings: Mutex::new(Vec::with_capacity(2)),
            present_bindings,
        })
    }

    /// Keeps only this frame's native scene and presentation observations.
    fn begin_frame(&self) {
        self.current.store(0, Ordering::Relaxed);
        self.ready.store(false, Ordering::Relaxed);
        self.presented.store(false, Ordering::Relaxed);
    }

    /// The native output dimensions, independent of the world texture's resolution.
    pub(crate) fn extent(&self) -> [u32; 2] {
        [self.textures[0].width(), self.textures[0].height()]
    }

    /// Whether this frame's world has been copied into the native UI scene.
    pub(crate) fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Relaxed)
    }

    /// Whether the completed native UI scene has been encoded into the real output this frame.
    pub(crate) fn was_presented(&self) -> bool {
        self.presented.load(Ordering::Relaxed)
    }

    /// Records an externally encoded final UI composite only after its scene was seeded.
    pub(crate) fn mark_presented(&self) {
        if self.is_ready() {
            self.presented.store(true, Ordering::Relaxed);
        }
    }

    /// Refuses stale dimensions, output formats, or resources from a replaced render device.
    fn matches(&self, device: &RenderDevice, target: &ViewTarget, extent: [u32; 2]) -> bool {
        &self.device == device.wgpu_device()
            && self.textures[0].format() == target.main_texture_format()
            && self.extent() == extent
            && target.out_texture_view_format() == Some(self.output_format)
    }

    /// Upscales the completed world once, before any native HUD or menu batches are encoded.
    pub(crate) fn seed(
        &self,
        world: &World,
        context: &mut RenderContext,
        target: &ViewTarget,
    ) -> bool {
        if !self.matches(context.render_device(), target, self.extent())
            || target
                .out_texture()
                .map(|output| crate::render_bounds::extent(output))
                != Some(self.extent())
        {
            return false;
        }
        if self.is_ready() {
            return true;
        }
        let (Some(cache), Some(upscale)) = (
            world.get_resource::<PipelineCache>(),
            world.get_resource::<UpscalePipeline>(),
        ) else {
            return false;
        };
        let sharpen = world
            .get_resource::<crate::render_scale::RenderScaleSharpen>()
            .is_some_and(|filter| filter.0);
        let Some(pipeline) = cache.get_render_pipeline(self.seed_pipelines[usize::from(sharpen)])
        else {
            return false;
        };
        let source = target.main_texture_view();
        let live = [source.id(), target.main_texture_other_view().id()];
        let binding = {
            let mut bindings = self
                .seed_bindings
                .lock()
                .expect("native UI seed bindings lock");
            bindings.retain(|(id, _)| live.contains(id));
            if let Some((_, binding)) = bindings.iter().find(|(id, _)| *id == source.id()) {
                binding.clone()
            } else {
                let binding =
                    upscale.binding(context.render_device(), cache, source, &self.sampler);
                bindings.push((source.id(), binding.clone()));
                binding
            }
        };
        self.current.store(0, Ordering::Relaxed);
        encode_blit(
            world,
            context,
            &self.views[0],
            pipeline,
            &binding,
            "world upscale before native UI",
        );
        self.ready.store(true, Ordering::Relaxed);
        true
    }

    /// Presents even when no final alpha layer exists, including empty and invert-only HUDs.
    pub(crate) fn present(
        &self,
        world: &World,
        context: &mut RenderContext,
        target: &ViewTarget,
    ) -> bool {
        if !self.is_ready()
            || !self.matches(context.render_device(), target, self.extent())
            || target
                .out_texture()
                .map(|output| crate::render_bounds::extent(output))
                != Some(self.extent())
        {
            return false;
        }
        let Some(pipeline) = world
            .get_resource::<PipelineCache>()
            .and_then(|cache| cache.get_render_pipeline(self.present_pipeline))
        else {
            return false;
        };
        // Obtaining the attachment also records usage for Bevy's surface presentation gate.
        let Some(output) = target.out_texture_color_attachment(None) else {
            return false;
        };
        let index = self.current.load(Ordering::Relaxed);
        encode_blit(
            world,
            context,
            output.view,
            pipeline,
            &self.present_bindings[index],
            "native UI output",
        );
        self.mark_presented();
        true
    }

    /// Changes the live native scene only when the caller will write the entire destination.
    fn post_process_write(&self) -> PostProcessWrite<'_> {
        let source = self.current.fetch_xor(1, Ordering::Relaxed);
        let destination = source ^ 1;
        PostProcessWrite {
            source: &self.views[source],
            source_texture: &self.textures[source],
            destination: &self.views[destination],
            destination_texture: &self.textures[destination],
        }
    }
}

/// Encodes a full-coverage blit without sampling the output attachment itself.
fn encode_blit(
    world: &World,
    context: &mut RenderContext,
    destination: &wgpu::TextureView,
    pipeline: &RenderPipeline,
    binding: &BindGroup,
    label: &'static str,
) {
    let attachments = [Some(RenderPassColorAttachment {
        view: destination,
        depth_slice: None,
        resolve_target: None,
        ops: Operations {
            load: LoadOp::Clear(default()),
            store: StoreOp::Store,
        },
    })];
    let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some(label),
        color_attachments: &attachments,
        depth_stencil_attachment: None,
        timestamp_writes: crate::gpu_timing::ui_pass_timestamps(
            world,
            render_scene::RuntimeStage::GpuBlit,
        ),
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_render_pipeline(pipeline);
    pass.set_bind_group(0, binding, &[]);
    pass.draw(0..3, 0..1);
}

/// Allocates only for an admitted scaled view whose real output still matches its marker.
#[allow(clippy::too_many_arguments)] // View admission and the two pipeline owners share one allocation boundary.
pub(crate) fn prepare_native_ui_targets(
    mut commands: Commands,
    device: Res<RenderDevice>,
    cache: Res<PipelineCache>,
    blit: Option<Res<BlitPipeline>>,
    mut pipelines: Option<ResMut<SpecializedRenderPipelines<BlitPipeline>>>,
    mut upscale: Option<ResMut<UpscalePipeline>>,
    mut failed: Local<FailedRequests<(wgpu::Device, [u32; 2], TextureFormat, TextureFormat)>>,
    views: Query<(
        Entity,
        &ViewTarget,
        Option<&crate::render_scale::RenderScaleOutput>,
        Option<&NativeUiTarget>,
    )>,
    removed: Query<Entity, (With<NativeUiTarget>, Without<ViewTarget>)>,
) {
    failed.retain(|entity| {
        views
            .get(entity)
            .is_ok_and(|(_, _, scaled, _)| scaled.is_some())
    });
    for entity in &removed {
        commands.entity(entity).remove::<NativeUiTarget>();
    }
    for (entity, target, scaled, previous) in &views {
        let extent = scaled.map(|output| output.0.to_array());
        let admitted = extent.filter(|extent| {
            !extent.contains(&0)
                && extent
                    .iter()
                    .all(|&value| value <= device.limits().max_texture_dimension_2d)
                && target
                    .out_texture()
                    .map(|output| crate::render_bounds::extent(output))
                    == Some(*extent)
                && [
                    render_scene::SCENE_COLOR_FORMAT,
                    render_scene::SCENE_HDR_FORMAT,
                ]
                .contains(&target.main_texture_format())
        });
        let (Some(extent), Some(output_format)) = (admitted, target.out_texture_view_format())
        else {
            if previous.is_some() {
                commands.entity(entity).remove::<NativeUiTarget>();
            }
            continue;
        };
        if let Some(previous) =
            previous.filter(|previous| previous.matches(&device, target, extent))
        {
            previous.begin_frame();
            continue;
        }
        // Standalone UI preparation does not need the core pipeline's blit resources.
        let (Some(blit), Some(pipelines), Some(upscale)) = (
            blit.as_deref(),
            pipelines.as_deref_mut(),
            upscale.as_deref_mut(),
        ) else {
            if previous.is_some() {
                commands.entity(entity).remove::<NativeUiTarget>();
            }
            continue;
        };
        let scene_format = target.main_texture_format();
        let seeds = [false, true].map(|sharpen| {
            upscale.specialize(
                &cache,
                UpscaleKey {
                    format: scene_format,
                    sharpen,
                },
            )
        });
        let present = pipelines.specialize(
            &cache,
            blit,
            BlitPipelineKey {
                target_format: output_format,
                blend_state: None,
                samples: 1,
                source_space: None,
            },
        );
        let request = (
            device.wgpu_device().clone(),
            extent,
            scene_format,
            output_format,
        );
        if let Some(native) = failed.attempt(entity, request, || {
            NativeUiTarget::new(
                &device,
                extent,
                [scene_format, output_format],
                [seeds[0], seeds[1], present],
                blit,
                &cache,
            )
        }) {
            commands.entity(entity).insert(native);
        } else if previous.is_some() {
            commands.entity(entity).remove::<NativeUiTarget>();
        }
    }
}

/// Selects native screen composition without changing the world view or its depth attachments.
#[derive(Clone, Copy)]
pub(crate) struct UiTarget<'a> {
    output: &'a ViewTarget,
    native: Option<&'a NativeUiTarget>,
}

impl<'a> UiTarget<'a> {
    /// Preparation selects the native extent before the world's pixels are seeded later.
    pub(crate) fn new(output: &'a ViewTarget, native: Option<&'a NativeUiTarget>) -> Self {
        Self { output, native }
    }

    /// Reports whether screen UI has its own output-sized scene.
    pub(crate) fn is_native(self) -> bool {
        self.native.is_some()
    }
    /// Returns the current scene storage used by UI composition.
    pub(crate) fn main_texture(self) -> &'a Texture {
        self.native.map_or_else(
            || self.output.main_texture(),
            |native| &native.textures[native.current.load(Ordering::Relaxed)],
        )
    }
    #[cfg(test)]
    /// Returns the other scene storage for ping-pong lifetime checks.
    pub(crate) fn main_texture_other(self) -> &'a Texture {
        self.native.map_or_else(
            || self.output.main_texture_other(),
            |native| &native.textures[native.current.load(Ordering::Relaxed) ^ 1],
        )
    }
    /// Returns the current scene view used by UI draws.
    pub(crate) fn main_texture_view(self) -> &'a TextureView {
        self.native.map_or_else(
            || self.output.main_texture_view(),
            |native| &native.views[native.current.load(Ordering::Relaxed)],
        )
    }
    /// Returns the scene view reserved for the next full-coverage composite.
    pub(crate) fn main_texture_other_view(self) -> &'a TextureView {
        self.native.map_or_else(
            || self.output.main_texture_other_view(),
            |native| &native.views[native.current.load(Ordering::Relaxed) ^ 1],
        )
    }
    /// Swaps scene views only for a caller that writes the whole destination.
    pub(crate) fn post_process_write(self) -> PostProcessWrite<'a> {
        self.native.map_or_else(
            || self.output.post_process_write(),
            NativeUiTarget::post_process_write,
        )
    }
    /// Borrows the native presentation attachment independently of scene size.
    pub(crate) fn out_texture(self) -> Option<&'a TextureView> {
        self.output.out_texture()
    }
    /// Acquires the final attachment and records its use for presentation.
    pub(crate) fn out_texture_color_attachment(
        self,
        clear: Option<LinearRgba>,
    ) -> Option<RenderPassColorAttachment<'a>> {
        self.output.out_texture_color_attachment(clear)
    }
}

#[cfg(test)]
#[path = "target_tests.rs"]
mod tests;
