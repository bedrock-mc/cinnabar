//! The buffers a frame's expansion dispatches read: the material attribute table followed by
//! every dispatch's jobs in one storage buffer, and each dispatch's `ExpandParams` in its own
//! slot of one uniform buffer. Both persist and grow, so a frame of expansion writes its jobs
//! and parameters into them instead of creating two buffers per dispatch, each job buffer
//! holding its own copy of the attribute table.

use std::num::NonZeroU64;

use bevy::render::render_resource::{
    BindingResource, Buffer, BufferBinding, BufferDescriptor, BufferUsages,
};
use bevy::render::renderer::{RenderDevice, RenderQueue};

use super::pipelines::EXPAND_PARAMS_BYTES;

/// Words of one `ExpandParams`.
pub(super) const PARAM_WORDS: usize = (EXPAND_PARAMS_BYTES / 4) as usize;
/// `ExpandParams` word of the dispatch's first job word (`flags.z`).
const FIRST_JOB_WORD: usize = 6;
/// `ExpandParams` word of the dispatch's job count (`jobs.x`).
const JOB_COUNT_WORD: usize = 12;

#[derive(Default)]
pub(super) struct ExpansionBuffers {
    jobs: Option<Buffer>,
    /// The attribute table at the start of `jobs`, as written.
    attributes: Vec<u32>,
    params: Option<Buffer>,
    /// Bytes between consecutive dispatches' parameters.
    params_stride: u64,
}

impl ExpansionBuffers {
    /// Writes `attributes` (when they changed or the buffer grew) and then the jobs of each
    /// of `dispatches`, with one parameter slot per dispatch: `params` with that dispatch's
    /// first job word and job count.
    pub(super) fn write(
        &mut self,
        device: &RenderDevice,
        queue: &RenderQueue,
        attributes: &[u32],
        params: [u32; PARAM_WORDS],
        dispatches: &[&[[u32; 4]]],
    ) {
        let jobs: usize = dispatches.iter().map(|jobs| jobs.len()).sum();
        let job_bytes = ((attributes.len() + jobs * 4) * 4).max(4) as u64;
        if self
            .jobs
            .as_ref()
            .is_none_or(|buffer| buffer.size() < job_bytes)
        {
            self.jobs = Some(
                device.create_buffer(&BufferDescriptor {
                    label: Some("shader-pack expansion jobs"),
                    size: job_bytes
                        .next_power_of_two()
                        .min(device.limits().max_storage_buffer_binding_size)
                        .min(device.limits().max_buffer_size),
                    usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
            );
            self.attributes.clear();
        }
        let buffer = self.jobs.as_ref().expect("created above");
        if self.attributes != attributes {
            queue.write_buffer(buffer, 0, bytemuck::cast_slice(attributes));
            self.attributes = attributes.to_vec();
        }
        let mut words: Vec<u32> = Vec::with_capacity(jobs * 4);
        let mut slots = Vec::with_capacity(dispatches.len() * PARAM_WORDS);
        let alignment = u64::from(device.limits().min_uniform_buffer_offset_alignment);
        self.params_stride = EXPAND_PARAMS_BYTES.next_multiple_of(alignment);
        let slot_words = (self.params_stride / 4) as usize;
        for jobs in dispatches {
            let mut slot = params;
            slot[FIRST_JOB_WORD] = (attributes.len() + words.len()) as u32;
            slot[JOB_COUNT_WORD] = jobs.len() as u32;
            words.extend(jobs.iter().flatten());
            slots.extend(slot);
            slots.resize(slots.len() + slot_words - PARAM_WORDS, 0);
        }
        if !words.is_empty() {
            queue.write_buffer(
                buffer,
                (attributes.len() * 4) as u64,
                bytemuck::cast_slice(&words),
            );
        }
        let params_bytes = (slots.len() * 4).max(4) as u64;
        if self
            .params
            .as_ref()
            .is_none_or(|buffer| buffer.size() < params_bytes)
        {
            self.params = Some(
                device.create_buffer(&BufferDescriptor {
                    label: Some("shader-pack expansion parameters"),
                    size: params_bytes
                        .next_power_of_two()
                        .min(device.limits().max_buffer_size),
                    usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
            );
        }
        let params = self.params.as_ref().expect("created above");
        queue.write_buffer(params, 0, bytemuck::cast_slice(&slots));
    }

    /// The attribute table and jobs every dispatch binds; `None` before the first
    /// [`Self::write`].
    pub(super) fn jobs(&self) -> Option<&Buffer> {
        self.jobs.as_ref()
    }

    /// The parameters of dispatch `index` of the last [`Self::write`].
    pub(super) fn params(&self, index: usize) -> Option<BindingResource<'_>> {
        Some(BindingResource::Buffer(BufferBinding {
            buffer: self.params.as_ref()?,
            offset: index as u64 * self.params_stride,
            size: NonZeroU64::new(EXPAND_PARAMS_BYTES),
        }))
    }
}

/// Jobs that fit beside the attributes and one worst-case parameter slot per job.
pub(super) fn frame_job_capacity(limits: &wgpu::Limits, attribute_words: usize) -> usize {
    let storage = limits
        .max_storage_buffer_binding_size
        .min(limits.max_buffer_size);
    let jobs = storage.saturating_sub(attribute_words as u64 * 4) / 16;
    let stride =
        EXPAND_PARAMS_BYTES.next_multiple_of(u64::from(limits.min_uniform_buffer_offset_alignment));
    jobs.min(limits.max_buffer_size / stride) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_jobs_leave_room_for_attributes_and_aligned_parameters() {
        let mut limits = wgpu::Limits::default();
        limits.max_storage_buffer_binding_size = 100;
        limits.max_buffer_size = 2048;
        assert_eq!(frame_job_capacity(&limits, 5), 5);
        limits.max_buffer_size = 512;
        assert_eq!(frame_job_capacity(&limits, 5), 2);
        assert_eq!(frame_job_capacity(&limits, 25), 0);
    }
}
