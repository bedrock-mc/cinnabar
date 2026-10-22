//! Actor pages upload in row bands within a per-frame byte budget and bind only once complete.
use super::*;
use wgpu::Origin3d;

/// Bytes of page pixels written per frame; demanded pages spend it before prewarmed ones.
pub(super) const UPLOAD_BYTES_PER_FRAME: usize = 8 * 1024 * 1024;

/// A created page whose fitted pixels are still being written; it draws nothing until complete.
pub(super) struct PendingPage {
    pub(super) source: TextureKey,
    texture: Texture,
    pixels: Arc<[u8]>,
    size: Extent3d,
    /// Rows written so far, counted across layers in layer-major order.
    written_rows: u32,
    pub(super) color_mask: bool,
    pub(super) multitexture: bool,
    reduce: bool,
}

/// Creates a page's texture without writing it, halving fresh pages that cannot allocate.
pub(super) fn start_page(
    device: &RenderDevice,
    page: &render_model::ActorTexturePage,
    source: TextureKey,
    reduce: bool,
) -> Option<PendingPage> {
    let mut fitted = page
        .fit_within(device.limits().max_texture_dimension_2d)
        .into_owned();
    loop {
        let size = Extent3d {
            width: u32::from(fitted.width),
            height: u32::from(fitted.height),
            depth_or_array_layers: fitted.layers,
        };
        let texture = gpu_allocation::texture(
            device,
            &TextureDescriptor {
                label: Some("immutable actor page"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::Rgba8UnormSrgb,
                usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                view_formats: crate::actor::gpu::ARTWORK_VIEW_FORMATS,
            },
        );
        if let Some(texture) = texture {
            return Some(PendingPage {
                source,
                texture,
                pixels: fitted.shared_pixels(),
                size,
                written_rows: 0,
                color_mask: page.color_mask,
                multitexture: page.multitexture,
                reduce,
            });
        }
        let side = u32::from(fitted.width.max(fitted.height));
        if !reduce || side <= 1 {
            return None;
        }
        fitted = fitted.fit_within((side / 2).max(1)).into_owned();
    }
}

/// How far one frame's writes took a pending page.
pub(super) enum Progress {
    Done(GpuArtworkPage),
    Pending(PendingPage),
    /// A write or the view ran out of memory; the texture is released.
    Failed,
}

/// Writes whole rows while `budget` lasts, publishing the page once every row is written.
pub(super) fn continue_page(
    device: &RenderDevice,
    queue: &RenderQueue,
    mut pending: PendingPage,
    budget: &mut usize,
) -> Progress {
    let Extent3d {
        width,
        height,
        depth_or_array_layers: layers,
    } = pending.size;
    let row_bytes = width as usize * 4;
    while pending.written_rows < height * layers && *budget >= row_bytes {
        let (layer, y) = (pending.written_rows / height, pending.written_rows % height);
        let rows = ((*budget / row_bytes).min((height - y) as usize)) as u32;
        let band_layers = if y == 0 && rows == height {
            (*budget / (height as usize * row_bytes)).min((layers - layer) as usize) as u32
        } else {
            1
        };
        let start = pending.written_rows as usize * row_bytes;
        let bytes = rows as usize * band_layers as usize * row_bytes;
        *budget -= bytes;
        let written = gpu_allocation::checked(device, "actor page upload", bytes as u64, || {
            queue.profiled_write_texture(
                TexelCopyTextureInfo {
                    texture: &pending.texture,
                    mip_level: 0,
                    origin: Origin3d { x: 0, y, z: layer },
                    aspect: default(),
                },
                &pending.pixels[start..start + bytes],
                TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width * 4),
                    rows_per_image: Some(rows),
                },
                Extent3d {
                    width,
                    height: rows,
                    depth_or_array_layers: band_layers,
                },
            );
        });
        if written.is_none() {
            return retry_smaller(device, pending);
        }
        pending.written_rows += rows * band_layers;
    }
    if pending.written_rows < height * layers {
        return Progress::Pending(pending);
    }
    #[cfg(test)]
    super::work_tests::record_upload(pending.pixels.len());
    let Some(view) = gpu_allocation::checked(device, "actor page view", 0, || {
        pending.texture.create_view(&TextureViewDescriptor {
            dimension: Some(TextureViewDimension::D2Array),
            ..default()
        })
    }) else {
        return retry_smaller(device, pending);
    };
    Progress::Done(GpuArtworkPage {
        source: pending.source,
        _texture: pending.texture,
        view,
        bind_group: None,
        color_mask: pending.color_mask,
        multitexture: pending.multitexture,
    })
}

/// Retries fresh pages at lower resolution after staging or view allocation fails.
fn retry_smaller(device: &RenderDevice, pending: PendingPage) -> Progress {
    let side = pending.size.width.max(pending.size.height);
    if !pending.reduce || side <= 1 {
        return Progress::Failed;
    }
    let page = render_model::ActorTexturePage {
        width: pending.size.width as u16,
        height: pending.size.height as u16,
        layers: pending.size.depth_or_array_layers,
        rgba8: pending.pixels,
        color_mask: pending.color_mask,
        multitexture: pending.multitexture,
    };
    drop(pending.texture);
    let page = page.fit_within(side / 2);
    start_page(device, &page, pending.source, true).map_or(Progress::Failed, Progress::Pending)
}
