//! Particle atlas uploads that survive GPU memory exhaustion by releasing, retrying and reducing.

use super::*;
use crate::gpu_allocation::{self, UploadRetry};

/// Smallest reduced atlas side; reduction also stops before a tile slot drops below one texel.
const MIN_REDUCED_SIDE: u32 = particles::ATLAS_SIDE / 4;

/// The uploaded atlas generation, including a pending retry after a failed replacement.
#[derive(Default)]
pub(super) struct AtlasUpload {
    /// Power-of-two box-filter factor; normalized placements sample the same texels.
    pub(super) reduction: u32,
    /// Side of the full-size base, which a retry may upload after newer frames arrive.
    side: u32,
    pub(super) retry: UploadRetry,
}

/// Replaces the atlas when the frame publishes a new base, then uploads newer dynamic tiles.
pub(super) fn prepare(
    frame: &ParticleGpuFrame,
    device: &RenderDevice,
    queue: &RenderQueue,
    gpu: &mut ParticleGpu,
) {
    if let Some(base) = &frame.base
        && gpu.base.as_ref().is_none_or(|old| !Arc::ptr_eq(old, base))
        && assets::texture::policy::TexturePolicy::for_device(
            device.limits().max_texture_dimension_2d,
        )
        .page_bytes(frame.atlas_side, frame.atlas_side)
            == Some(base.len())
    {
        // Old placements are invalid for the new base, so release its atlas before allocating.
        let released = gpu.texture.take().is_some();
        gpu.texture_view = None;
        gpu.bind_group = None;
        gpu.base = Some(Arc::clone(base));
        gpu.uploaded_seq = 0;
        gpu.atlas = AtlasUpload {
            side: frame.atlas_side,
            ..AtlasUpload::default()
        };
        // A full-size retry may fit once in-flight frames retire the released atlas.
        install(gpu, device, queue, !released);
        let uploaded = gpu.texture.is_some();
        gpu.atlas.retry.record(uploaded, released, queue);
    } else if gpu.atlas.retry.take_ready() {
        install(gpu, device, queue, true);
        let uploaded = gpu.texture.is_some();
        gpu.atlas.retry.record(uploaded, false, queue);
    }
    if let Some(texture) = &gpu.texture
        && frame.patch_seq != gpu.uploaded_seq
    {
        let factor = gpu.atlas.reduction.max(1);
        for patch in frame.patches.iter().filter(|p| p.seq > gpu.uploaded_seq) {
            let origin = [patch.x / factor, patch.y / factor];
            if factor == 1 {
                write_rect(
                    queue,
                    texture,
                    origin,
                    [patch.width, patch.height],
                    &patch.rgba8,
                );
            } else if let Some((size, pixels)) =
                reduce(patch.width, patch.height, &patch.rgba8, factor)
            {
                write_rect(queue, texture, origin, size, &pixels);
            }
        }
        gpu.uploaded_seq = frame.patch_seq;
    }
}

/// Uploads the current base at full size, then, when `reduce` allows, at halved sizes.
fn install(gpu: &mut ParticleGpu, device: &RenderDevice, queue: &RenderQueue, reduce: bool) {
    let (Some(base), side) = (gpu.base.clone(), gpu.atlas.side) else {
        return;
    };
    let mut factor = 1;
    loop {
        if let Some((texture, view)) = upload(device, queue, side, &base, factor) {
            publish(gpu, texture, view, factor);
            return;
        }
        factor *= 2;
        if !reduce || factor > particles::TILE_SLOT || side / factor < MIN_REDUCED_SIDE {
            return;
        }
    }
}

/// Records an uploaded atlas; its dynamic tiles upload again at its reduction.
fn publish(gpu: &mut ParticleGpu, texture: Texture, view: TextureView, factor: u32) {
    gpu.texture = Some(texture);
    gpu.texture_view = Some(view);
    gpu.uploaded_seq = 0;
    gpu.bind_group = None;
    gpu.atlas.reduction = factor;
}

/// Creates and fills one atlas inside a recoverable allocation scope.
fn upload(
    device: &RenderDevice,
    queue: &RenderQueue,
    side: u32,
    base: &[u8],
    factor: u32,
) -> Option<(Texture, TextureView)> {
    let reduced = reduce(side, side, base, factor);
    let (extent, pixels) = match &reduced {
        Some(([width, _], pixels)) => (*width, pixels.as_slice()),
        None if factor == 1 => (side, base),
        None => return None,
    };
    let texture = gpu_allocation::checked(device, "particle atlas", pixels.len() as u64, || {
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("particle atlas"),
            size: Extent3d {
                width: extent,
                height: extent,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            // Shader packs read the stored gamma bytes through an `Rgba8Unorm` view.
            view_formats: crate::actor::gpu::ARTWORK_VIEW_FORMATS,
        });
        write_rect(queue, &texture, [0, 0], [extent, extent], pixels);
        texture
    })?;
    let view = gpu_allocation::checked(device, "particle atlas view", 0, || {
        texture.create_view(&TextureViewDescriptor::default())
    })?;
    Some((texture, view))
}

/// Box-filters `rgba` by `factor`; `None` when no reduction applies or it cannot be expressed.
fn reduce(width: u32, height: u32, rgba: &[u8], factor: u32) -> Option<([u32; 2], Vec<u8>)> {
    if factor <= 1 {
        return None;
    }
    let (width, height) = (u16::try_from(width).ok()?, u16::try_from(height).ok()?);
    let limit = u32::from(width.max(height)) / factor;
    render_model::fit_rgba_within(width, height, rgba, limit.max(1))
        .map(|([width, height], pixels)| ([u32::from(width), u32::from(height)], pixels))
}

/// Queues one RGBA rectangle at `origin` in texel rows of `size[0]`.
fn write_rect(
    queue: &RenderQueue,
    texture: &Texture,
    origin: [u32; 2],
    size: [u32; 2],
    rgba: &[u8],
) {
    #[cfg(feature = "tracy")]
    let _span = bevy::log::info_span!("particles.texture_write", bytes = rgba.len()).entered();
    queue.profiled_write_texture(
        TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: Origin3d {
                x: origin[0],
                y: origin[1],
                z: 0,
            },
            aspect: Default::default(),
        },
        rgba,
        TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(size[0] * 4),
            rows_per_image: Some(size[1]),
        },
        Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
    );
}
