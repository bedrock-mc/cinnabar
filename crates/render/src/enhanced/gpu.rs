//! Render-world Enhanced resources: bind group layouts, fallbacks, material
//! classes, and per-view uniforms, targets and bind groups.

use std::{collections::HashMap, num::NonZeroU64};

use bevy::{
    ecs::{
        query::ROQueryItem,
        system::{SystemParam, SystemParamItem, lifetimeless::SRes},
    },
    math::{Mat4, UVec4, Vec4},
    prelude::*,
    render::{
        render_phase::{PhaseItem, RenderCommand, RenderCommandResult, TrackedRenderPass},
        render_resource::{
            AddressMode, BindGroup, BindGroupEntry, BindGroupLayoutDescriptor,
            BindGroupLayoutEntry, BindingResource, BindingType, Buffer, BufferBinding,
            BufferBindingType, BufferDescriptor, BufferId, BufferUsages, CompareFunction, Extent3d,
            FilterMode, Origin3d, PipelineCache, Sampler, SamplerBindingType, SamplerDescriptor,
            ShaderStages, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture, TextureDescriptor,
            TextureDimension, TextureFormat, TextureSampleType, TextureUsages, TextureView,
            TextureViewDescriptor, TextureViewDimension, TextureViewId,
        },
        renderer::{RenderDevice, RenderQueue},
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
const CASTER_UNIFORM_BYTES: u64 = 192;
pub(crate) const SHADOW_FORMAT: TextureFormat = TextureFormat::Depth32Float;
pub(crate) const POST_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

/// Mirrors `CasterUniform` in enhanced/caster.wgsl, padded to one dynamic slot.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CasterUniformGpu {
    clip_from_world: Mat4,
    params: Vec4,
    flags: UVec4,
    previous_clip_from_world: Mat4,
    previous_params: Vec4,
    local_light: Vec4,
    padding: [Vec4; 4],
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
            entry(7, fragment, texture(FLOAT, TextureViewDimension::D2Array)),
            entry(8, fragment, texture(FLOAT, TextureViewDimension::D2)),
            entry(9, fragment, texture(FLOAT, TextureViewDimension::D2)),
            entry(16, fragment, texture(FLOAT, TextureViewDimension::D2)),
            entry(17, fragment, texture(FLOAT, TextureViewDimension::D2)),
            entry(18, fragment, uniform(FRAME_BYTES, false)),
            entry(19, fragment, texture(FLOAT, TextureViewDimension::D2)),
            entry(
                20,
                fragment,
                texture(TextureSampleType::Depth, TextureViewDimension::D2Array),
            ),
            entry(21, fragment, texture(FLOAT, TextureViewDimension::D2)),
            entry(22, fragment, texture(FLOAT, TextureViewDimension::D3)),
            entry(
                23,
                fragment,
                BindingType::Sampler(SamplerBindingType::Filtering),
            ),
            entry(
                10,
                fragment,
                BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
            ),
            entry(
                11,
                fragment,
                BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(
                        super::local_lights::LOCAL_LIGHT_TILE_MIN_BYTES as u64,
                    ),
                },
            ),
            entry(
                12,
                fragment,
                BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(super::indirect::INDIRECT_MIN_BYTES),
                },
            ),
        ],
    )
}

/// Group 2 of the depth-only shadow-caster pipelines.
pub(crate) fn enhanced_caster_layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "enhanced shadow caster bind group layout",
        &[
            entry(
                0,
                ShaderStages::VERTEX_FRAGMENT,
                uniform(CASTER_UNIFORM_BYTES, true),
            ),
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
            entry(8, fragment, texture(FLOAT, TextureViewDimension::D2)),
            entry(9, fragment, texture(FLOAT, TextureViewDimension::D2)),
            entry(10, fragment, texture(FLOAT, TextureViewDimension::D2)),
            entry(
                12,
                ShaderStages::FRAGMENT,
                texture(FLOAT, TextureViewDimension::D2),
            ),
            entry(
                11,
                fragment,
                BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(16),
                },
            ),
            entry(13, fragment, texture(FLOAT, TextureViewDimension::D3)),
            entry(16, fragment, texture(FLOAT, TextureViewDimension::D2)),
            entry(22, fragment, texture(FLOAT, TextureViewDimension::D3)),
            entry(
                23,
                fragment,
                BindingType::Sampler(SamplerBindingType::Filtering),
            ),
            entry(
                15,
                fragment,
                texture(TextureSampleType::Depth, TextureViewDimension::D2),
            ),
            entry(
                14,
                fragment,
                BindingType::Sampler(SamplerBindingType::Filtering),
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

/// Depth array with a shared resolution for one shadow projection family.
pub(crate) struct ShadowTargets {
    _texture: Texture,
    pub(crate) array: TextureView,
    pub(crate) layers: Vec<TextureView>,
    resolution: u32,
}

impl ShadowTargets {
    /// Creates one independently rendered depth layer per projection.
    fn new(device: &RenderDevice, resolution: u32, cascades: u32) -> Self {
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("enhanced shadow map"),
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
                    label: Some("enhanced shadow projection view"),
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
    pub(crate) local_lights: super::local_lights::LocalLightView,
    pub(crate) indirect: super::indirect::IndirectGridGpu,
    pub(crate) indirect_bind_group: Option<BindGroup>,
    actor_shadow_signature: u64,
    point_shadow_seconds: f32,
    pub(crate) frame: Buffer,
    frame_data: EnhancedFrameGpu,
    pub(crate) shadow_submitted: std::sync::Arc<std::sync::atomic::AtomicU64>,
    pub(crate) capture_parent: Option<Entity>,
    pub(crate) capture_lighting_ready: bool,
    pub(crate) capture_shadow_submission:
        Option<(std::sync::Arc<std::sync::atomic::AtomicU64>, u64)>,
    pub(crate) casters: Buffer,
    pub(crate) depth_caster: Buffer,
    pub(crate) camera_clip: Mat4,
    pub(crate) settings: EnhancedRendering,
    pub(crate) cascades: Vec<CascadeBounds>,
    pub(crate) shadow: Option<ShadowTargets>,
    pub(crate) point_shadow: Option<ShadowTargets>,
    pub(crate) scene: Option<super::targets::SceneTargets>,
    pub(crate) resolved_depth: Option<ResolvedDepth>,
    pub(crate) hand_layer: Option<super::hand_layer::HandLayer>,
    pub(crate) shafts: Option<super::targets::EffectTarget>,
    pub(crate) view_bind_group: Option<BindGroup>,
    view_binding_key: Option<[TextureViewId; 12]>,
    view_buffer_key: Option<(BufferId, BufferId, BufferId)>,
    caster_material_key: Option<TextureViewId>,
    pub(crate) caster_bind_group: Option<BindGroup>,
    pub(crate) depth_bind_group: Option<BindGroup>,
    pub(crate) post: Option<super::targets::PostTargets>,
    pub(crate) history: super::temporal::HistoryState,
    pub(crate) local_shadow: Option<super::local_shadow_history::LocalShadowHistory>,
    pub(crate) exposure: super::exposure::ExposureBuffers,
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
            local_lights: super::local_lights::LocalLightView::new(device),
            indirect: if settings.reflection_capture {
                super::indirect::IndirectGridGpu::for_capture(device)
            } else {
                super::indirect::IndirectGridGpu::new(device)
            },
            indirect_bind_group: None,
            actor_shadow_signature: 0,
            point_shadow_seconds: f32::NEG_INFINITY,
            frame: buffer("enhanced frame uniform", FRAME_BYTES),
            frame_data: bytemuck::Zeroable::zeroed(),
            shadow_submitted: default(),
            capture_parent: None,
            capture_lighting_ready: false,
            capture_shadow_submission: None,
            casters: buffer(
                "enhanced caster uniforms",
                CASTER_SLOT_BYTES
                    * (u64::from(super::MAX_SHADOW_CASCADES)
                        + (super::local_lights::MAX_SHADOWED_LIGHTS
                            * super::local_lights::POINT_SHADOW_FACES)
                            as u64),
            ),
            depth_caster: buffer("enhanced camera depth uniform", CASTER_SLOT_BYTES),
            camera_clip: Mat4::IDENTITY,
            settings,
            cascades: Vec::new(),
            shadow: None,
            point_shadow: None,
            scene: None,
            resolved_depth: None,
            shafts: None,
            hand_layer: None,
            view_bind_group: None,
            view_binding_key: None,
            view_buffer_key: None,
            caster_material_key: None,
            caster_bind_group: None,
            depth_bind_group: None,
            post: None,
            history: default(),
            local_shadow: None,
            exposure: super::exposure::ExposureBuffers::new(device),
        }
    }
}

#[derive(Resource, Default)]
pub(crate) struct EnhancedViews(pub(crate) HashMap<Entity, EnhancedViewGpu>);

#[path = "view_prepare.rs"]
mod view_prepare;
pub(crate) use view_prepare::prepare_enhanced_views;

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
        if !render_model::enhanced_rendering_enabled() || !enhanced {
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

#[cfg(test)]
mod capability_tests {
    use super::*;

    #[test]
    fn combined_enhanced_layout_rejects_a_vanilla_capable_adapter() {
        let mut limits = wgpu::Limits::default();
        assert!(crate::material_shader::chunk_atlas_views_fit(&limits));
        assert!(!super::super::EnhancedRenderSupport::for_device_limits(&limits).0);
        let textures = crate::material_shader::CHUNK_SAMPLED_TEXTURE_BINDINGS
            + enhanced_view_layout()
                .entries
                .iter()
                .filter(|entry| matches!(entry.ty, BindingType::Texture { .. }))
                .count() as u32;
        limits.max_sampled_textures_per_shader_stage = textures;
        assert!(super::super::EnhancedRenderSupport::for_device_limits(&limits).0);
        limits.max_sampled_textures_per_shader_stage -= 1;
        assert!(!super::super::EnhancedRenderSupport::for_device_limits(&limits).0);
        limits.max_sampled_textures_per_shader_stage = textures;
        limits.max_samplers_per_shader_stage = crate::material_shader::CHUNK_SAMPLER_COUNT
            + enhanced_view_layout()
                .entries
                .iter()
                .filter(|entry| matches!(entry.ty, BindingType::Sampler(_)))
                .count() as u32;
        assert!(super::super::EnhancedRenderSupport::for_device_limits(&limits).0);
        limits.max_samplers_per_shader_stage -= 1;
        assert!(!super::super::EnhancedRenderSupport::for_device_limits(&limits).0);
    }
}
