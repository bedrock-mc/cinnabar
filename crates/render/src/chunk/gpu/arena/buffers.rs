//! Buffer descriptors and checked growth allocations for terrain arena streams.

use crate::chunk::*;

/// Vertex buffer 0 for CPU-planned terrain draws over `origin_capacity` origin slots: entry `i`
/// is `(i, 0)`, so fetching at a draw's base vertex yields its vertex index while the builtin
/// instance index keeps its first instance. GPU-culled draws bind the cull's draw offsets instead.
/// `None` when the device is out of memory for it.
pub(in crate::chunk) fn identity_vertex_offsets(
    render_device: &RenderDevice,
    origin_capacity: usize,
    requirement: crate::gpu_allocation::Requirement,
) -> Option<Buffer> {
    let words = identity_vertex_offset_words(origin_capacity);
    requirement.checked(
        render_device,
        "terrain identity vertex offsets",
        std::mem::size_of_val(words.as_slice()) as u64,
        || create_identity_vertex_offsets(render_device, &words),
    )
}

/// Generates the identity vertex and instance offsets for every origin corner.
pub(super) fn identity_vertex_offset_words(origin_capacity: usize) -> Vec<u32> {
    let corners = crate::chunk::gpu_cull::model::OFFSET_CORNERS;
    (0..origin_capacity as u32 * corners)
        .flat_map(|vertex| [vertex, 0])
        .collect()
}

/// Uploads immutable identity offsets used by CPU-planned terrain commands.
pub(super) fn create_identity_vertex_offsets(
    render_device: &RenderDevice,
    words: &[u32],
) -> Buffer {
    render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("terrain identity vertex offsets"),
        contents: bytemuck::cast_slice(words),
        usage: BufferUsages::VERTEX,
    })
}

/// Describes storage shared by arena uploads, shader reads, and migration copies.
fn storage_buffer_descriptor(label: &'static str, size: u64) -> BufferDescriptor<'static> {
    BufferDescriptor {
        label: Some(label),
        size,
        usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    }
}

/// Creates the initial storage for one terrain stream.
pub(in crate::chunk) fn create_storage_buffer(
    render_device: &RenderDevice,
    label: &'static str,
    size: u64,
) -> Buffer {
    render_device.create_buffer(&storage_buffer_descriptor(label, size))
}

/// [`create_storage_buffer`], or `None` when the device is out of memory.
pub(in crate::chunk) fn try_create_storage_buffer(
    render_device: &RenderDevice,
    label: &'static str,
    size: u64,
    requirement: crate::gpu_allocation::Requirement,
) -> Option<Buffer> {
    requirement.checked(render_device, label, size, || {
        create_storage_buffer(render_device, label, size)
    })
}

/// `None` when the device is out of memory for it.
pub(in crate::chunk) fn create_indirect_buffer(
    render_device: &RenderDevice,
    command_capacity: usize,
) -> Option<Buffer> {
    crate::gpu_allocation::terrain_buffer(
        render_device,
        &indirect_buffer_descriptor(command_capacity),
    )
}

/// Describes the shared indexed indirect-command buffer at its requested capacity.
pub(super) fn indirect_buffer_descriptor(command_capacity: usize) -> BufferDescriptor<'static> {
    BufferDescriptor {
        label: Some("packed chunk indirect commands"),
        size: command_capacity as u64 * INDEXED_INDIRECT_BYTES,
        usage: BufferUsages::INDIRECT
            | BufferUsages::COPY_DST
            | if cfg!(test) {
                BufferUsages::COPY_SRC
            } else {
                BufferUsages::empty()
            },
        mapped_at_creation: false,
    }
}
