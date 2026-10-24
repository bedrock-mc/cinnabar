//! Device-bounded storage for indirect culling and direct-draw occlusion.

use super::model::{
    CullRecord, CullViewUniform, OFFSET_ENTRY_BYTES, PHASE_COUNT, STREAM_COUNT, args_words,
    cull_workgroup, draw_offset_bytes, group_count,
};
use crate::queue_writes::ProfiledQueue as _;

const RECORD_BYTES: u64 = size_of::<CullRecord>() as u64;
const MIN_CAPACITY: u32 = 1024;

/// Describes an unmapped buffer, including the minimum size needed by shader bindings.
fn buffer(
    label: &'static str,
    size: u64,
    usage: wgpu::BufferUsages,
) -> wgpu::BufferDescriptor<'static> {
    wgpu::BufferDescriptor {
        label: Some(label),
        size: size.max(16),
        usage,
        mapped_at_creation: false,
    }
}

/// Whether every entire-buffer binding and dispatch fits this device.
fn fits(
    limits: &wgpu::Limits,
    workgroup: u32,
    capacity: u32,
    buffers: &[wgpu::BufferDescriptor<'_>],
) -> bool {
    group_count(capacity, workgroup) <= limits.max_compute_workgroups_per_dimension
        && buffers.iter().all(|buffer| {
            buffer.size <= limits.max_buffer_size
                && (!buffer.usage.contains(wgpu::BufferUsages::STORAGE)
                    || buffer.size <= limits.max_storage_buffer_binding_size)
        })
}

/// Rounds growth up when possible, clamps it to the last fitting slot, and refuses truncation.
fn growth_capacity<const N: usize>(
    limits: &wgpu::Limits,
    slots: u32,
    descriptors: impl Fn(u32) -> [wgpu::BufferDescriptor<'static>; N],
) -> Option<u32> {
    let workgroup = cull_workgroup(limits)?;
    // Signed base-vertex offsets address four corners of every command.
    let command_slots = (i32::MAX as u64 + 1) / (draw_offset_bytes(1) / OFFSET_ENTRY_BYTES);
    let mut low = 0;
    let mut high = command_slots.min(u64::from(u32::MAX)) as u32;
    while low < high {
        let middle = low + (high - low).div_ceil(2);
        if fits(limits, workgroup, middle, &descriptors(middle)) {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    let capacity = slots
        .max(MIN_CAPACITY)
        .checked_next_power_of_two()
        .unwrap_or(u32::MAX)
        .min(low);
    (capacity >= slots && capacity != 0).then_some(capacity)
}

/// GPU buffers for `capacity` record slots; `readback` adds copy-out for tests.
pub struct CullStorage {
    pub capacity: u32,
    pub records: wgpu::Buffer,
    pub enabled: wgpu::Buffer,
    pub history: wgpu::Buffer,
    pub record_draws: wgpu::Buffer,
    pub group_sums: wgpu::Buffer,
    pub args: wgpu::Buffer,
    pub draw_counts: wgpu::Buffer,
    /// Per-command vertex entries; see [`super::model::resolve_culled_args`].
    pub draw_offsets: wgpu::Buffer,
    pub uniforms: [wgpu::Buffer; PHASE_COUNT],
}

impl CullStorage {
    /// Describes every allocation once for both creation and limit checks.
    fn descriptors(
        capacity: u32,
        workgroup: u32,
        readback: bool,
    ) -> [wgpu::BufferDescriptor<'static>; 10] {
        use wgpu::BufferUsages as U;
        let copy_out = if readback { U::COPY_SRC } else { U::empty() };
        let slots = u64::from(capacity);
        let uniform = |label| {
            buffer(
                label,
                size_of::<CullViewUniform>() as u64,
                U::UNIFORM | U::COPY_DST,
            )
        };
        [
            buffer(
                "terrain cull records",
                slots * RECORD_BYTES,
                U::STORAGE | U::COPY_DST,
            ),
            buffer(
                "terrain cull enabled bits",
                slots.div_ceil(32) * 4,
                U::STORAGE | U::COPY_DST,
            ),
            buffer(
                "terrain cull visibility history",
                slots * 4,
                U::STORAGE | U::COPY_DST | copy_out,
            ),
            buffer(
                "terrain cull record draws",
                slots * 4,
                U::STORAGE | U::COPY_SRC,
            ),
            buffer(
                "terrain cull group sums",
                u64::from(group_count(capacity, workgroup).max(1)) * 16,
                U::STORAGE,
            ),
            buffer(
                "terrain cull indirect args",
                args_words(capacity) * 4,
                U::STORAGE | U::INDIRECT | U::COPY_DST | copy_out,
            ),
            buffer(
                "terrain cull draw counts",
                (PHASE_COUNT * STREAM_COUNT * 4) as u64,
                U::STORAGE | U::INDIRECT | copy_out,
            ),
            buffer(
                "terrain cull draw offsets",
                draw_offset_bytes(capacity),
                U::STORAGE | U::VERTEX | copy_out,
            ),
            uniform("terrain cull early view"),
            uniform("terrain cull late view"),
        ]
    }

    /// Returns growth that fits all buffers without dropping any requested slots.
    pub fn capacity(limits: &wgpu::Limits, slots: u32) -> Option<u32> {
        let workgroup = cull_workgroup(limits)?;
        growth_capacity(limits, slots, |capacity| {
            Self::descriptors(capacity, workgroup, false)
        })
    }

    /// Allocates an already bounded table; production callers use a recoverable error scope.
    pub fn new(device: &wgpu::Device, capacity: u32, readback: bool) -> Self {
        let workgroup = cull_workgroup(&device.limits()).expect("compatible cull limits");
        let [
            records,
            enabled,
            history,
            record_draws,
            group_sums,
            args,
            draw_counts,
            draw_offsets,
            early,
            late,
        ] = Self::descriptors(capacity, workgroup, readback)
            .map(|descriptor| device.create_buffer(&descriptor));
        Self {
            capacity,
            records,
            enabled,
            history,
            record_draws,
            group_sums,
            args,
            draw_counts,
            draw_offsets,
            uniforms: [early, late],
        }
    }

    /// Updates one phase's view after its allocation has succeeded.
    pub fn write_uniform(
        &self,
        queue: &wgpu::Queue,
        phase: super::model::CullPhase,
        uniform: &CullViewUniform,
    ) {
        queue.profiled_write_buffer(
            &self.uniforms[phase as usize],
            0,
            bytemuck::bytes_of(uniform),
        );
    }
}

/// Buffers for the occluded bit of `capacity` record slots.
pub struct OcclusionStorage {
    pub capacity: u32,
    pub records: wgpu::Buffer,
    pub occluded: wgpu::Buffer,
    pub uniform: wgpu::Buffer,
}

impl OcclusionStorage {
    /// Describes every allocation once for both creation and limit checks.
    fn descriptors(capacity: u32) -> [wgpu::BufferDescriptor<'static>; 3] {
        use wgpu::BufferUsages as U;
        [
            buffer(
                "terrain occlusion records",
                u64::from(capacity) * RECORD_BYTES,
                U::STORAGE | U::COPY_DST,
            ),
            buffer(
                "terrain occlusion bits",
                occlusion_bytes(capacity),
                U::STORAGE | U::COPY_SRC,
            ),
            buffer(
                "terrain occlusion view",
                size_of::<CullViewUniform>() as u64,
                U::UNIFORM | U::COPY_DST,
            ),
        ]
    }

    /// Returns growth that fits storage and its smaller readback buffers.
    pub fn capacity(limits: &wgpu::Limits, slots: u32) -> Option<u32> {
        growth_capacity(limits, slots, Self::descriptors)
    }

    /// Allocates an already bounded table; production callers use a recoverable error scope.
    pub fn new(device: &wgpu::Device, capacity: u32) -> Self {
        let [records, occluded, uniform] =
            Self::descriptors(capacity).map(|descriptor| device.create_buffer(&descriptor));
        Self {
            capacity,
            records,
            occluded,
            uniform,
        }
    }
}

/// Bytes of the occluded bitset for `capacity` slots, including the minimum binding size.
pub fn occlusion_bytes(capacity: u32) -> u64 {
    (u64::from(capacity.div_ceil(32)) * 4).max(16)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Growth can use a non-power-of-two ceiling but never an undersized table.
    #[test]
    fn growth_clamps_to_buffer_and_binding_limits_without_truncating_slots() {
        let slots = 1301;
        for binding in [false, true] {
            let bytes = draw_offset_bytes(slots);
            let limits = if binding {
                wgpu::Limits {
                    max_storage_buffer_binding_size: bytes,
                    ..Default::default()
                }
            } else {
                wgpu::Limits {
                    max_buffer_size: bytes,
                    ..Default::default()
                }
            };
            let capacity = CullStorage::capacity(&limits, slots).unwrap();
            assert_eq!(capacity, slots);
            assert_eq!(CullStorage::capacity(&limits, slots + 1), None);
            let (device, _) = wgpu::Device::noop(&wgpu::DeviceDescriptor {
                required_limits: limits,
                ..Default::default()
            });
            let storage = CullStorage::new(&device, capacity, false);
            assert_eq!(storage.draw_offsets.size(), bytes);
        }
    }

    /// Direct occlusion obeys the same bounds even though its records are smaller.
    #[test]
    fn occlusion_growth_clamps_both_limits_and_dispatch_count() {
        let slots = 1301;
        for binding in [false, true] {
            let bytes = u64::from(slots) * RECORD_BYTES;
            let limits = if binding {
                wgpu::Limits {
                    max_storage_buffer_binding_size: bytes,
                    ..Default::default()
                }
            } else {
                wgpu::Limits {
                    max_buffer_size: bytes,
                    ..Default::default()
                }
            };
            assert_eq!(OcclusionStorage::capacity(&limits, slots), Some(slots));
            assert_eq!(OcclusionStorage::capacity(&limits, slots + 1), None);
        }
        let limits = wgpu::Limits {
            max_compute_workgroups_per_dimension: 2,
            ..Default::default()
        };
        let slots = 2 * cull_workgroup(&limits).unwrap();
        assert_eq!(CullStorage::capacity(&limits, slots), Some(slots));
        assert_eq!(CullStorage::capacity(&limits, slots + 1), None);
        assert_eq!(OcclusionStorage::capacity(&limits, u32::MAX), None);
    }
}
