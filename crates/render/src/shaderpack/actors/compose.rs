//! The layer-composition kernel (`compose.wesl`): artwork whose native material composes
//! texture layers in the fragment (colour masks, multitexture) becomes one opaque layer the
//! pack samples as `gtexture`.

use std::num::NonZeroU64;

use bevy::asset::uuid_handle;
use bevy::prelude::{Handle, Shader};
use bevy::render::render_resource::{
    BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType, BufferBindingType, ShaderStages,
    StorageTextureAccess, TextureFormat, TextureSampleType, TextureViewDimension,
};
use bytemuck::{Pod, Zeroable};
use render_model::ActorGpuInstance;
use wgpu::util::DeviceExt;
use wgpu::{CommandEncoder, Device, TextureView};

use super::skin::SkinKernels;

pub(super) const COMPOSE_SHADER: Handle<Shader> =
    uuid_handle!("3f6e2b9a-7c41-4d08-b5e3-1a9c0d7f4e62");

/// Side of the kernel's square workgroup, substituted into its source.
const COMPOSE_WORKGROUP_SIDE: u32 = 8;
/// The kernel's mode for [`LayerCompose::Multitexture`]; any other mode is a colour mask.
const COMPOSE_MULTITEXTURE: u32 = 1;
/// The composed layer's format: raw gamma bytes, as the pack reads every actor texture.
const COMPOSED_FORMAT: TextureFormat = TextureFormat::Rgba8Unorm;

/// How a drawn layer is derived from its artwork before the pack samples it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum LayerCompose {
    /// The stored layer, sampled directly.
    Plain,
    /// Native `entity_change_color`: alpha weights the packed RGBA8 dye `tint` (none when
    /// zero) and is not coverage.
    ColorMask { tint: u32 },
    /// Native `entity_multitexture`: two overlay layers of the same texture, each weighted by
    /// its alpha over the base; base alpha is not coverage.
    Multitexture { layers: [u32; 2] },
}

impl LayerCompose {
    /// The composition a page's native material applies to `instance`, matching the actor
    /// pass's material selection.
    pub(crate) fn for_page(
        color_mask: bool,
        multitexture: bool,
        instance: &ActorGpuInstance,
    ) -> Self {
        if color_mask {
            Self::ColorMask {
                tint: instance.tint,
            }
        } else if multitexture && !instance.multitexture_layers.contains(&u32::MAX) {
            Self::Multitexture {
                layers: instance.multitexture_layers,
            }
        } else {
            Self::Plain
        }
    }

    /// Whether the composed layer already carries every colour the material applies, so the
    /// draw's vertex colour stays white.
    pub(crate) fn composes_colour(self) -> bool {
        self != Self::Plain
    }

    fn params(self, layer: u32) -> ComposeParams {
        let (overlays, tint, mode) = match self {
            Self::Multitexture { layers } => (layers, 0, COMPOSE_MULTITEXTURE),
            Self::ColorMask { tint } => ([layer; 2], tint, 0),
            Self::Plain => ([layer; 2], 0, 0),
        };
        ComposeParams {
            layers: [layer, overlays[0], overlays[1], 0],
            tint,
            mode,
            _padding: [0; 2],
        }
    }
}

/// The kernel's `ComposeParams` uniform.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct ComposeParams {
    layers: [u32; 4],
    tint: u32,
    mode: u32,
    _padding: [u32; 2],
}

/// Resolves the kernel's generated constants.
pub(super) fn compose_shader_source(source: &str) -> String {
    source
        .replace(
            "COMPOSE_WORKGROUP_SIDE",
            &format!("{COMPOSE_WORKGROUP_SIDE}u"),
        )
        .replace(
            "COMPOSE_MULTITEXTURE_MODE",
            &format!("{COMPOSE_MULTITEXTURE}u"),
        )
}

pub(super) fn shader(source: &str, path: impl Into<String>) -> Shader {
    crate::shader_safety::from_wesl(compose_shader_source(source), path)
}

/// Group 0 of the kernel: the artwork array, the composed layer and the parameters.
pub(crate) fn layout() -> BindGroupLayoutDescriptor {
    let entry = |binding, ty| BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::COMPUTE,
        ty,
        count: None,
    };
    BindGroupLayoutDescriptor::new(
        "shader-pack actor layer composition",
        &[
            entry(
                0,
                BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: false },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
                },
            ),
            entry(
                1,
                BindingType::StorageTexture {
                    access: StorageTextureAccess::WriteOnly,
                    format: COMPOSED_FORMAT,
                    view_dimension: TextureViewDimension::D2,
                },
            ),
            entry(
                2,
                BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(size_of::<ComposeParams>() as u64),
                },
            ),
        ],
    )
}

/// Records the composition of `texture`'s `layer` into a single-layer texture of the same
/// size, `reuse` when given (same size, from an earlier composition) or a new one, and
/// returns it with its view; `None` when an allocation fails.
pub(super) fn record(
    device: &Device,
    encoder: &mut CommandEncoder,
    kernels: &SkinKernels<'_>,
    texture: &wgpu::Texture,
    layer: u32,
    compose: LayerCompose,
    reuse: Option<wgpu::Texture>,
) -> Option<(wgpu::Texture, TextureView)> {
    let composed = match reuse {
        Some(composed) => composed,
        None => {
            let render_device = bevy::render::renderer::RenderDevice::from(device.clone());
            let size = wgpu::Extent3d {
                depth_or_array_layers: 1,
                ..texture.size()
            };
            let bytes = u64::from(size.width) * u64::from(size.height) * 4;
            let label = "shader-pack composed actor layer";
            crate::gpu_allocation::checked(&render_device, label, bytes, || {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: COMPOSED_FORMAT,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::STORAGE_BINDING,
                    view_formats: &[],
                })
            })?
        }
    };
    let view = composed.create_view(&Default::default());
    let kernel = (&**kernels.compose, &**kernels.compose_layout);
    dispatch(device, encoder, kernel, texture, layer, compose, &view)?;
    Some((composed, view))
}

/// Records one dispatch composing `texture`'s `layer` into `composed`, a [`COMPOSED_FORMAT`]
/// storage view of the same size; `None` when its parameters cannot be allocated.
fn dispatch(
    device: &Device,
    encoder: &mut CommandEncoder,
    (pipeline, layout): (&wgpu::ComputePipeline, &wgpu::BindGroupLayout),
    texture: &wgpu::Texture,
    layer: u32,
    compose: LayerCompose,
    composed: &TextureView,
) -> Option<()> {
    let render_device = bevy::render::renderer::RenderDevice::from(device.clone());
    let params = compose.params(layer);
    let label = "shader-pack actor composition parameters";
    let params = crate::gpu_allocation::checked(
        &render_device,
        label,
        size_of::<ComposeParams>() as u64,
        || {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::bytes_of(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            })
        },
    )?;
    let artwork = texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("shader-pack actor artwork array"),
        format: Some(texture.format().remove_srgb_suffix()),
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("shader-pack actor layer composition"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&artwork),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(composed),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: params.as_entire_binding(),
            },
        ],
    });
    let size = texture.size();
    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: Some("shader-pack actor layer composition"),
        timestamp_writes: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, &group, &[]);
    pass.dispatch_workgroups(
        size.width.div_ceil(COMPOSE_WORKGROUP_SIDE),
        size.height.div_ceil(COMPOSE_WORKGROUP_SIDE),
        1,
    );
    Some(())
}

#[cfg(test)]
#[path = "compose_tests.rs"]
mod tests;
