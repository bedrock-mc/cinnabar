//! Render-world Enhanced resources: bind group layouts, fallbacks, material
//! classes, and per-view uniforms, targets and bind groups.

use std::{collections::HashMap, num::NonZeroU64};

use bevy::{
    ecs::{
        query::ROQueryItem,
        system::{SystemParamItem, lifetimeless::SRes},
    },
    math::{Mat4, UVec4, Vec4},
    prelude::{Entity, FromWorld, Has, Query, Res, ResMut, Resource, Time, With, World, default},
    render::{
        render_phase::{PhaseItem, RenderCommand, RenderCommandResult, TrackedRenderPass},
        render_resource::{
            AddressMode, BindGroup, BindGroupEntry, BindGroupLayoutDescriptor,
            BindGroupLayoutEntry, BindingResource, BindingType, Buffer, BufferBinding,
            BufferBindingType, BufferDescriptor, BufferUsages, CompareFunction, Extent3d,
            FilterMode, Origin3d, PipelineCache, Sampler, SamplerBindingType, SamplerDescriptor,
            ShaderStages, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture, TextureDescriptor,
            TextureDimension, TextureFormat, TextureSampleType, TextureUsages, TextureView,
            TextureViewDescriptor, TextureViewDimension,
        },
        renderer::{RenderDevice, RenderQueue},
        texture::{CachedTexture, TextureCache},
        view::{ExtractedView, ViewDepthTexture, ViewTarget},
    },
};

use super::{
    EnhancedRendering,
    frame::{CascadeBounds, EnhancedFrameGpu, ViewInputs, build_frame},
    materials::material_classes,
};
use crate::{
    AtmosphereFrame, ChunkTextureAssetIdentity, ChunkTextureAssets, scene_sampling::ResolvedDepth,
};

pub(crate) const CASTER_SLOT_BYTES: u64 = 256;
const CASTER_UNIFORM_BYTES: u64 = 96;
pub(crate) const SHADOW_FORMAT: TextureFormat = TextureFormat::Depth32Float;
pub(crate) const POST_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

/// Mirrors `CasterUniform` in enhanced/caster.wgsl, padded to one dynamic slot.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CasterUniformGpu {
    clip_from_world: Mat4,
    params: Vec4,
    flags: UVec4,
    padding: [Vec4; 10],
}

const _: () = assert!(std::mem::size_of::<CasterUniformGpu>() as u64 == CASTER_SLOT_BYTES);

/// Builds a numbered bind-group entry.
fn entry(binding: u32, visibility: ShaderStages, ty: BindingType) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility,
        ty,
        count: None,
    }
}

/// Describes a uniform binding with its validated size.
fn uniform(min_size: u64, dynamic: bool) -> BindingType {
    BindingType::Buffer {
        ty: BufferBindingType::Uniform,
        has_dynamic_offset: dynamic,
        min_binding_size: NonZeroU64::new(min_size),
    }
}

/// Describes a sampled texture binding.
fn texture(sample_type: TextureSampleType, view_dimension: TextureViewDimension) -> BindingType {
    BindingType::Texture {
        sample_type,
        view_dimension,
        multisampled: false,
    }
}

const FLOAT: TextureSampleType = TextureSampleType::Float { filterable: true };
const FRAME_BYTES: u64 = std::mem::size_of::<EnhancedFrameGpu>() as u64;

/// Group 1 of every Enhanced chunk, model and liquid pipeline.
pub(crate) fn enhanced_view_layout() -> BindGroupLayoutDescriptor {
    let both = ShaderStages::VERTEX_FRAGMENT;
    let fragment = ShaderStages::FRAGMENT;
    BindGroupLayoutDescriptor::new(
        "enhanced view bind group layout",
        &[
            entry(0, both, uniform(FRAME_BYTES, false)),
            entry(
                1,
                fragment,
                texture(TextureSampleType::Depth, TextureViewDimension::D2Array),
            ),
            entry(
                2,
                fragment,
                BindingType::Sampler(SamplerBindingType::Comparison),
            ),
            entry(
                3,
                both,
                texture(TextureSampleType::Uint, TextureViewDimension::D2),
            ),
            entry(4, fragment, texture(FLOAT, TextureViewDimension::D2)),
            entry(
                5,
                fragment,
                texture(TextureSampleType::Depth, TextureViewDimension::D2),
            ),
            entry(
                6,
                fragment,
                BindingType::Sampler(SamplerBindingType::Filtering),
            ),
        ],
    )
}

/// Group 2 of the depth-only shadow-caster pipelines.
pub(crate) fn enhanced_caster_layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "enhanced shadow caster bind group layout",
        &[
            entry(0, ShaderStages::VERTEX, uniform(CASTER_UNIFORM_BYTES, true)),
            entry(
                1,
                ShaderStages::VERTEX,
                texture(TextureSampleType::Uint, TextureViewDimension::D2),
            ),
        ],
    )
}

/// Single layout shared by every fullscreen post pass.
pub(crate) fn enhanced_post_layout() -> BindGroupLayoutDescriptor {
    let fragment = ShaderStages::FRAGMENT;
    BindGroupLayoutDescriptor::new(
        "enhanced post bind group layout",
        &[
            entry(0, fragment, uniform(FRAME_BYTES, false)),
            entry(1, fragment, texture(FLOAT, TextureViewDimension::D2)),
            entry(
                2,
                fragment,
                BindingType::Sampler(SamplerBindingType::Filtering),
            ),
            entry(3, fragment, texture(FLOAT, TextureViewDimension::D2)),
            entry(4, fragment, texture(FLOAT, TextureViewDimension::D2)),
            entry(
                5,
                fragment,
                texture(TextureSampleType::Depth, TextureViewDimension::D2),
            ),
            entry(
                6,
                fragment,
                texture(TextureSampleType::Depth, TextureViewDimension::D2Array),
            ),
            entry(
                7,
                fragment,
                BindingType::Sampler(SamplerBindingType::Comparison),
            ),
        ],
    )
}

/// Creates an unused placeholder for disabled effects.
fn fallback_texture(
    device: &RenderDevice,
    label: &'static str,
    format: TextureFormat,
    dimension: TextureViewDimension,
) -> TextureView {
    device
        .create_texture(&TextureDescriptor {
            label: Some(label),
            size: Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format,
            usage: TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&TextureViewDescriptor {
            dimension: Some(dimension),
            ..default()
        })
}

/// Device-lifetime Enhanced objects shared by every view.
#[derive(Resource)]
pub(crate) struct EnhancedGpu {
    pub(crate) shadow_sampler: Sampler,
    pub(crate) linear_sampler: Sampler,
    pub(crate) fallback_shadow: TextureView,
    pub(crate) fallback_colour: TextureView,
    pub(crate) fallback_depth: TextureView,
    pub(crate) materials: TextureView,
    material_identity: Option<ChunkTextureAssetIdentity>,
}

impl FromWorld for EnhancedGpu {
    fn from_world(world: &mut World) -> Self {
        let device = world.resource::<RenderDevice>();
        Self {
            shadow_sampler: device.create_sampler(&SamplerDescriptor {
                label: Some("enhanced shadow comparison sampler"),
                address_mode_u: AddressMode::ClampToEdge,
                address_mode_v: AddressMode::ClampToEdge,
                mag_filter: FilterMode::Linear,
                min_filter: FilterMode::Linear,
                compare: Some(CompareFunction::LessEqual),
                ..default()
            }),
            linear_sampler: device.create_sampler(&SamplerDescriptor {
                label: Some("enhanced linear clamp sampler"),
                address_mode_u: AddressMode::ClampToEdge,
                address_mode_v: AddressMode::ClampToEdge,
                mag_filter: FilterMode::Linear,
                min_filter: FilterMode::Linear,
                ..default()
            }),
            fallback_shadow: fallback_texture(
                device,
                "enhanced fallback shadow",
                SHADOW_FORMAT,
                TextureViewDimension::D2Array,
            ),
            fallback_colour: fallback_texture(
                device,
                "enhanced fallback colour",
                POST_FORMAT,
                TextureViewDimension::D2,
            ),
            fallback_depth: fallback_texture(
                device,
                "enhanced fallback depth",
                SHADOW_FORMAT,
                TextureViewDimension::D2,
            ),
            materials: fallback_texture(
                device,
                "enhanced fallback material classes",
                TextureFormat::R32Uint,
                TextureViewDimension::D2,
            ),
            material_identity: None,
        }
    }
}

/// Uploads the class table only when an Enhanced view needs a new palette.
pub(crate) fn prepare_enhanced_materials(
    assets: Res<ChunkTextureAssets>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut gpu: ResMut<EnhancedGpu>,
    views: Query<(), With<EnhancedRendering>>,
) {
    let identity = assets.identity();
    if views.is_empty() || gpu.material_identity == Some(identity) {
        return;
    }
    let mut classes = material_classes(assets.assets());
    if classes.is_empty() {
        classes.push(0);
    }
    let width = 256_u32.min(device.limits().max_texture_dimension_2d);
    let height = (classes.len() as u32).div_ceil(width).max(1);
    if height > device.limits().max_texture_dimension_2d {
        bevy::log::warn!(
            "Enhanced material table exceeds the texture limit; material effects disabled"
        );
        gpu.materials = fallback_texture(
            &device,
            "enhanced oversized material fallback",
            TextureFormat::R32Uint,
            TextureViewDimension::D2,
        );
    } else {
        classes.resize(width as usize * height as usize, 0);
        let size = Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let table = device.create_texture(&TextureDescriptor {
            label: Some("enhanced material classes"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::R32Uint,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &table,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: Default::default(),
            },
            bytemuck::cast_slice(&classes),
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            size,
        );
        gpu.materials = table.create_view(&TextureViewDescriptor::default());
    }
    gpu.material_identity = Some(identity);
}

/// Cascaded shadow depth array owned by one view.
pub(crate) struct ShadowTargets {
    _texture: Texture,
    pub(crate) array: TextureView,
    pub(crate) layers: Vec<TextureView>,
    resolution: u32,
}

impl ShadowTargets {
    /// Creates one independently rendered depth layer per cascade.
    fn new(device: &RenderDevice, resolution: u32, cascades: u32) -> Self {
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("enhanced cascaded shadow map"),
            size: Extent3d {
                width: resolution,
                height: resolution,
                depth_or_array_layers: cascades,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: SHADOW_FORMAT,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let array = texture.create_view(&TextureViewDescriptor {
            label: Some("enhanced shadow array view"),
            dimension: Some(TextureViewDimension::D2Array),
            ..default()
        });
        let layers = (0..cascades)
            .map(|layer| {
                texture.create_view(&TextureViewDescriptor {
                    label: Some("enhanced shadow cascade view"),
                    dimension: Some(TextureViewDimension::D2),
                    base_array_layer: layer,
                    array_layer_count: Some(1),
                    ..default()
                })
            })
            .collect();
        Self {
            _texture: texture,
            array,
            layers,
            resolution,
        }
    }
}

/// Per-view Enhanced state, rebuilt from the frame each render update.
pub(crate) struct EnhancedViewGpu {
    pub(crate) frame: Buffer,
    pub(crate) casters: Buffer,
    pub(crate) settings: EnhancedRendering,
    pub(crate) cascades: Vec<CascadeBounds>,
    pub(crate) shadow: Option<ShadowTargets>,
    pub(crate) scene_colour: Option<CachedTexture>,
    pub(crate) scene_depth: Option<CachedTexture>,
    pub(crate) resolved_depth: Option<ResolvedDepth>,
    pub(crate) shafts: Option<CachedTexture>,
    pub(crate) hand_layer: Option<super::hand_layer::HandLayer>,
    pub(crate) view_bind_group: Option<BindGroup>,
    pub(crate) caster_bind_group: Option<BindGroup>,
}

impl EnhancedViewGpu {
    /// Allocates the fixed-size per-view uniforms.
    fn new(device: &RenderDevice, settings: EnhancedRendering) -> Self {
        let buffer = |label, size| {
            device.create_buffer(&BufferDescriptor {
                label: Some(label),
                size,
                usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        Self {
            frame: buffer("enhanced frame uniform", FRAME_BYTES),
            casters: buffer(
                "enhanced caster uniforms",
                CASTER_SLOT_BYTES * u64::from(super::MAX_SHADOW_CASCADES),
            ),
            settings,
            cascades: Vec::new(),
            shadow: None,
            scene_colour: None,
            scene_depth: None,
            resolved_depth: None,
            shafts: None,
            hand_layer: None,
            view_bind_group: None,
            caster_bind_group: None,
        }
    }
}

#[derive(Resource, Default)]
pub(crate) struct EnhancedViews(pub(crate) HashMap<Entity, EnhancedViewGpu>);

/// Collects the camera matrices and frustum slopes.
fn view_inputs(view: &ExtractedView, seconds: f32) -> ViewInputs {
    let world_from_view = view.world_from_view.to_matrix();
    let clip_from_world = view
        .clip_from_world
        .unwrap_or(view.clip_from_view * world_from_view.inverse());
    let projection = view.clip_from_view;
    let slope = (1.0 / projection.x_axis.x).hypot(1.0 / projection.y_axis.y);
    ViewInputs {
        clip_from_world,
        world_from_clip: clip_from_world.inverse(),
        camera: view.world_from_view.translation(),
        forward: view.world_from_view.forward().as_vec3(),
        slope: if slope.is_finite() { slope } else { 1.0 },
        near: projection.w_axis.z,
        viewport: [view.viewport.z, view.viewport.w],
        seconds,
    }
}

/// Allocates a correctly sized effect target from the texture cache.
fn cached(
    cache: &mut TextureCache,
    device: &RenderDevice,
    label: &'static str,
    size: [u32; 2],
    mips: u32,
    format: TextureFormat,
    usage: TextureUsages,
) -> CachedTexture {
    cache.get(
        device,
        TextureDescriptor {
            label: Some(label),
            size: Extent3d {
                width: size[0].max(1),
                height: size[1].max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: mips,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        },
    )
}

/// Prepares uniforms and targets only for opted-in cameras.
#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare_enhanced_views(
    mut state: ResMut<EnhancedViews>,
    gpu: Res<EnhancedGpu>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    pipeline_cache: Res<PipelineCache>,
    mut texture_cache: ResMut<TextureCache>,
    atmosphere: Option<Res<AtmosphereFrame>>,
    time: Res<Time>,
    views: Query<(
        Entity,
        &ExtractedView,
        &ViewTarget,
        &ViewDepthTexture,
        &EnhancedRendering,
    )>,
) {
    state.0.retain(|entity, _| views.contains(*entity));
    let atmosphere = atmosphere.map(|frame| *frame).unwrap_or_default();
    let seconds = time.elapsed_secs_wrapped();
    let view_layout = pipeline_cache.get_bind_group_layout(&enhanced_view_layout());
    let caster_layout = pipeline_cache.get_bind_group_layout(&enhanced_caster_layout());
    for (entity, view, target, depth, settings) in &views {
        let inputs = view_inputs(view, seconds);
        let (frame, fits) = build_frame(&inputs, settings, &atmosphere);
        let state = state
            .0
            .entry(entity)
            .or_insert_with(|| EnhancedViewGpu::new(&device, *settings));
        if state
            .resolved_depth
            .as_ref()
            .is_none_or(|resolved| !resolved.matches(depth))
        {
            state.resolved_depth = Some(ResolvedDepth::new(
                &device,
                depth,
                crate::RuntimeStage::GpuPost,
            ));
        }
        if state
            .hand_layer
            .as_ref()
            .is_none_or(|layer| !layer.matches(target))
        {
            state.hand_layer = Some(super::hand_layer::HandLayer::new(&device, target));
        }
        state.settings = *settings;
        state.cascades = fits.iter().map(|fit| fit.bounds).collect();
        queue.write_buffer(&state.frame, 0, bytemuck::bytes_of(&frame));
        let casters = fits
            .iter()
            .map(|fit| CasterUniformGpu {
                clip_from_world: fit.clip_from_world,
                params: Vec4::new(seconds, frame.ambient_colour.w, 0.0, 0.0),
                flags: UVec4::new(frame.flags.x, 0, 0, 0),
                padding: [Vec4::ZERO; 10],
            })
            .collect::<Vec<_>>();
        queue.write_buffer(&state.casters, 0, bytemuck::cast_slice(&casters));

        let resolution = frame.flags.z;
        let cascades = frame.flags.y;
        if frame.flags.x & super::frame::FEATURE_SHADOWS == 0 {
            state.shadow = None;
        } else if state.shadow.as_ref().is_none_or(|shadow| {
            shadow.resolution != resolution || shadow.layers.len() as u32 != cascades
        }) {
            state.shadow = Some(ShadowTargets::new(&device, resolution, cascades));
        }

        let size = inputs.viewport;
        let half = size.map(|value| value.div_ceil(2).max(1));
        let sampled = TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING;
        state.shafts = (frame.flags.x & super::frame::FEATURE_SHAFTS != 0).then(|| {
            cached(
                &mut texture_cache,
                &device,
                "enhanced light shafts",
                half,
                1,
                POST_FORMAT,
                sampled,
            )
        });
        let scene_size = [
            target.main_texture().width(),
            target.main_texture().height(),
        ];
        let snapshot = TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING;
        state.scene_colour = settings.water_reflections.then(|| {
            cached(
                &mut texture_cache,
                &device,
                "enhanced opaque colour snapshot",
                scene_size,
                1,
                target.main_texture_format(),
                snapshot | TextureUsages::RENDER_ATTACHMENT,
            )
        });
        state.scene_depth = settings.water_reflections.then(|| {
            cached(
                &mut texture_cache,
                &device,
                "enhanced opaque depth snapshot",
                scene_size,
                1,
                SHADOW_FORMAT,
                snapshot,
            )
        });

        let shadow_view = state
            .shadow
            .as_ref()
            .map_or(&gpu.fallback_shadow, |shadow| &shadow.array);
        let scene_colour = state
            .scene_colour
            .as_ref()
            .map_or(&gpu.fallback_colour, |texture| &texture.default_view);
        let scene_depth = state
            .scene_depth
            .as_ref()
            .map_or(&gpu.fallback_depth, |texture| &texture.default_view);
        state.view_bind_group = Some(device.create_bind_group(
            "enhanced view bind group",
            &view_layout,
            &[
                BindGroupEntry {
                    binding: 0,
                    resource: state.frame.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(shadow_view),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::Sampler(&gpu.shadow_sampler),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: BindingResource::TextureView(&gpu.materials),
                },
                BindGroupEntry {
                    binding: 4,
                    resource: BindingResource::TextureView(scene_colour),
                },
                BindGroupEntry {
                    binding: 5,
                    resource: BindingResource::TextureView(scene_depth),
                },
                BindGroupEntry {
                    binding: 6,
                    resource: BindingResource::Sampler(&gpu.linear_sampler),
                },
            ],
        ));
        state.caster_bind_group = Some(device.create_bind_group(
            "enhanced caster bind group",
            &caster_layout,
            &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::Buffer(BufferBinding {
                        buffer: &state.casters,
                        offset: 0,
                        size: NonZeroU64::new(CASTER_UNIFORM_BYTES),
                    }),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(&gpu.materials),
                },
            ],
        ));
    }
}

/// Binds group `I` on Enhanced views; vanilla views are left untouched.
pub(crate) struct SetEnhancedViewBindGroup<const I: usize>;

impl<P: PhaseItem, const I: usize> RenderCommand<P> for SetEnhancedViewBindGroup<I> {
    type Param = Option<SRes<EnhancedViews>>;
    type ViewQuery = (Entity, Has<EnhancedRendering>);
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        (view, enhanced): ROQueryItem<'w, '_, Self::ViewQuery>,
        _entity: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        views: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        if !super::enhanced_rendering_enabled() || !enhanced {
            return RenderCommandResult::Success;
        }
        let Some(views) = views else {
            return RenderCommandResult::Skip;
        };
        let Some(bind_group) = views
            .into_inner()
            .0
            .get(&view)
            .and_then(|state| state.view_bind_group.as_ref())
        else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(I, bind_group, &[]);
        RenderCommandResult::Success
    }
}
