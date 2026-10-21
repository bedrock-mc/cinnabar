//! Translated programs become raw wgpu pipelines.
//!
//! Bevy 0.20's `PipelineCache` only accepts WGSL, WESL or SPIR-V text and recomposes WGSL
//! through naga_oil, so pack programs bypass it: the validated naga modules from
//! `shaderpack` reach wgpu unchanged as `ShaderSource::Naga` (the `naga-ir` wgpu feature),
//! and layouts come straight from each program's [`ProgramReflection`]. Pipelines are
//! created on the load worker inside a validation error scope, so a pack the device rejects
//! fails its load instead of raising a device error.

use super::plan::{MAX_COLOR_BUFFERS, color_buffer_of};
use shaderpack::{
    BlendFactor as PackFactor, BlendMode, BlendRule, BufferFormat, ProgramGroup, ProgramReflection,
    SampleType, SamplerBinding, SamplerClass, ScalarType, StageKind, StageMask,
    TextureDimension as PackDimension, ValueType, VertexAttribute,
};
use std::borrow::Cow;
use wgpu::{
    BindGroupLayout, BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType, BlendComponent,
    BlendFactor, BlendOperation, BlendState, BufferBindingType, ColorTargetState, ColorWrites,
    Device, Features, SamplerBindingType, ShaderStages, StorageTextureAccess, TextureFormat,
    TextureSampleType, TextureUsages, TextureViewDimension, VertexAttribute as WgpuAttribute,
    VertexBufferLayout, VertexFormat, VertexStepMode,
};

/// Format of the pack's screen, which `final` writes as Minecraft's 8-bit framebuffer.
pub(crate) const SCREEN_FORMAT: TextureFormat = TextureFormat::Rgba8Unorm;

/// The device a pack is built for.
#[derive(Clone)]
pub(crate) struct GpuContext {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub adapter: wgpu::Adapter,
}

impl GpuContext {
    pub(crate) fn support(&self) -> FormatSupport {
        FormatSupport::for_device(self.device.features(), |format| {
            self.adapter.get_texture_format_features(format)
        })
    }

    /// Whether the adapter can use `format` with every usage in `usages`.
    pub(crate) fn supports(&self, format: TextureFormat, usages: TextureUsages) -> bool {
        effective_format_features(self.device.features(), format, |format| {
            self.adapter.get_texture_format_features(format)
        })
        .allowed_usages
        .contains(usages)
    }

    /// Whether the device has image atomics on `format` (`TEXTURE_ATOMIC`, `r32ui`/`r32i`).
    pub(crate) fn supports_atomics(&self, format: TextureFormat) -> bool {
        self.device
            .features()
            .contains(wgpu::Features::TEXTURE_ATOMIC)
            && self.supports(format, TextureUsages::STORAGE_ATOMIC)
    }
}

/// Uses adapter-specific usages only when the device enabled them.
fn effective_format_features(
    features: Features,
    format: TextureFormat,
    adapter: impl Fn(TextureFormat) -> wgpu::TextureFormatFeatures,
) -> wgpu::TextureFormatFeatures {
    let mut support = if features.contains(Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES) {
        adapter(format)
    } else {
        format.guaranteed_format_features(features)
    };
    if !features.contains(format.required_features()) {
        support.allowed_usages = TextureUsages::empty();
    }
    support
}

/// Device features that decide which pack buffer formats need a fallback.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct FormatSupport {
    /// `FLOAT32_FILTERABLE`: 32-bit float textures can use filtering samplers.
    pub float32_filterable: bool,
    /// `RG11B10UFLOAT_RENDERABLE`.
    pub rg11b10_renderable: bool,
    /// Exact 16-bit unorm buffers support render attachments; otherwise they use 32-bit
    /// floating-point formats.
    pub norm16: bool,
}

impl FormatSupport {
    /// Uses only the format usages guaranteed by the enabled device features.
    #[cfg(test)]
    pub(crate) fn from_features(features: Features) -> Self {
        Self::for_device(features, |format| {
            format.guaranteed_format_features(features)
        })
    }

    /// Enables exact norm16 targets only when every selected norm16 format can be attached.
    fn for_device(
        features: Features,
        adapter: impl Fn(TextureFormat) -> wgpu::TextureFormatFeatures,
    ) -> Self {
        let mut support = Self {
            float32_filterable: features.contains(Features::FLOAT32_FILTERABLE),
            rg11b10_renderable: features.contains(Features::RG11B10UFLOAT_RENDERABLE),
            norm16: features.contains(Features::TEXTURE_FORMAT_16BIT_NORM),
        };
        support.norm16 = support.norm16
            && BufferFormat::ALL
                .iter()
                .map(|format| texture_format(*format, support))
                .filter(|format| {
                    format
                        .required_features()
                        .contains(Features::TEXTURE_FORMAT_16BIT_NORM)
                })
                .all(|format| {
                    effective_format_features(features, format, &adapter)
                        .allowed_usages
                        .contains(TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING)
                });
        support
    }

    fn float32(self, format: TextureFormat, half: TextureFormat) -> TextureFormat {
        if self.float32_filterable {
            format
        } else {
            half
        }
    }
}

/// The renderable, sampleable wgpu format holding a pack buffer format. RGB formats gain
/// an alpha channel; formats wgpu cannot render (snorm, shared exponent, packed 16-bit)
/// widen to a float format with at least their range and precision. Declared 32-bit float
/// formats retain their storage-image format even when sampling cannot filter them.
pub(crate) fn texture_format(format: BufferFormat, support: FormatSupport) -> TextureFormat {
    use BufferFormat as B;
    use TextureFormat as T;
    let unorm16 = |exact: T, float: T, half: T| {
        if support.norm16 {
            exact
        } else {
            support.float32(float, half)
        }
    };
    match format {
        B::Rgba | B::Rgb8 | B::Rgba8 | B::Rgba2 | B::Rgba4 | B::R3G3B2 | B::Rgb5A1 | B::Rgb565 => {
            T::Rgba8Unorm
        }
        B::R8 => T::R8Unorm,
        B::Rg8 => T::Rg8Unorm,
        B::R8Snorm | B::R16f => T::R16Float,
        B::Rg8Snorm | B::Rg16f => T::Rg16Float,
        B::Rgb8Snorm | B::Rgba8Snorm | B::Rgb16f | B::Rgba16f | B::Rgb9E5 => T::Rgba16Float,
        B::R16 => unorm16(T::R16Unorm, T::R32Float, T::R16Float),
        B::Rg16 => unorm16(T::Rg16Unorm, T::Rg32Float, T::Rg16Float),
        B::Rgb16 | B::Rgba16 => unorm16(T::Rgba16Unorm, T::Rgba32Float, T::Rgba16Float),
        B::R16Snorm => support.float32(T::R32Float, T::R16Float),
        B::Rg16Snorm => support.float32(T::Rg32Float, T::Rg16Float),
        B::Rgb16Snorm | B::Rgba16Snorm => support.float32(T::Rgba32Float, T::Rgba16Float),
        B::R32f => T::R32Float,
        B::Rg32f => T::Rg32Float,
        B::Rgb32f | B::Rgba32f => T::Rgba32Float,
        B::R8i => T::R8Sint,
        B::Rg8i => T::Rg8Sint,
        B::Rgb8i | B::Rgba8i => T::Rgba8Sint,
        B::R8ui => T::R8Uint,
        B::Rg8ui => T::Rg8Uint,
        B::Rgb8ui | B::Rgba8ui => T::Rgba8Uint,
        B::R16i => T::R16Sint,
        B::Rg16i => T::Rg16Sint,
        B::Rgb16i | B::Rgba16i => T::Rgba16Sint,
        B::R16ui => T::R16Uint,
        B::Rg16ui => T::Rg16Uint,
        B::Rgb16ui | B::Rgba16ui => T::Rgba16Uint,
        B::R32i => T::R32Sint,
        B::Rg32i => T::Rg32Sint,
        B::Rgb32i | B::Rgba32i => T::Rgba32Sint,
        B::R32ui => T::R32Uint,
        B::Rg32ui => T::Rg32Uint,
        B::Rgb32ui | B::Rgba32ui => T::Rgba32Uint,
        B::Rgb10A2 => T::Rgb10a2Unorm,
        B::Rgb10A2ui => T::Rgb10a2Uint,
        B::R11fG11fB10f if support.rg11b10_renderable => T::Rg11b10Ufloat,
        B::R11fG11fB10f => T::Rgba16Float,
    }
}

/// Component kind a shader reads from or writes to a texture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Float,
    Sint,
    Uint,
}

/// The kind of a colour format.
pub(crate) fn format_kind(format: TextureFormat) -> Kind {
    match format.sample_type(None, None) {
        Some(TextureSampleType::Sint) => Kind::Sint,
        Some(TextureSampleType::Uint) => Kind::Uint,
        _ => Kind::Float,
    }
}

/// The kind of a reflected scalar type.
pub(crate) fn scalar_kind(ty: ValueType) -> Kind {
    match scalar_type(ty) {
        ScalarType::Int8
        | ScalarType::Int16
        | ScalarType::Int
        | ScalarType::Int64
        | ScalarType::Bool => Kind::Sint,
        ScalarType::Uint8 | ScalarType::Uint16 | ScalarType::Uint | ScalarType::Uint64 => {
            Kind::Uint
        }
        ScalarType::Half | ScalarType::Float | ScalarType::Double => Kind::Float,
    }
}

/// Returns the component type shared by a scalar, vector or matrix.
fn scalar_type(ty: ValueType) -> ScalarType {
    match ty {
        ValueType::Scalar(s) | ValueType::Vector { scalar: s, .. } => s,
        ValueType::Matrix { scalar, .. } => scalar,
    }
}

/// The kind a sampled texture declaration expects.
pub(crate) fn sample_kind(sample: SampleType) -> Option<Kind> {
    match sample {
        SampleType::Float => Some(Kind::Float),
        SampleType::Sint => Some(Kind::Sint),
        SampleType::Uint => Some(Kind::Uint),
        SampleType::Depth => None,
    }
}

/// Rejects texture shapes that wgpu cannot bind.
fn view_dimension(dimension: PackDimension) -> Result<TextureViewDimension, String> {
    Ok(match dimension {
        PackDimension::D1Array => return Err("1D array textures are unsupported by wgpu".into()),
        PackDimension::D1 => TextureViewDimension::D1,
        PackDimension::D2 => TextureViewDimension::D2,
        PackDimension::D2Array => TextureViewDimension::D2Array,
        PackDimension::D3 => TextureViewDimension::D3,
        PackDimension::Cube => TextureViewDimension::Cube,
        PackDimension::CubeArray => TextureViewDimension::CubeArray,
    })
}

fn stages(mask: StageMask) -> ShaderStages {
    let mut stages = ShaderStages::NONE;
    if mask.vertex {
        stages |= ShaderStages::VERTEX;
    }
    if mask.fragment {
        stages |= ShaderStages::FRAGMENT;
    }
    if mask.compute {
        stages |= ShaderStages::COMPUTE;
    }
    stages
}

/// The wgpu format of a GLSL image format qualifier.
pub(crate) fn storage_format(format: naga::StorageFormat) -> Option<TextureFormat> {
    use TextureFormat as T;
    use naga::StorageFormat as S;
    Some(match format {
        S::R8Unorm => T::R8Unorm,
        S::R8Snorm => T::R8Snorm,
        S::R8Uint => T::R8Uint,
        S::R8Sint => T::R8Sint,
        S::R16Uint => T::R16Uint,
        S::R16Sint => T::R16Sint,
        S::R16Float => T::R16Float,
        S::Rg8Unorm => T::Rg8Unorm,
        S::Rg8Snorm => T::Rg8Snorm,
        S::Rg8Uint => T::Rg8Uint,
        S::Rg8Sint => T::Rg8Sint,
        S::R32Uint => T::R32Uint,
        S::R32Sint => T::R32Sint,
        S::R32Float => T::R32Float,
        S::Rg16Uint => T::Rg16Uint,
        S::Rg16Sint => T::Rg16Sint,
        S::Rg16Float => T::Rg16Float,
        S::Rgba8Unorm => T::Rgba8Unorm,
        S::Rgba8Snorm => T::Rgba8Snorm,
        S::Rgba8Uint => T::Rgba8Uint,
        S::Rgba8Sint => T::Rgba8Sint,
        S::Bgra8Unorm => T::Bgra8Unorm,
        S::Rgb10a2Uint => T::Rgb10a2Uint,
        S::Rgb10a2Unorm => T::Rgb10a2Unorm,
        S::Rg11b10Ufloat => T::Rg11b10Ufloat,
        S::R64Uint => T::R64Uint,
        S::Rg32Uint => T::Rg32Uint,
        S::Rg32Sint => T::Rg32Sint,
        S::Rg32Float => T::Rg32Float,
        S::Rgba16Uint => T::Rgba16Uint,
        S::Rgba16Sint => T::Rgba16Sint,
        S::Rgba16Float => T::Rgba16Float,
        S::Rgba32Uint => T::Rgba32Uint,
        S::Rgba32Sint => T::Rgba32Sint,
        S::Rgba32Float => T::Rgba32Float,
        S::R16Unorm => T::R16Unorm,
        S::R16Snorm => T::R16Snorm,
        S::Rg16Unorm => T::Rg16Unorm,
        S::Rg16Snorm => T::Rg16Snorm,
        S::Rgba16Unorm => T::Rgba16Unorm,
        S::Rgba16Snorm => T::Rgba16Snorm,
    })
}

/// Sample type of a bound texture: its own format and the device's features decide float
/// filterability (32-bit floats filter with `FLOAT32_FILTERABLE`, which is why the host
/// keeps them).
pub(crate) fn texture_sample_type(
    format: TextureFormat,
    support: FormatSupport,
) -> TextureSampleType {
    let features = if support.float32_filterable {
        Features::FLOAT32_FILTERABLE
    } else {
        Features::empty()
    };
    match format {
        TextureFormat::Depth32Float | TextureFormat::Depth24Plus => TextureSampleType::Depth,
        f => f
            .sample_type(None, Some(features))
            .unwrap_or(TextureSampleType::Float { filterable: true }),
    }
}

/// Binding type of a reflected sampler: depth comparison, or a filtering class. The
/// classes without texel or mip filtering also read integer and unfilterable textures.
pub(crate) fn sampler_binding_type(sampler: &SamplerBinding) -> SamplerBindingType {
    match sampler.class() {
        None => SamplerBindingType::Comparison,
        Some(class) if !class.linear() && !class.mipmap_linear() => {
            SamplerBindingType::NonFiltering
        }
        Some(_) => SamplerBindingType::Filtering,
    }
}

/// The native sampler settings of a class, including the atlas's linear mip filtering.
pub(crate) fn class_sampler(class: SamplerClass) -> wgpu::SamplerDescriptor<'static> {
    let address = if class.clamps() {
        wgpu::AddressMode::ClampToEdge
    } else {
        wgpu::AddressMode::Repeat
    };
    let filter = if class.linear() {
        wgpu::FilterMode::Linear
    } else {
        wgpu::FilterMode::Nearest
    };
    let mipmap = if class.mipmap_linear() {
        wgpu::MipmapFilterMode::Linear
    } else {
        wgpu::MipmapFilterMode::Nearest
    };
    wgpu::SamplerDescriptor {
        label: Some(class.glsl_name()),
        address_mode_u: address,
        address_mode_v: address,
        address_mode_w: address,
        mag_filter: filter,
        min_filter: filter,
        mipmap_filter: mipmap,
        ..Default::default()
    }
}

/// The bind-group layout entries of one program, per group (0..=3), with each sampled
/// texture typed as the host will bind it (`sample_types` is parallel to
/// `reflection.textures`; see [`super::resources::TextureSource::sample_type`]).
pub(crate) fn layout_entries(
    reflection: &ProgramReflection,
    sample_types: &[TextureSampleType],
) -> Result<[Vec<BindGroupLayoutEntry>; 4], String> {
    if let Some(block) = &reflection.uniforms {
        for member in &block.members {
            if !matches!(
                scalar_type(member.ty),
                ScalarType::Bool | ScalarType::Int | ScalarType::Uint | ScalarType::Float
            ) {
                return Err(format!(
                    "uniform {} has an unsupported component width",
                    member.name
                ));
            }
        }
    }
    let mut groups: [Vec<BindGroupLayoutEntry>; 4] = Default::default();
    let mut push = |group: u32, entry: BindGroupLayoutEntry| {
        if let Some(entries) = groups.get_mut(group as usize) {
            entries.push(entry);
        }
    };
    if let Some(block) = &reflection.uniforms {
        push(
            block.group,
            BindGroupLayoutEntry {
                binding: block.binding,
                visibility: stages(block.stages),
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: std::num::NonZeroU64::new(u64::from(block.size)),
                },
                count: None,
            },
        );
    }
    for (buffer, ty) in reflection
        .uniform_buffers
        .iter()
        .map(|b| (b, BufferBindingType::Uniform))
        .chain(reflection.storage_buffers.iter().map(|b| {
            (
                b,
                BufferBindingType::Storage {
                    read_only: b.read_only,
                },
            )
        }))
    {
        push(
            buffer.group,
            BindGroupLayoutEntry {
                binding: buffer.binding,
                visibility: stages(buffer.stages),
                ty: BindingType::Buffer {
                    ty,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        );
    }
    for sampler in &reflection.samplers {
        push(
            sampler.group,
            BindGroupLayoutEntry {
                binding: sampler.binding,
                visibility: stages(sampler.stages),
                ty: BindingType::Sampler(sampler_binding_type(sampler)),
                count: None,
            },
        );
    }
    for (texture, sample_type) in reflection.textures.iter().zip(sample_types) {
        push(
            texture.group,
            BindGroupLayoutEntry {
                binding: texture.binding,
                visibility: stages(texture.stages),
                ty: BindingType::Texture {
                    sample_type: *sample_type,
                    view_dimension: view_dimension(texture.dimension)?,
                    multisampled: false,
                },
                count: None,
            },
        );
    }
    for image in &reflection.storage_images {
        let Some(format) = storage_format(image.format) else {
            continue;
        };
        push(
            image.group,
            BindGroupLayoutEntry {
                binding: image.binding,
                visibility: stages(image.stages),
                ty: BindingType::StorageTexture {
                    access: match (image.atomic, image.read, image.write) {
                        (true, ..) => StorageTextureAccess::Atomic,
                        (_, true, true) => StorageTextureAccess::ReadWrite,
                        (_, true, false) => StorageTextureAccess::ReadOnly,
                        _ => StorageTextureAccess::WriteOnly,
                    },
                    format,
                    view_dimension: view_dimension(image.dimension)?,
                },
                count: None,
            },
        );
    }
    for entries in &mut groups {
        entries.sort_by_key(|e| e.binding);
    }
    Ok(groups)
}

/// Creates the four bind-group layouts of a program; empty groups have no layout.
pub(crate) fn bind_group_layouts(
    device: &Device,
    name: &str,
    entries: &[Vec<BindGroupLayoutEntry>; 4],
) -> [Option<BindGroupLayout>; 4] {
    std::array::from_fn(|group| {
        (!entries[group].is_empty()).then(|| {
            device.create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some(&format!("shader pack {name} group {group}")),
                entries: &entries[group],
            })
        })
    })
}

/// Bytes per supplied attribute in the shared full-screen quad buffer.
pub(crate) const QUAD_ATTRIBUTE_BYTES: u64 = 16;
/// Supplied attributes in the quad buffer, in [`VertexAttribute::location`] order.
pub(crate) const QUAD_ATTRIBUTES: u64 = shaderpack::FIRST_OTHER_ATTRIBUTE_LOCATION as u64;
/// The quad buffer's slot after the supplied attributes: zero, which the raw position parts
/// (`iris_PackedPosition`, `iris_DrawOrigin`) read so positions come from the float input.
const QUAD_ZERO_SLOT: u64 = QUAD_ATTRIBUTES;
/// Slots per quad vertex.
const QUAD_SLOTS: u64 = QUAD_ATTRIBUTES + 1;

/// The quad vertices (triangle strip) with every supplied attribute, 16 bytes each:
/// position (0..1 square), white colour, +Z normal, texture coordinate equal to position;
/// every other attribute is zero, which reads as 0 whether declared float or integer.
pub(crate) fn quad_vertices() -> Vec<u8> {
    let corners = [[0.0f32, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
    let mut bytes =
        Vec::with_capacity(corners.len() * (QUAD_SLOTS * QUAD_ATTRIBUTE_BYTES) as usize);
    for [x, y] in corners {
        for location in 0..QUAD_SLOTS {
            let value: [f32; 4] = match location {
                l if Some(l as u32) == VertexAttribute::Position.location() => [x, y, 0.0, 1.0],
                l if Some(l as u32) == VertexAttribute::Color.location() => [1.0; 4],
                l if Some(l as u32) == VertexAttribute::Normal.location() => [0.0, 0.0, 1.0, 0.0],
                l if Some(l as u32) == VertexAttribute::TexCoord.location() => [x, y, 0.0, 1.0],
                _ => [0.0; 4],
            };
            for v in value {
                bytes.extend_from_slice(&v.to_le_bytes());
            }
        }
    }
    bytes
}

/// The quad-buffer attributes a program's vertex stage declares. Inputs Iris does not
/// supply (`Other`) are left unbound; wgpu then rejects the pipeline, failing the load.
pub(crate) fn quad_attributes(reflection: &ProgramReflection) -> Vec<WgpuAttribute> {
    reflection
        .vertex_inputs
        .iter()
        .filter_map(|input| {
            let slot = match input.attribute {
                VertexAttribute::PackedPosition | VertexAttribute::DrawOrigin => QUAD_ZERO_SLOT,
                ref supplied => u64::from(supplied.location()?),
            };
            let components = match input.ty {
                ValueType::Scalar(_) => 1,
                ValueType::Vector { size, .. } => size,
                ValueType::Matrix { .. } => return None,
            };
            let format = match (scalar_kind(input.ty), components) {
                (Kind::Float, 1) => VertexFormat::Float32,
                (Kind::Float, 2) => VertexFormat::Float32x2,
                (Kind::Float, 3) => VertexFormat::Float32x3,
                (Kind::Float, _) => VertexFormat::Float32x4,
                (Kind::Sint, 1) => VertexFormat::Sint32,
                (Kind::Sint, 2) => VertexFormat::Sint32x2,
                (Kind::Sint, 3) => VertexFormat::Sint32x3,
                (Kind::Sint, _) => VertexFormat::Sint32x4,
                (Kind::Uint, 1) => VertexFormat::Uint32,
                (Kind::Uint, 2) => VertexFormat::Uint32x2,
                (Kind::Uint, 3) => VertexFormat::Uint32x3,
                (Kind::Uint, _) => VertexFormat::Uint32x4,
            };
            Some(WgpuAttribute {
                format,
                offset: slot * QUAD_ATTRIBUTE_BYTES,
                shader_location: input.location,
            })
        })
        .collect()
}

pub(crate) fn quad_layout(attributes: &[WgpuAttribute]) -> VertexBufferLayout<'_> {
    VertexBufferLayout {
        array_stride: QUAD_SLOTS * QUAD_ATTRIBUTE_BYTES,
        step_mode: VertexStepMode::Vertex,
        attributes,
    }
}

fn blend_factor(factor: PackFactor) -> BlendFactor {
    match factor {
        PackFactor::Zero => BlendFactor::Zero,
        PackFactor::One => BlendFactor::One,
        PackFactor::SrcColor => BlendFactor::Src,
        PackFactor::OneMinusSrcColor => BlendFactor::OneMinusSrc,
        PackFactor::DstColor => BlendFactor::Dst,
        PackFactor::OneMinusDstColor => BlendFactor::OneMinusDst,
        PackFactor::SrcAlpha => BlendFactor::SrcAlpha,
        PackFactor::OneMinusSrcAlpha => BlendFactor::OneMinusSrcAlpha,
        PackFactor::DstAlpha => BlendFactor::DstAlpha,
        PackFactor::OneMinusDstAlpha => BlendFactor::OneMinusDstAlpha,
        PackFactor::SrcAlphaSaturate => BlendFactor::SrcAlphaSaturated,
    }
}

/// The wgpu blend state of a `blend.*` mode; `None` disables blending.
pub(crate) fn blend_state(mode: BlendMode) -> Option<BlendState> {
    match mode {
        BlendMode::Off => None,
        BlendMode::Factors {
            src_color,
            dst_color,
            src_alpha,
            dst_alpha,
        } => Some(BlendState {
            color: BlendComponent {
                src_factor: blend_factor(src_color),
                dst_factor: blend_factor(dst_color),
                operation: BlendOperation::Add,
            },
            alpha: BlendComponent {
                src_factor: blend_factor(src_alpha),
                dst_factor: blend_factor(dst_alpha),
                operation: BlendOperation::Add,
            },
        }),
    }
}

/// Whether every device can filter (and blend) `format` without optional features.
pub(crate) fn filterable(format: TextureFormat) -> bool {
    matches!(
        format.sample_type(None, None),
        Some(TextureSampleType::Float { filterable: true })
    )
}

/// A colour target for an output of `ty` into `format`, or `None` when the shader's
/// component kind cannot write the format (the output is then discarded). Integer
/// formats never blend.
pub(crate) fn color_target(
    format: TextureFormat,
    ty: ValueType,
    blend: Option<BlendState>,
) -> Option<ColorTargetState> {
    (scalar_kind(ty) == format_kind(format)).then(|| ColorTargetState {
        format,
        blend: blend.filter(|_| format_kind(format) == Kind::Float && filterable(format)),
        write_mask: ColorWrites::ALL,
    })
}

/// Colour targets per fragment output location, applying pack overrides to the draw's
/// baseline blend state. `final` writes the screen at location 0.
pub(crate) fn color_targets(
    group: ProgramGroup,
    name: &str,
    reflection: &ProgramReflection,
    buffer_formats: &[Option<TextureFormat>; MAX_COLOR_BUFFERS as usize],
    blends: &[BlendRule],
    baseline: Option<BlendState>,
) -> Vec<Option<ColorTargetState>> {
    let len = reflection
        .color_outputs
        .iter()
        .map(|o| o.location as usize + 1)
        .max()
        .unwrap_or(0);
    let mut targets = vec![None; len];
    let program_blend = blends
        .iter()
        .rev()
        .find(|b| b.program == name && b.buffer.is_none())
        .map(|b| b.mode);
    for output in &reflection.color_outputs {
        let format = if group == ProgramGroup::Final {
            (output.location == 0).then_some(SCREEN_FORMAT)
        } else {
            output
                .buffer
                .and_then(|b| buffer_formats.get(b as usize).copied().flatten())
        };
        let Some(format) = format else {
            continue;
        };
        let buffer_blend = output.buffer.and_then(|b| {
            blends
                .iter()
                .rev()
                .find(|r| {
                    r.program == name && r.buffer.as_deref().and_then(color_buffer_of) == Some(b)
                })
                .map(|r| r.mode)
        });
        let blend = buffer_blend.or(program_blend).map_or(baseline, blend_state);
        targets[output.location as usize] = color_target(format, output.ty, blend);
    }
    targets
}

/// The target of a program that writes no colour buffer the frame has but stores to images
/// or storage buffers: the screen with every channel masked off. WebGPU needs an attachment
/// to draw where OpenGL does not.
pub(crate) const MASKED_TARGET: [Option<ColorTargetState>; 1] = [Some(ColorTargetState {
    format: SCREEN_FORMAT,
    blend: None,
    write_mask: ColorWrites::empty(),
})];

/// Whether any stage of the program stores to an image or a storage buffer.
pub(crate) fn stores(reflection: &ProgramReflection) -> bool {
    reflection
        .storage_images
        .iter()
        .any(|i| i.write || i.atomic)
        || reflection.storage_buffers.iter().any(|b| !b.read_only)
}

/// Shader modules of a program's stages in reflection entry-point order.
pub(crate) fn shader_modules(
    device: &Device,
    name: &str,
    stages: &[(StageKind, naga::Module)],
) -> Vec<(StageKind, wgpu::ShaderModule)> {
    stages
        .iter()
        .map(|(stage, module)| {
            (
                *stage,
                device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some(&format!("shader pack {name} {}", stage.extension())),
                    source: wgpu::ShaderSource::Naga(Cow::Owned(module.clone())),
                }),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests;
