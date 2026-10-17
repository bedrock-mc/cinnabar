//! Retained actor history storage avoids staging identical bytes on repeated frames.

use crate::gpu_allocation;
use bevy::render::{render_resource::*, renderer::RenderDevice};

pub(super) struct MotionBuffer {
    pub buffer: Buffer,
    contents: Vec<u8>,
}

impl MotionBuffer {
    /// Creates a nonempty binding even before the first actor history is available.
    pub fn new(device: &RenderDevice, bytes: usize) -> Option<Self> {
        Some(Self {
            buffer: allocate(device, bytes)?,
            contents: Vec::new(),
        })
    }

    /// Writes changed contents and reports whether callers must rebuild the buffer binding.
    pub fn upload(
        &mut self,
        device: &RenderDevice,
        bytes: &[u8],
        write: impl FnOnce(&Buffer, &[u8]),
    ) -> Option<bool> {
        if self.contents.get(..bytes.len()) == Some(bytes) {
            return Some(false);
        }
        let resized = self.buffer.size() < bytes.len() as u64;
        let replacement = if resized {
            Some(allocate(device, bytes.len())?)
        } else {
            None
        };
        let buffer = replacement.as_ref().unwrap_or(&self.buffer);
        if gpu_allocation::checked(device, "actor velocity upload", bytes.len() as u64, || {
            write(buffer, bytes)
        })
        .is_none()
        {
            self.contents.clear();
            return None;
        }
        if let Some(buffer) = replacement {
            self.buffer = buffer;
        }
        // Shorter inputs leave the unused GPU tail intact, so retain its shadow too.
        self.contents
            .resize(self.contents.len().max(bytes.len()), 0);
        self.contents[..bytes.len()].copy_from_slice(bytes);
        Some(resized)
    }
}

/// Allocates storage owned by one view, retaining capacity when subsequent inputs shrink.
fn allocate(device: &RenderDevice, bytes: usize) -> Option<Buffer> {
    if bytes as u64 > crate::actor::gpu::storage_binding_bytes(device) {
        return None;
    }
    gpu_allocation::buffer(
        device,
        &BufferDescriptor {
            label: Some("actor velocity history"),
            size: capacity(
                bytes as u64,
                crate::actor::gpu::storage_binding_bytes(device),
            ),
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        },
    )
}

/// Rounds storage up geometrically without exceeding the device's whole-binding limit.
fn capacity(bytes: u64, limit: u64) -> u64 {
    let bytes = bytes.max(1);
    assert!(
        bytes <= limit,
        "actor motion history exceeds the storage binding limit"
    );
    bytes
        .checked_next_power_of_two()
        .unwrap_or(limit)
        .min(limit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocation_failure_keeps_motion_buffer_and_skips_its_write() {
        let (device, _) = wgpu::Device::noop(&Default::default());
        let device = RenderDevice::from(device);
        let mut buffer = MotionBuffer::new(&device, 16).unwrap();
        let previous = buffer.buffer.id();
        let _failure = gpu_allocation::inject(|label, _| label == "actor velocity history");
        assert!(
            buffer
                .upload(&device, &[1; 64], |_, _| panic!(
                    "failed buffer was written"
                ))
                .is_none()
        );
        assert_eq!(buffer.buffer.id(), previous);
        assert!(buffer.contents.is_empty());
    }

    #[test]
    fn growing_crowds_create_logarithmically_many_buffers_without_extra_uploads() {
        let (device, _) = wgpu::Device::noop(&Default::default());
        let device = RenderDevice::from(device);
        let actors = 256;
        for stride in [
            size_of::<crate::motion_blur::actor_history::MotionInstance>(),
            render_model::ACTOR_BONE_MATRIX_BYTES,
        ] {
            let mut buffer = MotionBuffer::new(&device, stride).unwrap();
            let mut creations = 1;
            let mut allocated = buffer.buffer.size();
            let mut uploaded = 0;
            for count in 1..=actors {
                let bytes = vec![1; count * stride];
                let identity = buffer.buffer.id();
                let resized = buffer
                    .upload(&device, &bytes, |target, source| {
                        assert!(target.size() >= source.len() as u64);
                        assert_eq!(source, bytes);
                        uploaded += source.len();
                    })
                    .unwrap();
                assert_eq!(resized, identity != buffer.buffer.id());
                if resized {
                    creations += 1;
                    allocated += buffer.buffer.size();
                }
            }
            let exact_size_bytes = stride * actors * (actors + 1) / 2;
            assert!(
                creations <= 10,
                "created {creations} buffers for {actors} actors"
            );
            assert!(allocated < exact_size_bytes as u64 / 16);
            assert_eq!(uploaded, exact_size_bytes);
        }
    }

    #[test]
    fn motion_transform_bytes_keep_their_order_through_growth_and_empty_inputs() {
        use crate::motion_blur::actor_history::MotionInstance;
        use bevy::math::{Mat4, Vec3};

        let (device, _) = wgpu::Device::noop(&Default::default());
        let device = RenderDevice::from(device);
        let mut instances = MotionBuffer::new(&device, size_of::<MotionInstance>()).unwrap();
        let mut bones = MotionBuffer::new(&device, render_model::ACTOR_BONE_MATRIX_BYTES).unwrap();
        let mut instance_gpu = vec![0; instances.buffer.size() as usize];
        let mut bone_gpu = vec![0; bones.buffer.size() as usize];
        for (frame, count) in [1, 2, 3, 4, 2, 0, 3, 8, 1, 0].into_iter().enumerate() {
            let records = (0..count)
                .rev()
                .map(|actor| MotionInstance {
                    current_clip_from_actor: Mat4::from_translation(Vec3::new(
                        actor as f32,
                        frame as f32,
                        1.0,
                    )),
                    previous_clip_from_actor: Mat4::from_translation(Vec3::new(
                        actor as f32,
                        frame as f32 - 1.0,
                        2.0,
                    )),
                    pose: [actor, 1, 0, 0],
                })
                .collect::<Vec<_>>();
            let matrices = (0..count)
                .map(|bone| [[bone as f32, frame as f32, 3.0, 4.0]; 3])
                .collect::<Vec<_>>();
            for (buffer, bytes, gpu) in [
                (
                    &mut instances,
                    bytemuck::cast_slice(&records),
                    &mut instance_gpu,
                ),
                (&mut bones, bytemuck::cast_slice(&matrices), &mut bone_gpu),
            ] {
                let previous = buffer.buffer.clone();
                let previous_size = previous.size();
                let resized = buffer
                    .upload(&device, bytes, |target, source| {
                        if target.id() != previous.id() {
                            *gpu = vec![0; target.size() as usize];
                        }
                        gpu[..source.len()].copy_from_slice(source);
                    })
                    .unwrap();
                assert_eq!(resized, buffer.buffer.id() != previous.id());
                assert_eq!(previous.size(), previous_size);
                assert_eq!(&gpu[..bytes.len()], bytes);
                assert!(
                    !buffer
                        .upload(&device, bytes, |_, _| panic!("repeated upload"))
                        .unwrap()
                );
            }
        }
    }

    #[test]
    fn geometric_growth_respects_both_device_limits_without_truncating_payloads() {
        for (buffer_limit, binding_limit) in [(1024, 768), (768, 1024)] {
            let (device, _) = wgpu::Device::noop(&wgpu::DeviceDescriptor {
                required_limits: wgpu::Limits {
                    max_buffer_size: buffer_limit,
                    max_storage_buffer_binding_size: binding_limit,
                    ..Default::default()
                },
                ..Default::default()
            });
            let device = RenderDevice::from(device);
            let mut buffer = MotionBuffer::new(&device, 512).unwrap();
            let bytes = vec![1; 576];
            assert!(
                buffer
                    .upload(&device, &bytes, |_, source| assert_eq!(source, bytes))
                    .unwrap()
            );
            assert_eq!(buffer.buffer.size(), buffer_limit.min(binding_limit));
            let identity = buffer.buffer.id();
            let bytes = vec![2; buffer.buffer.size() as usize];
            assert!(
                !buffer
                    .upload(&device, &bytes, |_, source| assert_eq!(source, bytes))
                    .unwrap()
            );
            assert_eq!(buffer.buffer.id(), identity);
        }
    }

    #[test]
    fn empty_motion_storage_is_nonempty_and_does_not_upload() {
        let (device, _) = wgpu::Device::noop(&Default::default());
        let device = RenderDevice::from(device);
        let mut buffer = MotionBuffer::new(&device, 0).unwrap();
        assert!(buffer.buffer.size() > 0);
        assert!(
            !buffer
                .upload(&device, &[], |_, _| panic!("empty upload"))
                .unwrap()
        );
    }

    #[test]
    fn capacity_handles_rounding_overflow() {
        assert_eq!(capacity((1 << 63) + 1, u64::MAX), u64::MAX);
    }

    #[test]
    #[should_panic(expected = "actor motion history exceeds the storage binding limit")]
    fn oversized_motion_history_is_rejected_without_truncation() {
        capacity(1025, 1024);
    }

    #[test]
    fn repeated_motion_contents_stage_no_bytes_or_allocations() {
        let (device, _) = wgpu::Device::noop(&Default::default());
        let device = RenderDevice::from(device);
        let mut buffer = MotionBuffer::new(&device, render_model::ACTOR_BONE_MATRIX_BYTES).unwrap();
        let bytes = [1; render_model::ACTOR_BONE_MATRIX_BYTES];
        let mut uploaded = 0;
        assert!(
            !buffer
                .upload(&device, &bytes, |_, source| uploaded += source.len())
                .unwrap()
        );
        assert_eq!(uploaded, bytes.len());
        let identity = buffer.buffer.id();
        let allocated = test_alloc::thread_allocations();
        for source in [&bytes[..], &bytes[..bytes.len() / 2], &[], &bytes[..]] {
            assert!(
                !buffer
                    .upload(&device, source, |_, _| panic!(
                        "unchanged contents uploaded"
                    ))
                    .unwrap()
            );
        }
        assert_eq!(test_alloc::thread_allocations(), allocated);
        assert_eq!(buffer.buffer.id(), identity);
    }

    #[test]
    fn motion_updates_reach_the_buffer_after_changes_growth_and_shrinkage() {
        let (device, _) = wgpu::Device::noop(&Default::default());
        let device = RenderDevice::from(device);
        let mut buffer = MotionBuffer::new(&device, render_model::ACTOR_BONE_MATRIX_BYTES).unwrap();
        let mut writes = Vec::new();
        let original = vec![1; render_model::ACTOR_BONE_MATRIX_BYTES];
        buffer
            .upload(&device, &original, |_, bytes| writes.push(bytes.to_vec()))
            .unwrap();
        let identity = buffer.buffer.id();
        let larger = vec![2; original.len() * 2];
        assert!(
            buffer
                .upload(&device, &larger, |_, bytes| writes.push(bytes.to_vec()))
                .unwrap()
        );
        assert_ne!(buffer.buffer.id(), identity);
        let identity = buffer.buffer.id();
        assert!(
            !buffer
                .upload(&device, &original, |_, bytes| writes.push(bytes.to_vec()))
                .unwrap()
        );
        assert_eq!(buffer.buffer.id(), identity);
        let mut restored = larger.clone();
        restored[..original.len()].copy_from_slice(&original);
        assert!(
            !buffer
                .upload(&device, &restored, |_, _| panic!("retained tail uploaded"))
                .unwrap()
        );
        assert!(
            !buffer
                .upload(&device, &larger, |_, bytes| writes.push(bytes.to_vec()))
                .unwrap()
        );
        assert_eq!(writes, [original.clone(), larger.clone(), original, larger]);
    }
}
