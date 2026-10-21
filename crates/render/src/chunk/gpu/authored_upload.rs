//! Authored-only replacements preserve the carrier and publish after bounded complete uploads.

use crate::chunk::*;
use crate::queue_writes::ProfiledQueue as _;
use render_scene::{ChunkTextureBindingIdentity, EnhancedTextureAssets};

#[cfg(test)]
use super::bind_groups::same_carrier;
use super::bind_groups::{ChunkTextureUploadStats, PreparedChunkTextureAssets};

pub(super) const FRAME_UPLOAD_BYTES: usize = 8 * 1024 * 1024;
const _: () =
    assert!((std::mem::size_of::<u32>() as u64).is_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT));
const _: () = assert!(
    EnhancedTextureAssets::REFERENCE_BASE_WORDS * std::mem::size_of::<u32>() <= FRAME_UPLOAD_BYTES
);

#[derive(Clone, Copy, Debug)]
struct MipTask {
    page: usize,
    mip: u32,
    size: u32,
    layers: u32,
}

#[derive(Clone, Copy, Debug)]
struct Stripe {
    page: usize,
    mip: u32,
    size: u32,
    layer: u32,
    layer_count: u32,
    row: u32,
    rows: u32,
    offset: usize,
    bytes: usize,
}

struct Schedule {
    tasks: Vec<MipTask>,
    task: usize,
    layer: u32,
    row: u32,
}

impl Schedule {
    /// Validates every mip and starts a layer-major upload schedule.
    fn new(pages: &[&TextureArray; 6]) -> Option<Self> {
        let mut tasks = Vec::new();
        for (page, array) in pages.iter().enumerate() {
            let side = array.mips.first()?.size;
            if !side.is_power_of_two()
                || array.layers == 0
                || array.mips.len() != side.ilog2() as usize + 1
            {
                return None;
            }
            for (mip, data) in array.mips.iter().enumerate() {
                let size = side >> mip;
                let bytes = (size as usize)
                    .checked_mul(size as usize)?
                    .checked_mul(array.layers as usize)?
                    .checked_mul(4)?;
                if data.size != size || data.rgba8.len() != bytes {
                    return None;
                }
                tasks.push(MipTask {
                    page,
                    mip: mip as u32,
                    size,
                    layers: array.layers,
                });
            }
        }
        Some(Self {
            tasks,
            task: 0,
            layer: 0,
            row: 0,
        })
    }

    /// Takes complete rows or layers that fit the remaining byte budget.
    fn next(&mut self, budget: usize) -> Option<Stripe> {
        let task = *self.tasks.get(self.task)?;
        let row_bytes = task.size as usize * 4;
        let layer_bytes = row_bytes * task.size as usize;
        if budget < row_bytes {
            return None;
        }
        let (layer_count, rows) = if self.row == 0 && budget >= layer_bytes {
            (
                (budget / layer_bytes).min((task.layers - self.layer) as usize) as u32,
                task.size,
            )
        } else {
            (
                1,
                (budget / row_bytes).min((task.size - self.row) as usize) as u32,
            )
        };
        let stripe = Stripe {
            page: task.page,
            mip: task.mip,
            size: task.size,
            layer: self.layer,
            layer_count,
            row: self.row,
            rows,
            offset: self.layer as usize * layer_bytes + self.row as usize * row_bytes,
            bytes: rows as usize * row_bytes * layer_count as usize,
        };
        self.row += rows;
        if self.row == task.size {
            self.row = 0;
            self.layer += layer_count;
            if self.layer == task.layers {
                self.layer = 0;
                self.task += 1;
            }
        }
        Some(stripe)
    }

    /// Reports whether every scheduled source byte has been submitted.
    fn complete(&self) -> bool {
        self.task == self.tasks.len()
    }
}

pub(super) struct AuthoredUpload {
    identity: ChunkTextureBindingIdentity,
    source: Arc<EnhancedTextureAssets>,
    textures: [Texture; 6],
    views: [TextureView; 6],
    references: Buffer,
    reference_words_written: usize,
    schedule: Schedule,
    written: u64,
    texture_bytes: u64,
}

impl AuthoredUpload {
    /// Starts a bounded upload only when requested maps differ from the bound source.
    pub(super) fn for_assets(
        assets: &ChunkTextureAssets,
        prepared: &PreparedChunkTextureAssets,
        device: &RenderDevice,
    ) -> Option<Self> {
        let source = assets.enhanced()?;
        if prepared
            .authored_source
            .as_ref()
            .is_some_and(|bound| Arc::ptr_eq(bound, source))
        {
            return None;
        }
        Self::new(assets.binding_identity(), source.clone(), device)
    }

    /// Allocates unpublished resources for device-admitted maps and their reference table.
    pub(super) fn new(
        identity: ChunkTextureBindingIdentity,
        source: Arc<EnhancedTextureAssets>,
        device: &RenderDevice,
    ) -> Option<Self> {
        if !source.fits_device_limits(&device.limits()) {
            return None;
        }
        let arrays = source.pages();
        let schedule = Schedule::new(&arrays)?;
        let texture_bytes = arrays
            .iter()
            .flat_map(|array| array.mips.iter())
            .map(|mip| mip.rgba8.len() as u64)
            .sum();
        let textures = std::array::from_fn(|index| {
            let array = arrays[index];
            device.create_texture(&TextureDescriptor {
                label: Some("staged authored Enhanced terrain"),
                size: Extent3d {
                    width: array.mips[0].size,
                    height: array.mips[0].size,
                    depth_or_array_layers: array.layers,
                },
                mip_level_count: array.mips.len() as u32,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: if index < 2 {
                    TextureFormat::Rgba8UnormSrgb
                } else {
                    TextureFormat::Rgba8Unorm
                },
                usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                view_formats: &[],
            })
        });
        let views = std::array::from_fn(|index| {
            textures[index].create_view(&TextureViewDescriptor {
                label: Some("staged authored Enhanced terrain view"),
                dimension: Some(TextureViewDimension::D2Array),
                ..Default::default()
            })
        });
        Some(Self {
            identity,
            textures,
            views,
            references: device.create_buffer(&BufferDescriptor {
                label: Some("staged authored texture references"),
                size: std::mem::size_of_val(source.texture_refs.as_ref()) as u64,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            reference_words_written: 0,
            source,
            schedule,
            written: 0,
            texture_bytes,
        })
    }

    /// Checks whether the upload still belongs to the current asset generation.
    pub(super) fn matches(&self, identity: ChunkTextureBindingIdentity) -> bool {
        self.identity == identity
    }

    /// Streams aligned reference words and texture rows within the caller's byte budget.
    pub(super) fn step(&mut self, queue: &RenderQueue, budget: usize) -> usize {
        let mut remaining = budget;
        let word_bytes = std::mem::size_of::<u32>();
        let words = (self.source.texture_refs.len() - self.reference_words_written)
            .min(remaining / word_bytes);
        if words > 0 {
            let end = self.reference_words_written + words;
            let bytes =
                bytemuck::cast_slice(&self.source.texture_refs[self.reference_words_written..end]);
            queue.profiled_write_buffer(
                &self.references,
                (self.reference_words_written * word_bytes) as u64,
                bytes,
            );
            self.reference_words_written = end;
            remaining -= bytes.len();
        }
        let arrays = self.source.pages();
        while let Some(stripe) = self.schedule.next(remaining) {
            let mip = &arrays[stripe.page].mips[stripe.mip as usize];
            queue.profiled_write_texture(
                TexelCopyTextureInfo {
                    texture: &self.textures[stripe.page],
                    mip_level: stripe.mip,
                    origin: Origin3d {
                        x: 0,
                        y: stripe.row,
                        z: stripe.layer,
                    },
                    aspect: Default::default(),
                },
                &mip.rgba8[stripe.offset..stripe.offset + stripe.bytes],
                TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stripe.size * 4),
                    rows_per_image: Some(stripe.rows),
                },
                Extent3d {
                    width: stripe.size,
                    height: stripe.rows,
                    depth_or_array_layers: stripe.layer_count,
                },
            );
            remaining -= stripe.bytes;
        }
        let written = budget - remaining;
        self.written += written as u64;
        written
    }

    /// Reports whether all reference words and texture bytes have been submitted.
    pub(super) fn complete(&self) -> bool {
        self.reference_words_written == self.source.texture_refs.len() && self.schedule.complete()
    }

    /// Replaces authored resources together only after the upload is complete.
    pub(super) fn publish(
        self,
        prepared: &mut PreparedChunkTextureAssets,
        stats: &mut ChunkTextureUploadStats,
    ) {
        assert!(self.complete());
        replace_stats(prepared, stats, self.texture_bytes, self.written);
        prepared.authored_source = Some(self.source);
        prepared._enhanced_textures = self.textures;
        prepared.enhanced_views = self.views;
        prepared.enhanced_texture_refs = self.references;
        prepared.identity = self.identity;
    }
}

/// Replaces the authored contribution without recounting the retained carrier.
fn replace_stats(
    prepared: &mut PreparedChunkTextureAssets,
    stats: &mut ChunkTextureUploadStats,
    texture_bytes: u64,
    upload_bytes: u64,
) {
    stats.upload_count += 1;
    stats.texture_bytes_including_mips = stats
        .texture_bytes_including_mips
        .saturating_sub(prepared.authored_bytes)
        .saturating_add(texture_bytes);
    stats.queue_upload_bytes = stats
        .queue_upload_bytes
        .saturating_sub(prepared.authored_upload_bytes)
        .saturating_add(upload_bytes);
    prepared.authored_bytes = texture_bytes;
    prepared.authored_upload_bytes = upload_bytes;
}

/// Restores neutral authored resources while retaining the carrier atlas.
pub(super) fn detach(
    prepared: &mut PreparedChunkTextureAssets,
    identity: ChunkTextureBindingIdentity,
    device: &RenderDevice,
    stats: &mut ChunkTextureUploadStats,
) {
    let references = vec![u32::MAX; EnhancedTextureAssets::REFERENCE_BASE_WORDS];
    let bytes = bytemuck::cast_slice(&references);
    let buffer = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("carrier-only authored texture references"),
        contents: bytes,
        usage: BufferUsages::STORAGE,
    });
    replace_stats(prepared, stats, 0, bytes.len() as u64);
    prepared.authored_source = None;
    prepared._enhanced_textures = [
        prepared._textures[0].clone(),
        prepared._textures[1].clone(),
        prepared._pbr_textures[0].clone(),
        prepared._pbr_textures[1].clone(),
        prepared._pbr_textures[2].clone(),
        prepared._pbr_textures[3].clone(),
    ];
    prepared.enhanced_views = [
        prepared.views[0].clone(),
        prepared.views[1].clone(),
        prepared.pbr_views[0].clone(),
        prepared.pbr_views[1].clone(),
        prepared.pbr_views[2].clone(),
        prepared.pbr_views[3].clone(),
    ];
    prepared.enhanced_texture_refs = buffer;
    prepared.identity = identity;
}

#[cfg(test)]
mod tests;
