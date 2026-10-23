use crate::queue_writes::ProfiledQueue as _;
use render_model::terrain_units::{IndexedUnitPage, VisibleTerrainUnit};

use super::UnitStream;
use crate::chunk::*;
use crate::upload_staging::TerrainUploadStaging;

/// One binding contains both stable geometry slots and the ordered visibility references.
pub(super) struct UnitPage {
    pub(super) buffer: Buffer,
    pub(super) group: BindGroup,
    pub(super) count: u32,
    pub(super) masks: Option<gpu_cull::unit_mask::UnitMaskBindings>,
    layout: IndexedUnitPage,
    indexed: bool,
    packed: Vec<VisibleTerrainUnit>,
    references: Vec<u32>,
}

impl UnitPage {
    /// Forces a complete descriptor and reference upload while keeping this storage.
    pub(super) fn invalidate(&mut self) {
        self.indexed = false;
        self.packed.clear();
    }
}

#[derive(Default)]
pub(super) struct Uploaded {
    pub(super) descriptors: u64,
    pub(super) references: u64,
}

/// Uploads changed slot spans; oversized streams retain complete ordered page draws.
pub(super) fn upload(
    stream: &mut UnitStream,
    capacity: usize,
    device: &RenderDevice,
    queue: &RenderQueue,
    cache: &PipelineCache,
    staging: Option<&TerrainUploadStaging>,
) -> Result<Uploaded, crate::gpu_allocation::TerrainWriteError> {
    let mut uploaded = Uploaded::default();
    if stream.indexed {
        if stream.descriptors.records.is_empty() {
            stream.pages.clear();
            return Ok(uploaded);
        }
        stream.pages.truncate(1);
        let Some(fresh) = ensure_page(
            &mut stream.pages,
            0,
            stream.descriptors.records.len(),
            capacity,
            device,
            cache,
        ) else {
            return Err(crate::gpu_allocation::TerrainWriteError);
        };
        let page = &mut stream.pages[0];
        let all = fresh || !page.indexed;
        page.indexed = true;
        page.packed.clear();
        upload_page(
            page,
            &stream.descriptors.records,
            &stream.descriptors.dirty,
            &stream.references,
            all,
            device,
            queue,
            staging,
            &mut uploaded,
        )?;
    } else {
        let mut pages = 0;
        for (index, descriptors) in stream.fallback.chunks(capacity).enumerate() {
            pages += 1;
            let Some(fresh) = ensure_page(
                &mut stream.pages,
                index,
                descriptors.len(),
                capacity,
                device,
                cache,
            ) else {
                return Err(crate::gpu_allocation::TerrainWriteError);
            };
            let page = &mut stream.pages[index];
            let all = fresh || page.indexed || page.packed != descriptors;
            page.indexed = false;
            let references = (0..descriptors.len() as u32).collect::<Vec<_>>();
            upload_page(
                page,
                descriptors,
                &[],
                &references,
                all,
                device,
                queue,
                staging,
                &mut uploaded,
            )?;
            if all {
                page.packed.clear();
                page.packed.extend_from_slice(descriptors);
            }
        }
        stream.pages.truncate(pages);
    }
    Ok(uploaded)
}

/// Grows geometrically without exceeding either the buffer or storage-binding limit.
fn ensure_page(
    pages: &mut Vec<UnitPage>,
    index: usize,
    needed: usize,
    maximum: usize,
    device: &RenderDevice,
    cache: &PipelineCache,
) -> Option<bool> {
    if pages
        .get(index)
        .is_some_and(|page| page.layout.capacity >= needed)
    {
        return Some(false);
    }
    let layout = IndexedUnitPage {
        capacity: needed.next_power_of_two().min(maximum),
    };
    let buffer = crate::gpu_allocation::terrain_buffer(
        device,
        &BufferDescriptor {
            label: Some("indexed terrain unit page"),
            size: layout.byte_size(),
            usage: BufferUsages::STORAGE
                | BufferUsages::COPY_DST
                | if cfg!(test) {
                    BufferUsages::COPY_SRC
                } else {
                    BufferUsages::empty()
                },
            mapped_at_creation: false,
        },
    )?;
    let group = device.create_bind_group(
        "indexed terrain unit page",
        &cache.get_bind_group_layout(&super::bind_group_layout()),
        &[BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    );
    let page = UnitPage {
        buffer,
        group,
        count: 0,
        masks: None,
        layout,
        indexed: false,
        packed: Vec::new(),
        references: Vec::new(),
    };
    if index == pages.len() {
        pages.push(page);
    } else {
        pages[index] = page;
    }
    Some(true)
}

/// Coalesces adjacent changed slots, without copying any intervening unchanged descriptor.
pub(super) fn dirty_ranges(dirty: &[u32]) -> impl Iterator<Item = Range<usize>> + '_ {
    let mut remaining = dirty;
    std::iter::from_fn(move || {
        let (&start, rest) = remaining.split_first()?;
        let mut end = start + 1;
        let mut used = 0;
        for &slot in rest {
            if slot != end {
                break;
            }
            end += 1;
            used += 1;
        }
        remaining = &rest[used..];
        Some(start as usize..end as usize)
    })
}

/// Keeps all writes in the existing ordered staging pool before any camera uses the page.
#[allow(clippy::too_many_arguments)]
fn upload_page(
    page: &mut UnitPage,
    descriptors: &[VisibleTerrainUnit],
    dirty: &[u32],
    references: &[u32],
    all: bool,
    device: &RenderDevice,
    queue: &RenderQueue,
    staging: Option<&TerrainUploadStaging>,
    uploaded: &mut Uploaded,
) -> Result<(), crate::gpu_allocation::TerrainWriteError> {
    let count = references.len() as u32;
    let header = page.layout.header(count);
    let record_bytes = size_of::<VisibleTerrainUnit>() as u64;
    let mut writes: Vec<crate::upload_staging::BufferWrite<'_>> = Vec::new();
    if all || page.count != count {
        writes.push((&page.buffer, 0, bytemuck::bytes_of(&header)));
        uploaded.descriptors += record_bytes;
    }
    if all {
        writes.push((
            &page.buffer,
            record_bytes,
            bytemuck::cast_slice(descriptors),
        ));
        uploaded.descriptors += record_bytes * descriptors.len() as u64;
    } else {
        for range in dirty_ranges(dirty) {
            uploaded.descriptors += range.len() as u64 * record_bytes;
            writes.push((
                &page.buffer,
                (1 + range.start as u64) * record_bytes,
                bytemuck::cast_slice(&descriptors[range]),
            ));
        }
    }
    let changed = all || page.references != references;
    if changed && !references.is_empty() {
        let bytes = bytemuck::cast_slice(references);
        writes.push((&page.buffer, page.layout.reference_offset(), bytes));
        uploaded.references += bytes.len() as u64;
    }
    TerrainUploads {
        device,
        queue,
        staging,
    }
    .write(&writes, |rest, site| {
        for &(buffer, offset, bytes) in rest {
            queue.profiled_write_buffer_at(buffer, offset, bytes, site);
        }
    })?;
    if changed {
        page.references.clear();
        page.references.extend_from_slice(references);
    }
    page.count = count;
    Ok(())
}
