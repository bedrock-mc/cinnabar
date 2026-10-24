//! Uploads assembled atlas levels and the remap table.

use super::labpbr::{NEUTRAL_NORMAL, NEUTRAL_SPECULAR};
use super::pixels::{AtlasImages, LayerImages, MipImages};
use super::plan::AtlasPlan;
use crate::queue_writes::ProfiledQueue as _;
use wgpu::util::DeviceExt as _;

/// Encoded colour as stored; packs read the plain RGBA8 view and decode sRGB themselves.
pub(crate) const COLOR_STORAGE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// The view shader packs sample as `gtexture`.
pub(crate) const COLOR_VIEW_FORMAT: wgpu::TextureFormat = COLOR_STORAGE_FORMAT;
/// LabPBR data atlases are linear.
pub(crate) const PBR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// GPU copy of one image set: colour, normals and specular.
#[derive(Clone)]
pub(crate) struct LayerTextures {
    pub color: wgpu::Texture,
    /// Normal and specular textures; `None` in a frame store without authored maps.
    pub pbr: Option<[wgpu::Texture; 2]>,
}

/// Uploaded generation before it is published to the render world.
pub(crate) struct UploadedAtlas {
    pub atlas: LayerTextures,
    pub frames: Option<LayerTextures>,
    pub remap: wgpu::Buffer,
}

/// Creates a 2D texture from `images` with every level written through the queue.
fn upload_images(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &'static str,
    images: &MipImages,
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: images.width,
            height: images.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: images.levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    });
    for (level, bytes) in images.levels.iter().enumerate() {
        let (width, height) = (images.width >> level, images.height >> level);
        queue.profiled_write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: level as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }
    texture
}

/// Uploads one image set; neutral PBR levels are generated only when `neutral_pbr` asks.
/// `None` once `cancelled`, checked before each texture.
fn upload_layer(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    labels: [&'static str; 3],
    images: &LayerImages,
    usage: wgpu::TextureUsages,
    neutral_pbr: bool,
    cancelled: &dyn Fn() -> bool,
) -> Option<LayerTextures> {
    let upload = |label, images: &MipImages, format| {
        (!cancelled()).then(|| upload_images(device, queue, label, images, format, usage))
    };
    let color = upload(labels[0], &images.color, COLOR_STORAGE_FORMAT)?;
    let neutral = || {
        let levels = images.color.levels.len() as u32;
        let (width, height) = (images.color.width, images.color.height);
        [
            MipImages::filled(width, height, levels, NEUTRAL_NORMAL),
            MipImages::filled(width, height, levels, NEUTRAL_SPECULAR),
        ]
    };
    let pbr = match &images.pbr {
        Some([normals, specular]) => Some([
            upload(labels[1], normals, PBR_FORMAT)?,
            upload(labels[2], specular, PBR_FORMAT)?,
        ]),
        None if neutral_pbr => {
            let [normals, specular] = neutral();
            Some([
                upload(labels[1], &normals, PBR_FORMAT)?,
                upload(labels[2], &specular, PBR_FORMAT)?,
            ])
        }
        None => None,
    };
    Some(LayerTextures { color, pbr })
}

/// Uploads the atlas (sampled, copied into and rendered into by animation updates), its frame
/// store, and the remap table; `None` once `cancelled`, checked before each texture.
pub(crate) fn upload_atlas(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    plan: &AtlasPlan,
    images: &AtlasImages,
    cancelled: &dyn Fn() -> bool,
) -> Option<UploadedAtlas> {
    let atlas = upload_layer(
        device,
        queue,
        [
            "shader-pack colour atlas",
            "shader-pack normal atlas",
            "shader-pack specular atlas",
        ],
        &images.atlas,
        wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::RENDER_ATTACHMENT,
        true,
        cancelled,
    )?;
    let frames = match &images.frames {
        Some(frames) => Some(upload_layer(
            device,
            queue,
            [
                "shader-pack colour frame store",
                "shader-pack normal frame store",
                "shader-pack specular frame store",
            ],
            frames,
            wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            false,
            cancelled,
        )?),
        None => None,
    };
    if cancelled() {
        return None;
    }
    let remap = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("shader-pack atlas remap"),
        contents: bytemuck::cast_slice(&plan.remap),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    });
    Some(UploadedAtlas {
        atlas,
        frames,
        remap,
    })
}

/// Java's `NEAREST_MIPMAP_LINEAR` terrain filtering, clamped to the atlas levels.
pub(crate) fn atlas_sampler_descriptor(mip_levels: u32) -> wgpu::SamplerDescriptor<'static> {
    wgpu::SamplerDescriptor {
        label: Some("shader-pack atlas sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        lod_max_clamp: mip_levels.saturating_sub(1) as f32,
        ..Default::default()
    }
}
