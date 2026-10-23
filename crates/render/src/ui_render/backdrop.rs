//! Mod-surface backdrops: glass panels and the world backdrop behind an open menu.
//!
//! When the accepted UI publication holds backdrop batches ([`UI_BLEND_BACKDROP`]), the overlay
//! builds one dual-Kawase blur pyramid from the post-processed scene, then writes the next main
//! texture as a copy of the scene with every backdrop shape drawn over it, sampling the
//! pyramid at the level its blur needs. This runs before the first UI layer composites, so the
//! retained layers never hold scene pixels and a changing scene needs no layer redraw. Without
//! backdrop batches no pass runs and no pyramid stays allocated.

use super::*;
use crate::gpu_allocation::{FailedRequests, checked};
use bevy::render::{
    render_resource::{
        BindGroup, BindGroupEntries, BindGroupLayout, Extent3d, LoadOp, Operations,
        RenderPassColorAttachment, RenderPassDescriptor, StoreOp, TextureDescriptor,
        TextureDimension, TextureUsages, TextureView, TextureViewDescriptor, TextureViewId,
    },
    renderer::RenderContext,
};
use render_model::{UI_BLEND_BACKDROP, UiTextureLocation};
use std::collections::HashMap;

pub(crate) const UI_BACKDROP_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("2f0e6d3a-58c4-4b8e-9d47-6a1c0b9e3f52");
pub(crate) const UI_BACKDROP_FILTER_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("b7c41e09-2d6a-4f35-8e1b-94d3a6f0c218");
/// Downsampled levels in the shared pyramid. Level `L` reconstructs a blur with a standard
/// deviation of about `0.82 * 2^L` scene pixels, so the last level covers
/// [`render_api::UI_BACKDROP_MAX_BLUR_PIXELS`].
pub(crate) const PYRAMID_LEVELS: u32 = 8;
/// Pyramid texels keep HDR range and dark gradients without banding.
const PYRAMID_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

const _: () = assert!(
    // 2/3 * 4^L - 1/4 is the reconstructed variance at level L (see `ui_backdrop.wgsl`).
    (2 << (2 * PYRAMID_LEVELS)) / 3
        >= (render_api::UI_BACKDROP_MAX_BLUR_PIXELS as u32
            * render_api::UI_BACKDROP_MAX_BLUR_PIXELS as u32),
    "the smallest pyramid level must reach the largest backdrop blur"
);

/// Whether a batch shows the processed scene instead of drawing into a UI layer.
pub(crate) fn is_backdrop(batch: &UiRenderBatch) -> bool {
    batch.blend_mode == UI_BLEND_BACKDROP
}

/// The pyramid's first level and mip count for a scene of `size` physical pixels.
pub(crate) fn pyramid_extent(size: [u32; 2]) -> ([u32; 2], u32) {
    let base = size.map(|side| (side / 2).max(1));
    let mips = 32 - base[0].max(base[1]).leading_zeros();
    (base, mips.min(PYRAMID_LEVELS))
}

#[derive(Resource)]
pub(crate) struct UiBackdropPipeline {
    pub(super) filter_layout: BindGroupLayoutDescriptor,
    pub(super) apply_layout: BindGroupLayoutDescriptor,
    downsample: Option<CachedRenderPipelineId>,
    /// The element-filter composite into the UI layer.
    filter: Option<CachedRenderPipelineId>,
    /// Copy and apply pipelines for each main-texture format.
    formats: HashMap<TextureFormat, (CachedRenderPipelineId, CachedRenderPipelineId)>,
}

impl FromWorld for UiBackdropPipeline {
    fn from_world(_world: &mut World) -> Self {
        let texture = |binding| BindGroupLayoutEntry {
            binding,
            visibility: ShaderStages::FRAGMENT,
            ty: BindingType::Texture {
                sample_type: TextureSampleType::Float { filterable: true },
                view_dimension: TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let sampler = |binding| BindGroupLayoutEntry {
            binding,
            visibility: ShaderStages::FRAGMENT,
            ty: BindingType::Sampler(SamplerBindingType::Filtering),
            count: None,
        };
        Self {
            filter_layout: BindGroupLayoutDescriptor::new(
                "UI backdrop filter layout",
                &[texture(0), sampler(1)],
            ),
            apply_layout: BindGroupLayoutDescriptor::new(
                "UI backdrop scene layout",
                &[texture(0), texture(1), sampler(2)],
            ),
            downsample: None,
            filter: None,
            formats: HashMap::new(),
        }
    }
}

impl UiBackdropPipeline {
    /// A fullscreen filter writing `format` through `entry`.
    fn filter_descriptor(
        &self,
        entry: &'static str,
        format: TextureFormat,
    ) -> RenderPipelineDescriptor {
        RenderPipelineDescriptor {
            label: Some("UI backdrop filter".into()),
            layout: vec![self.filter_layout.clone()],
            vertex: VertexState {
                shader: UI_BACKDROP_FILTER_SHADER_HANDLE,
                entry_point: Some("fullscreen_vertex".into()),
                ..default()
            },
            fragment: Some(FragmentState {
                shader: UI_BACKDROP_FILTER_SHADER_HANDLE,
                entry_point: Some(entry.into()),
                targets: vec![Some(ColorTargetState {
                    format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        }
    }

    /// Shapes drawn into `format` through `entry`, reading UI vertices and the viewport.
    fn apply_descriptor(
        &self,
        format: TextureFormat,
        entry: &'static str,
    ) -> RenderPipelineDescriptor {
        let ui = pipeline::ui_pipeline_descriptor(pipeline::ui_bind_group_layout());
        RenderPipelineDescriptor {
            label: Some("UI backdrop shapes".into()),
            layout: vec![pipeline::ui_bind_group_layout(), self.apply_layout.clone()],
            vertex: VertexState {
                shader: UI_BACKDROP_SHADER_HANDLE,
                entry_point: Some("backdrop_vertex".into()),
                buffers: ui.vertex.buffers,
                ..default()
            },
            fragment: Some(FragmentState {
                shader: UI_BACKDROP_SHADER_HANDLE,
                entry_point: Some(entry.into()),
                targets: vec![Some(ColorTargetState {
                    format,
                    blend: Some(pipeline::ui_alpha_blend_state()),
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        }
    }

    /// Queues the pyramid downsample once.
    pub(super) fn downsample(&mut self, cache: &PipelineCache) -> CachedRenderPipelineId {
        match self.downsample {
            Some(id) => id,
            None => {
                let descriptor = self.filter_descriptor("downsample", PYRAMID_FORMAT);
                *self
                    .downsample
                    .insert(cache.queue_render_pipeline(descriptor))
            }
        }
    }

    /// Queues the element-filter composite into the UI layer once.
    pub(super) fn filter_composite(&mut self, cache: &PipelineCache) -> CachedRenderPipelineId {
        match self.filter {
            Some(id) => id,
            None => {
                let descriptor =
                    self.apply_descriptor(composite::UI_LAYER_FORMAT, "filter_fragment");
                *self.filter.insert(cache.queue_render_pipeline(descriptor))
            }
        }
    }

    /// Queues the downsample and the copy/apply pair for a view's main texture, once each.
    fn specialize(
        &mut self,
        cache: &PipelineCache,
        hdr: bool,
    ) -> (
        CachedRenderPipelineId,
        CachedRenderPipelineId,
        CachedRenderPipelineId,
    ) {
        let format = if hdr {
            render_scene::SCENE_HDR_FORMAT
        } else {
            render_scene::SCENE_COLOR_FORMAT
        };
        let downsample = match self.downsample {
            Some(id) => id,
            None => {
                let id = cache
                    .queue_render_pipeline(self.filter_descriptor("downsample", PYRAMID_FORMAT));
                *self.downsample.insert(id)
            }
        };
        if !self.formats.contains_key(&format) {
            let copy = cache.queue_render_pipeline(self.filter_descriptor("copy", format));
            let apply =
                cache.queue_render_pipeline(self.apply_descriptor(format, "backdrop_fragment"));
            self.formats.insert(format, (copy, apply));
        }
        let (copy, apply) = self.formats[&format];
        (downsample, copy, apply)
    }
}

impl crate::pipeline_warmup::PrewarmPipelines for UiBackdropPipeline {
    /// Compiles every backdrop pass while loading, so opening glass never compiles mid-play.
    fn prewarm(
        &mut self,
        cache: &PipelineCache,
        view: crate::pipeline_warmup::WarmView,
        ids: &mut crate::pipeline_warmup::WarmupIds,
    ) -> Result<(), BevyError> {
        let (downsample, copy, apply) = self.specialize(cache, view.hdr);
        ids.extend([downsample, copy, apply, self.filter_composite(cache)]);
        Ok(())
    }
}

/// One view's blur pyramid and the bind groups between its own levels.
pub(super) struct Pyramid {
    pub(super) scene: [u32; 2],
    /// Each level as a render target and as a single-level source.
    pub(super) levels: Vec<TextureView>,
    /// Every level, sampled by backdrop shapes.
    pub(super) all: TextureView,
    /// Level `m` read while writing level `m + 1`.
    pub(super) chain: Vec<BindGroup>,
    /// For each texture the source may be in: its filter source and its shape bindings.
    pub(super) sources: Vec<(TextureViewId, BindGroup, BindGroup)>,
}

impl Pyramid {
    /// Binds `source` as this pyramid's first input and as what its shapes sample, once.
    pub(super) fn bind_source(
        &mut self,
        device: &RenderDevice,
        layouts: [&BindGroupLayout; 2],
        sampler: &Sampler,
        source: &TextureView,
    ) {
        if self.sources.iter().any(|(id, _, _)| *id == source.id()) {
            return;
        }
        let filter = device.create_bind_group(
            "UI backdrop source",
            layouts[0],
            &BindGroupEntries::sequential((source, sampler)),
        );
        let shapes = device.create_bind_group(
            "UI backdrop shapes",
            layouts[1],
            &BindGroupEntries::sequential((source, &self.all, sampler)),
        );
        self.sources.push((source.id(), filter, shapes));
    }
}

/// Per-view pyramids, kept only while the accepted publication draws backdrops.
#[derive(Default, Resource)]
pub(crate) struct UiBackdropStore {
    device: Option<wgpu::Device>,
    sampler: Option<Sampler>,
    views: HashMap<Entity, Pyramid>,
    /// Pipelines for each view this frame: downsample, copy and apply.
    pipelines: HashMap<Entity, [CachedRenderPipelineId; 3]>,
    /// Per-view element-filter targets, kept only while filters are published.
    pub(super) filters: HashMap<Entity, super::filter::FilterTargets>,
    /// Extents whose pyramid or filter targets ran out of memory; retried on a resize.
    failed_backdrops: FailedRequests<[u32; 2]>,
    pub(super) failed_filters: FailedRequests<[u32; 2]>,
}

impl UiBackdropStore {
    /// Whether any view holds a pyramid; false whenever no backdrop is published.
    #[cfg(test)]
    pub(crate) fn allocated(&self) -> bool {
        !self.views.is_empty()
    }
}

/// Allocates pyramids and queues pipelines for each view while backdrops are published, and
/// releases them as soon as none are.
pub(crate) fn prepare_ui_backdrops(
    mut store: ResMut<UiBackdropStore>,
    mut pipeline: ResMut<UiBackdropPipeline>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    views: Query<(Entity, &ViewTarget, &bevy::render::camera::ExtractedCamera)>,
    gpu: Option<Res<UiGpu>>,
) {
    let store = &mut *store;
    store.failed_backdrops.retain(|view| views.contains(view));
    store.failed_filters.retain(|view| views.contains(view));
    if store.device.as_ref() != Some(device.wgpu_device()) {
        store.views.clear();
        store.filters.clear();
        store.failed_backdrops = Default::default();
        store.failed_filters = Default::default();
        store.sampler = None;
        store.device = Some(device.wgpu_device().clone());
    }
    let published = |wanted: fn(&UiRenderBatch) -> bool| {
        gpu.as_ref()
            .is_some_and(|gpu| gpu.accepted_revision.is_some() && gpu.batches.iter().any(wanted))
    };
    let (backdrops, filters) = (
        published(is_backdrop),
        published(super::filter::is_filter_composite),
    );
    store.pipelines.clear();
    if !backdrops {
        store.views.clear();
    }
    if !filters {
        store.filters.clear();
    }
    if !backdrops && !filters {
        return;
    }
    store.views.retain(|entity, _| views.contains(*entity));
    store.filters.retain(|entity, _| views.contains(*entity));
    let sampler = store
        .sampler
        .get_or_insert_with(|| {
            device.create_sampler(&SamplerDescriptor {
                label: Some("UI backdrop pyramid sampler"),
                address_mode_u: AddressMode::ClampToEdge,
                address_mode_v: AddressMode::ClampToEdge,
                mag_filter: FilterMode::Linear,
                min_filter: FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..default()
            })
        })
        .clone();
    let filter = cache.get_bind_group_layout(&pipeline.filter_layout);
    let shapes = cache.get_bind_group_layout(&pipeline.apply_layout);
    if filters {
        super::filter::prepare(
            store,
            &mut pipeline,
            &cache,
            &device,
            [&filter, &shapes],
            &sampler,
            views.iter().map(|(entity, target, _)| (entity, target)),
        );
    }
    if !backdrops {
        return;
    }
    for (entity, target, camera) in &views {
        let size = target.main_texture().size();
        let scene = [size.width, size.height];
        if store
            .views
            .get(&entity)
            .is_none_or(|pyramid| pyramid.scene != scene)
        {
            // Without a pyramid the backdrop draws over the unblurred scene.
            store.views.remove(&entity);
            match store
                .failed_backdrops
                .attempt(entity, scene, || pyramid(&device, &filter, &sampler, scene))
            {
                Some(pyramid) => store.views.insert(entity, pyramid),
                None => continue,
            };
        }
        let pyramid = store.views.get_mut(&entity).expect("inserted above");
        let mains = [target.main_texture_view(), target.main_texture_other_view()];
        pyramid
            .sources
            .retain(|(id, _, _)| mains.iter().any(|main| main.id() == *id));
        for main in mains {
            pyramid.bind_source(&device, [&filter, &shapes], &sampler, main);
        }
        let (downsample, copy, apply) = pipeline.specialize(&cache, camera.hdr);
        store.pipelines.insert(entity, [downsample, copy, apply]);
    }
}

/// Allocates the levels below `scene` and binds each as the next one's source; `None` when out
/// of memory.
pub(super) fn pyramid(
    device: &RenderDevice,
    layout: &BindGroupLayout,
    sampler: &Sampler,
    scene: [u32; 2],
) -> Option<Pyramid> {
    let ([width, height], mips) = pyramid_extent(scene);
    // The mip chain adds a third to the base level.
    let bytes = u64::from(width) * u64::from(height) * 8 * 4 / 3;
    let texture = checked(device, "UI backdrop blur pyramid", bytes, || {
        device.create_texture(&TextureDescriptor {
            label: Some("UI backdrop blur pyramid"),
            size: Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: mips,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: PYRAMID_FORMAT,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
    })?;
    let levels: Vec<_> = (0..mips)
        .map(|mip| {
            texture.create_view(&TextureViewDescriptor {
                label: Some("UI backdrop pyramid level"),
                base_mip_level: mip,
                mip_level_count: Some(1),
                ..default()
            })
        })
        .collect();
    let chain = levels[..levels.len() - 1]
        .iter()
        .map(|level| {
            device.create_bind_group(
                "UI backdrop pyramid level",
                layout,
                &BindGroupEntries::sequential((level, sampler)),
            )
        })
        .collect();
    Some(Pyramid {
        scene,
        all: texture.create_view(&TextureViewDescriptor::default()),
        levels,
        chain,
        sources: Vec::new(),
    })
}

/// What [`apply_backdrops`] draws with besides the view's batches.
pub(super) struct BackdropDraw<'a> {
    pub(super) world: &'a World,
    pub(super) gpu: &'a UiGpu,
    pub(super) vertices: &'a Buffer,
    pub(super) indices: &'a Buffer,
    pub(super) view: Entity,
    pub(super) viewport: Option<&'a bevy::camera::Viewport>,
}

/// Blurs the scene once and draws `backdrops` over it into the next main texture. Returns false,
/// leaving the scene untouched, while a pipeline still compiles or no pyramid is resident.
pub(super) fn apply_backdrops(
    context: &mut RenderContext,
    draw: &BackdropDraw<'_>,
    target: &ViewTarget,
    backdrops: &[(usize, &UiRenderBatch, UiTextureLocation)],
) -> bool {
    let world = draw.world;
    let (Some(store), Some(cache)) = (
        world.get_resource::<UiBackdropStore>(),
        world.get_resource::<PipelineCache>(),
    ) else {
        return false;
    };
    let (Some(pyramid), Some(ids)) = (store.views.get(&draw.view), store.pipelines.get(&draw.view))
    else {
        return false;
    };
    let [Some(downsample), Some(copy), Some(apply)] = ids.map(|id| cache.get_render_pipeline(id))
    else {
        return false;
    };
    let Some((_, scene_source, shapes)) = pyramid
        .sources
        .iter()
        .find(|(id, _, _)| *id == target.main_texture_view().id())
    else {
        return false;
    };
    if pyramid.scene != crate::render_bounds::extent(target.main_texture_view()) {
        return false;
    }
    let profile = world.get_resource::<profile::UiProfile>();
    let stage = render_scene::RuntimeStage::GpuUiBackdrop;
    let post = target.post_process_write();
    let levels = super::backdrop_levels::count(draw.gpu.backdrop_blur, pyramid.levels.len());
    for (level, destination) in pyramid.levels.iter().take(levels).enumerate() {
        let source = if level == 0 {
            scene_source
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
            label: Some("UI backdrop downsample"),
            color_attachments: &attachments,
            depth_stencil_attachment: None,
            timestamp_writes: crate::gpu_timing::ui_pass_timestamps(world, stage),
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_render_pipeline(downsample);
        pass.set_bind_group(0, source, &[]);
        pass.draw(0..3, 0..1);
        if let Some(profile) = profile {
            profile.record_pass(stage);
            profile.record_draw(stage, 0);
        }
    }
    let attachments = [Some(RenderPassColorAttachment {
        view: post.destination,
        depth_slice: None,
        resolve_target: None,
        ops: Operations {
            load: LoadOp::Clear(Default::default()),
            store: StoreOp::Store,
        },
    })];
    let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("UI backdrop shapes"),
        color_attachments: &attachments,
        depth_stencil_attachment: None,
        timestamp_writes: crate::gpu_timing::ui_pass_timestamps(world, stage),
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_render_pipeline(copy);
    pass.set_bind_group(0, scene_source, &[]);
    pass.draw(0..3, 0..1);
    pass.set_render_pipeline(apply);
    pass.set_bind_group(1, shapes, &[]);
    layer::draw_batches(
        &mut pass,
        draw.gpu,
        draw.vertices,
        draw.indices,
        draw.viewport,
        crate::render_bounds::extent(post.destination),
        backdrops,
        None,
        None,
        profile.map(|profile| (profile, stage)),
    );
    true
}

#[cfg(test)]
#[path = "backdrop_raster_tests.rs"]
mod raster_tests;
#[cfg(test)]
#[path = "backdrop_tests.rs"]
mod tests;
