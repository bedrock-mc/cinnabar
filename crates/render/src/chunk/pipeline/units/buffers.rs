use crate::queue_writes::ProfiledQueue as _;
use render_model::terrain_units::{VisibleTerrainUnit, shards};

use crate::chunk::*;
use crate::upload_staging::TerrainUploadStaging;

/// One binding-sized page of a stream's ordered visible units.
pub(super) struct UnitPage {
    buffer: Buffer,
    pub(super) group: BindGroup,
    pub(super) count: u32,
    uploaded: Vec<VisibleTerrainUnit>,
}

/// Uploads only changed pages through the shared staging pool. Pages never truncate a stream.
pub(super) fn upload(
    pages: &mut Vec<UnitPage>,
    visible: &[VisibleTerrainUnit],
    device: &RenderDevice,
    queue: &RenderQueue,
    cache: &PipelineCache,
    staging: Option<&TerrainUploadStaging>,
) -> u64 {
    let limits = device.limits();
    let max_bytes = limits
        .max_buffer_size
        .min(limits.max_storage_buffer_binding_size);
    let mut bytes_uploaded = 0;
    let mut page_count = 0;
    for (index, range) in shards(visible.len(), max_bytes).enumerate() {
        page_count += 1;
        let units = &visible[range];
        let bytes = bytemuck::cast_slice(units);
        if pages
            .get(index)
            .is_none_or(|page| page.buffer.size() < bytes.len() as u64)
        {
            let buffer = device.create_buffer(&BufferDescriptor {
                label: Some("visible terrain unit page"),
                size: (bytes.len() as u64).next_power_of_two().min(max_bytes),
                usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let group = device.create_bind_group(
                "visible terrain unit page",
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
                uploaded: Vec::new(),
            };
            if index == pages.len() {
                pages.push(page);
            } else {
                pages[index] = page;
            }
        }
        let page = &mut pages[index];
        page.count = units.len() as u32;
        if page.uploaded != units {
            #[cfg(feature = "tracy")]
            let _span = bevy::log::info_span!("terrain.unit_upload", bytes = bytes.len()).entered();
            if let Some(staging) = staging {
                staging.write_batch(device, queue, &[(&page.buffer, 0, bytes)]);
            } else {
                queue.profiled_write_buffer(&page.buffer, 0, bytes);
            }
            page.uploaded.clear();
            page.uploaded.extend_from_slice(units);
            bytes_uploaded += bytes.len() as u64;
        }
    }
    pages.truncate(page_count);
    bytes_uploaded
}
