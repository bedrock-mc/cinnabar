//! What each sampler and image name of a program binds, and the pack-wide textures that do
//! not depend on the view: `noisetex`, custom textures from `shaders.properties`, the
//! shadow maps (cleared until the geometry adapter renders them) and placeholders for names
//! the host cannot supply.

use super::pipelines::GpuContext;
use super::pipelines::{FormatSupport, storage_format, texture_format, texture_sample_type};
use super::plan::{MAX_COLOR_BUFFERS, color_buffer_of};
use raw::RawSpec;
use shaderpack::{
    BufferFormat, BufferId, CustomImage, ImageSize, ProgramGroup, ProgramReflection, SampleType,
    ShaderPack, TextureDimension, TextureSource as PackTexture,
};
use std::collections::HashMap;
use vanilla::VanillaTextures;
use wgpu::{TextureFormat, TextureUsages};

pub(crate) mod bindings;
pub(super) mod raw;
pub(crate) mod vanilla;

/// Iris's default `noiseTextureResolution`.
const DEFAULT_NOISE_RESOLUTION: u32 = 256;
/// Iris's default `shadowMapResolution`.
const DEFAULT_SHADOW_RESOLUTION: u32 = 1024;
/// Largest custom texture side the host decodes.
const MAX_CUSTOM_TEXTURE_SIDE: u32 = 8192;

/// What a sampled texture binding reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextureSource {
    /// `colortexN`, on the side the pass reads.
    Buffer { index: u32, format: TextureFormat },
    /// `depthtexN` as floats: `depthtex0` reads the geometry depth attachment itself where
    /// nothing draws into it ([`super::targets::ViewTargets`]), the others their copies.
    Depth { index: usize, format: TextureFormat },
    /// `depthtexN` for the comparison sampler.
    DepthCompare(usize),
    /// A custom image read through its sampler name.
    Image { index: usize, format: TextureFormat },
    /// A pack-wide texture.
    Constant { index: usize, format: TextureFormat },
    /// The block atlas Cinnabar builds for geometry programs (`gtexture`, `normals`,
    /// `specular`).
    Atlas(AtlasLayer),
}

/// One of the block atlases.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AtlasLayer {
    Color,
    Normals,
    Specular,
}

/// The format every atlas view has.
pub(crate) const ATLAS_FORMAT: TextureFormat = TextureFormat::Rgba8Unorm;

impl TextureSource {
    pub(crate) fn format(&self) -> TextureFormat {
        match *self {
            TextureSource::Buffer { format, .. }
            | TextureSource::Depth { format, .. }
            | TextureSource::Image { format, .. }
            | TextureSource::Constant { format, .. } => format,
            TextureSource::DepthCompare(_) => TextureFormat::Depth32Float,
            TextureSource::Atlas(_) => ATLAS_FORMAT,
        }
    }

    /// The sample type its binding declares. `depthtex` reads use the non-filtering
    /// `ClampNearest` sampler, so they bind as unfilterable floats, which both the float
    /// copies and the geometry depth attachment satisfy.
    pub(crate) fn sample_type(&self, support: FormatSupport) -> wgpu::TextureSampleType {
        match self {
            TextureSource::Depth { .. } => wgpu::TextureSampleType::Float { filterable: false },
            source => texture_sample_type(source.format(), support),
        }
    }
}

/// The atlas layer a geometry program's sampler name reads, as Iris binds them.
fn atlas_layer(name: &str) -> Option<AtlasLayer> {
    match name {
        "gtexture" | "texture" | "tex" => Some(AtlasLayer::Color),
        "normals" => Some(AtlasLayer::Normals),
        "specular" => Some(AtlasLayer::Specular),
        _ => None,
    }
}

/// What a storage-image binding writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImageSource {
    /// `colorimgN`: the buffer's current side.
    Buffer(u32),
    /// A custom image.
    Image(usize),
    /// A placeholder the program may write but nothing reads.
    Constant(usize),
}

/// A program's bindings, parallel to its reflection's textures and storage images.
pub(crate) struct ProgramBindings {
    pub textures: Vec<TextureSource>,
    pub images: Vec<ImageSource>,
}

pub(crate) struct ConstantTexture {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
}

/// Pack-wide textures, indexed by [`TextureSource::Constant`] and [`ImageSource::Constant`].
#[derive(Default)]
pub(crate) struct ConstantTextures {
    pub textures: Vec<ConstantTexture>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Key {
    Noise,
    Custom(String, String, TextureDimension),
    ShadowDepth(u32),
    ShadowFloat(u32),
    ShadowColor(u32),
    Placeholder(TextureFormat, TextureDimension, bool),
}

/// Creates pack-wide textures on demand while programs are resolved.
pub(crate) struct ConstantBuilder<'a> {
    gpu: &'a GpuContext,
    pack: &'a ShaderPack,
    translation: &'a shaderpack::PackTranslation,
    /// What `minecraft:` resource references resolve to.
    vanilla: &'a VanillaTextures,
    support: FormatSupport,
    shadow_resolution: u32,
    noise_resolution: u32,
    keys: HashMap<Key, (usize, TextureFormat)>,
    out: ConstantTextures,
    clears: Vec<(usize, wgpu::LoadOp<f32>, bool)>,
}

fn const_value<'a>(translation: &'a shaderpack::PackTranslation, name: &str) -> Option<&'a str> {
    translation
        .programs
        .iter()
        .filter(|p| p.enabled)
        .flat_map(|p| &p.consts)
        .rev()
        .find(|c| c.name == name)
        .map(|c| c.value.as_str())
}

impl<'a> ConstantBuilder<'a> {
    pub(crate) fn new(
        gpu: &'a GpuContext,
        pack: &'a ShaderPack,
        translation: &'a shaderpack::PackTranslation,
        vanilla: &'a VanillaTextures,
        support: FormatSupport,
    ) -> Self {
        let number = |name: &str, default: u32| {
            const_value(translation, name)
                .and_then(|v| v.trim().trim_end_matches(['f', 'F']).parse::<f32>().ok())
                .map_or(default, |v| (v as u32).max(1))
        };
        Self {
            gpu,
            pack,
            translation,
            vanilla,
            support,
            shadow_resolution: number("shadowMapResolution", DEFAULT_SHADOW_RESOLUTION),
            noise_resolution: number("noiseTextureResolution", DEFAULT_NOISE_RESOLUTION),
            keys: HashMap::new(),
            out: ConstantTextures::default(),
            clears: vec![],
        }
    }

    fn push(
        &mut self,
        key: Key,
        texture: wgpu::Texture,
        view: wgpu::TextureView,
    ) -> (usize, TextureFormat) {
        let format = texture.format();
        let index = self.out.textures.len();
        self.out.textures.push(ConstantTexture { texture, view });
        self.keys.insert(key, (index, format));
        (index, format)
    }

    fn texture(
        &self,
        label: &str,
        format: TextureFormat,
        size: [u32; 3],
        dimension: TextureDimension,
        usage: TextureUsages,
    ) -> (wgpu::Texture, wgpu::TextureView) {
        let (wgpu_dimension, layers, view_dimension) = match dimension {
            TextureDimension::D1 => (
                wgpu::TextureDimension::D1,
                1,
                wgpu::TextureViewDimension::D1,
            ),
            TextureDimension::D2 => (
                wgpu::TextureDimension::D2,
                1,
                wgpu::TextureViewDimension::D2,
            ),
            TextureDimension::D2Array => (
                wgpu::TextureDimension::D2,
                1,
                wgpu::TextureViewDimension::D2Array,
            ),
            TextureDimension::D3 => (
                wgpu::TextureDimension::D3,
                size[2],
                wgpu::TextureViewDimension::D3,
            ),
            TextureDimension::Cube => (
                wgpu::TextureDimension::D2,
                6,
                wgpu::TextureViewDimension::Cube,
            ),
            TextureDimension::CubeArray => (
                wgpu::TextureDimension::D2,
                6,
                wgpu::TextureViewDimension::CubeArray,
            ),
        };
        let texture = self.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size[0].max(1),
                height: if wgpu_dimension == wgpu::TextureDimension::D1 {
                    1
                } else {
                    size[1].max(1)
                },
                depth_or_array_layers: layers.max(1),
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu_dimension,
            format,
            usage,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(view_dimension),
            ..Default::default()
        });
        (texture, view)
    }

    /// A zero (depth: 1.0) texture of `format` and shape, shared by every unknown binding of
    /// the same type.
    pub(crate) fn placeholder(
        &mut self,
        format: TextureFormat,
        dimension: TextureDimension,
        storage: bool,
    ) -> (usize, TextureFormat) {
        let key = Key::Placeholder(format, dimension, storage);
        if let Some(found) = self.keys.get(&key) {
            return *found;
        }
        let depth = format.is_depth_stencil_format();
        let dimension = if depth {
            TextureDimension::D2
        } else {
            dimension
        };
        let mut usage = TextureUsages::TEXTURE_BINDING;
        if storage {
            usage |= TextureUsages::STORAGE_BINDING;
        }
        if depth {
            usage |= TextureUsages::RENDER_ATTACHMENT;
        }
        let (texture, view) = self.texture(
            "shader pack placeholder",
            format,
            [1, 1, 1],
            dimension,
            usage,
        );
        let found = self.push(key, texture, view);
        if depth {
            self.clears.push((found.0, wgpu::LoadOp::Clear(1.0), true));
        }
        found
    }

    fn placeholder_for(
        &mut self,
        sample: SampleType,
        dimension: TextureDimension,
    ) -> TextureSource {
        let format = match sample {
            SampleType::Float => TextureFormat::Rgba8Unorm,
            SampleType::Sint => TextureFormat::Rgba8Sint,
            SampleType::Uint => TextureFormat::Rgba8Uint,
            SampleType::Depth => TextureFormat::Depth32Float,
        };
        let (index, format) = self.placeholder(format, dimension, false);
        TextureSource::Constant { index, format }
    }

    /// Resolves noise, rejecting unsupported generated extents before allocating pixels.
    fn noise(&mut self) -> Result<(usize, TextureFormat), String> {
        if let Some(found) = self.keys.get(&Key::Noise) {
            return Ok(*found);
        }
        let file = self
            .translation
            .properties
            .custom_textures
            .iter()
            .rev()
            .find(|t| t.stage == "noise")
            .and_then(|t| self.decode(&t.source));
        let (texture, view) = match file {
            Some(decoded) => decoded,
            None => {
                let side = self.noise_resolution;
                let pixels = generated_noise(side, &self.gpu.device.limits())?;
                let (texture, view) = self.texture(
                    "shader pack noisetex",
                    TextureFormat::Rgba8Unorm,
                    [side, side, 1],
                    TextureDimension::D2,
                    TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                );
                self.upload(&texture, &pixels, 4 * side, [side, side, 1]);
                (texture, view)
            }
        };
        Ok(self.push(Key::Noise, texture, view))
    }

    fn upload(&self, texture: &wgpu::Texture, bytes: &[u8], bytes_per_row: u32, size: [u32; 3]) {
        self.gpu.queue.write_texture(
            texture.as_image_copy(),
            bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(size[1]),
            },
            wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: size[2],
            },
        );
    }

    /// Decodes a PNG, raw or vanilla resource custom texture; `None` (logged) when it cannot
    /// be read.
    fn decode(&self, source: &PackTexture) -> Option<(wgpu::Texture, wgpu::TextureView)> {
        let result = match source {
            PackTexture::File(path) => self.decode_png(path),
            PackTexture::Raw { path, spec } => self.decode_raw(path, spec),
            PackTexture::Resource(location) => self.vanilla.resolve(location).and_then(|image| {
                self.rgba_texture(location, image.width, image.height, &image.rgba8)
            }),
        };
        result
            .inspect_err(|error| {
                bevy::log::warn!(target: "cinnabar::shaderpack", "custom texture skipped: {error}");
            })
            .ok()
    }

    fn decode_png(&self, path: &str) -> Result<(wgpu::Texture, wgpu::TextureView), String> {
        let bytes = self
            .pack
            .source()
            .read_bytes(path)
            .map_err(|e| format!("{path}: {e}"))?;
        let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info().map_err(|e| format!("{path}: {e}"))?;
        let header = reader.info();
        let size = png_buffer_len(
            header.width,
            header.height,
            reader.output_buffer_size(),
            MAX_CUSTOM_TEXTURE_SIDE.min(self.gpu.device.limits().max_texture_dimension_2d),
        )?;
        let mut pixels = vec![0; size];
        let info = reader
            .next_frame(&mut pixels)
            .map_err(|e| format!("{path}: {e}"))?;
        let channels = info.color_type.samples();
        let rgba: Vec<u8> = pixels[..info.buffer_size()]
            .chunks_exact(channels)
            .flat_map(|p| match *p {
                [v] => [v, v, v, 255],
                [v, a] => [v, v, v, a],
                [r, g, b] => [r, g, b, 255],
                [r, g, b, a] => [r, g, b, a],
                _ => [0, 0, 0, 255],
            })
            .collect();
        self.rgba_texture(path, info.width, info.height, &rgba)
    }

    /// A texture of top-row-first RGBA8 `rgba`.
    fn rgba_texture(
        &self,
        label: &str,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Result<(wgpu::Texture, wgpu::TextureView), String> {
        let expected = rgba_byte_len(
            width,
            height,
            MAX_CUSTOM_TEXTURE_SIDE.min(self.gpu.device.limits().max_texture_dimension_2d),
        )
        .map_err(|error| format!("{label}: {error}"))?;
        let row = 4 * width as usize;
        if rgba.len() != expected {
            return Err(format!(
                "{label}: {width}x{height} does not match its pixels"
            ));
        }
        // Images are stored bottom-up in pack textures, as OpenGL uploads them.
        let flipped: Vec<u8> = rgba.chunks_exact(row).rev().flatten().copied().collect();
        let (texture, view) = self.texture(
            label,
            TextureFormat::Rgba8Unorm,
            [width, height, 1],
            TextureDimension::D2,
            TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        );
        self.upload(&texture, &flipped, 4 * width, [width, height, 1]);
        Ok((texture, view))
    }

    /// `<TYPE> <internalFormat> <width> [<height> [<depth>]] <pixelFormat> <pixelType>`.
    fn decode_raw(
        &self,
        path: &str,
        spec: &[String],
    ) -> Result<(wgpu::Texture, wgpu::TextureView), String> {
        let raw = RawSpec::parse(spec)
            .ok_or_else(|| format!("{path}: unsupported raw texture {spec:?}"))?;
        super::targets::admission::texture_extent(
            storage_shape(raw.dimension),
            raw.size,
            &self.gpu.device.limits(),
        )
        .map_err(|error| format!("{path}: {error}"))?;
        let bytes = self
            .pack
            .source()
            .read_bytes(path)
            .map_err(|e| format!("{path}: {e}"))?;
        let format = texture_format(raw.internal, self.support);
        let pixels = raw
            .convert(&bytes, format)
            .ok_or_else(|| format!("{path}: data does not match {spec:?}"))?;
        let block = format.block_copy_size(None).unwrap_or(4);
        let (texture, view) = self.texture(
            path,
            format,
            raw.size,
            raw.dimension,
            TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        );
        let size = if raw.dimension == TextureDimension::D1 {
            [raw.size[0], 1, 1]
        } else {
            raw.size
        };
        self.upload(&texture, &pixels, block * size[0], size);
        Ok((texture, view))
    }

    /// The stage's custom texture for sampler `name` declared with `dimension`: OptiFine binds
    /// each shape on its own target of the unit, so `texture.composite.colortex0 = x.dat
    /// TEXTURE_3D` replaces only a `sampler3D colortex0` and a `sampler2D colortex0` still
    /// reads the buffer.
    fn custom(
        &mut self,
        stage: &str,
        name: &str,
        dimension: TextureDimension,
    ) -> Option<(usize, TextureFormat)> {
        let key = Key::Custom(stage.to_string(), name.to_string(), dimension);
        if let Some(found) = self.keys.get(&key) {
            return Some(*found);
        }
        let source = self
            .translation
            .properties
            .custom_textures
            .iter()
            .rev()
            .find(|t| t.stage == stage && t.uniform() == name && t.source.dimension() == dimension)?
            .source
            .clone();
        let (texture, view) = self.decode(&source)?;
        Some(self.push(key, texture, view))
    }

    /// Creates a requested shadow texture after checking its device extent.
    fn shadow(&mut self, index: u32, sample: SampleType) -> Result<(usize, TextureFormat), String> {
        let key = if sample == SampleType::Depth {
            Key::ShadowDepth(index)
        } else {
            Key::ShadowFloat(index)
        };
        if let Some(found) = self.keys.get(&key) {
            return Ok(*found);
        }
        let side = self.shadow_resolution;
        super::targets::admission::texture_extent(
            wgpu::TextureDimension::D2,
            [side, side, 1],
            &self.gpu.device.limits(),
        )?;
        let format = if sample == SampleType::Depth {
            TextureFormat::Depth32Float
        } else if self.support.float32_filterable {
            TextureFormat::R32Float
        } else {
            TextureFormat::R16Float
        };
        let (texture, view) = self.texture(
            "shader pack shadowtex",
            format,
            [side, side, 1],
            TextureDimension::D2,
            TextureUsages::TEXTURE_BINDING
                | TextureUsages::RENDER_ATTACHMENT
                | TextureUsages::COPY_SRC
                | TextureUsages::COPY_DST,
        );
        let found = self.push(key, texture, view);
        self.clears.push((
            found.0,
            wgpu::LoadOp::Clear(1.0),
            sample == SampleType::Depth,
        ));
        Ok(found)
    }

    /// `shadowcolorN`, created on first use; the shadow pass draws into it.
    pub(crate) fn shadow_color(&mut self, index: u32) -> Result<(usize, TextureFormat), String> {
        if let Some(found) = self.keys.get(&Key::ShadowColor(index)) {
            return Ok(*found);
        }
        let side = self.shadow_resolution;
        super::targets::admission::texture_extent(
            wgpu::TextureDimension::D2,
            [side, side, 1],
            &self.gpu.device.limits(),
        )?;
        let format = texture_format(
            self.translation
                .buffers
                .format(BufferId::ShadowColor(index)),
            self.support,
        );
        let (texture, view) = self.texture(
            "shader pack shadowcolor",
            format,
            [side, side, 1],
            TextureDimension::D2,
            TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT,
        );
        let found = self.push(Key::ShadowColor(index), texture, view);
        self.clears.push((found.0, wgpu::LoadOp::Clear(1.0), false));
        Ok(found)
    }

    /// Whether pack-wide texture `index` has the shape a sampler of `dimension` reads.
    fn has_shape(&self, index: usize, dimension: TextureDimension) -> bool {
        self.out.textures[index].texture.dimension() == storage_shape(dimension)
    }

    /// Clears the textures that start at a value other than zero and returns them.
    pub(crate) fn finish(self) -> ConstantTextures {
        if !self.clears.is_empty() {
            let mut encoder =
                self.gpu
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("shader pack constant clears"),
                    });
            for (index, value, depth) in &self.clears {
                let view = &self.out.textures[*index].view;
                let wgpu::LoadOp::Clear(value) = *value else {
                    continue;
                };
                let colour = [Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(value),
                            g: f64::from(value),
                            b: f64::from(value),
                            a: f64::from(value),
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })];
                let depth_attachment = depth.then(|| wgpu::RenderPassDepthStencilAttachment {
                    view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(value),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                });
                encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("shader pack constant clear"),
                    color_attachments: if *depth { &[] } else { &colour },
                    depth_stencil_attachment: depth_attachment,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
            }
            self.gpu.queue.submit([encoder.finish()]);
        }
        self.out
    }
}

/// Deterministic RGBA noise, rejected before allocation when its extent exceeds the device.
fn generated_noise(side: u32, limits: &wgpu::Limits) -> Result<Vec<u8>, String> {
    super::targets::admission::texture_extent(wgpu::TextureDimension::D2, [side, side, 1], limits)?;
    let bytes = rgba_byte_len(side, side, limits.max_texture_dimension_2d)?;
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    Ok((0..bytes)
        .map(|i| {
            if i % 4 == 3 {
                return 255;
            }
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 56) as u8
        })
        .collect())
}

/// The buffer index of `depthtexN` (with `gdepthtex` as `depthtex0`).
fn depth_index(name: &str) -> Option<usize> {
    match name {
        "depthtex0" | "gdepthtex" => Some(0),
        "depthtex1" => Some(1),
        "depthtex2" => Some(2),
        _ => None,
    }
}

/// The storage shape of a texture a sampler of `dimension` reads: arrays and cubes are 2D
/// layers.
fn storage_shape(dimension: TextureDimension) -> wgpu::TextureDimension {
    match dimension {
        TextureDimension::D1 => wgpu::TextureDimension::D1,
        TextureDimension::D3 => wgpu::TextureDimension::D3,
        TextureDimension::D2
        | TextureDimension::D2Array
        | TextureDimension::Cube
        | TextureDimension::CubeArray => wgpu::TextureDimension::D2,
    }
}

/// The texture format of each custom image: the format a program declares for it, else the
/// mapping of its `internalFormat`.
pub(crate) fn image_formats(
    images: &[CustomImage],
    reflections: &[&ProgramReflection],
    support: FormatSupport,
) -> Vec<TextureFormat> {
    images
        .iter()
        .map(|image| {
            reflections
                .iter()
                .flat_map(|r| &r.storage_images)
                .find(|i| i.name == image.name)
                .and_then(|i| storage_format(i.format))
                .or_else(|| {
                    BufferFormat::parse(&image.internal_format).map(|f| texture_format(f, support))
                })
                .unwrap_or(TextureFormat::Rgba8Unorm)
        })
        .collect()
}

/// Admitted RGBA8 bytes for a nonempty image, with checked size arithmetic.
fn rgba_byte_len(width: u32, height: u32, limit: u32) -> Result<usize, String> {
    if width == 0 || height == 0 || width > limit || height > limit {
        return Err(format!(
            "{width}x{height} exceeds the texture limit {limit}"
        ));
    }
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|size| size.checked_mul(4))
        .ok_or_else(|| "RGBA byte count overflows".into())
}

/// Validates the PNG header and decoded byte count before allocating caller-owned pixels.
fn png_buffer_len(
    width: u32,
    height: u32,
    output: Option<usize>,
    limit: u32,
) -> Result<usize, String> {
    let rgba = rgba_byte_len(width, height, limit)?;
    output
        .filter(|size| *size != 0 && *size <= rgba)
        .ok_or_else(|| "png decoded byte count exceeds its admitted extent".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_names_and_generated_noise_follow_optifine() {
        assert_eq!(depth_index("gdepthtex"), Some(0));
        assert_eq!(depth_index("depthtex3"), None);
        let noise = generated_noise(4, &wgpu::Limits::default()).unwrap();
        assert_eq!(noise.len(), 64);
        assert!(noise.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
        assert!(noise.as_chunks::<4>().0.iter().any(|p| p[0] != noise[0]));
    }
    /// A tiny compressed image with a valid CRC and a supplied canvas size.
    fn png_header(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = vec![];
        {
            let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&[0; 4])
                .unwrap();
        }
        bytes[16..20].copy_from_slice(&width.to_be_bytes());
        bytes[20..24].copy_from_slice(&height.to_be_bytes());
        let crc = bytes[12..29].iter().fold(!0_u32, |crc, byte| {
            (0..8).fold(crc ^ u32::from(*byte), |crc, _| {
                (crc >> 1) ^ if crc & 1 != 0 { 0xedb8_8320 } else { 0 }
            })
        });
        bytes[29..33].copy_from_slice(&(!crc).to_be_bytes());
        bytes
    }

    #[test]
    fn oversized_png_headers_are_rejected_before_decode_allocation() {
        let bytes = png_header(32768, 32768);
        let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let reader = decoder.read_info().unwrap();
        let header = reader.info();
        assert!(
            png_buffer_len(
                header.width,
                header.height,
                reader.output_buffer_size(),
                MAX_CUSTOM_TEXTURE_SIDE
            )
            .is_err()
        );
        assert_eq!(
            png_buffer_len(2, 3, Some(24), MAX_CUSTOM_TEXTURE_SIDE).unwrap(),
            24
        );
        assert!(png_buffer_len(0, 3, Some(0), MAX_CUSTOM_TEXTURE_SIDE).is_err());
        assert!(png_buffer_len(2, 3, Some(25), MAX_CUSTOM_TEXTURE_SIDE).is_err());
    }
    #[test]
    fn generated_noise_rejects_unsupported_extents_before_allocating_pixels() {
        let limits = wgpu::Limits {
            max_texture_dimension_2d: 4096,
            ..Default::default()
        };
        assert!(generated_noise(8192, &limits).is_err());
        assert!(generated_noise(0, &limits).is_err());
        assert!(png_buffer_len(4097, 1, Some(16388), limits.max_texture_dimension_2d).is_err());
    }
}
