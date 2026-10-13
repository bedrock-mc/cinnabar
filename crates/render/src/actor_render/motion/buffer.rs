//! Retained actor history storage avoids staging identical bytes on repeated frames.

use bevy::render::{render_resource::*, renderer::RenderDevice};

pub(super) struct MotionBuffer {
    pub buffer: Buffer,
    contents: Vec<u8>,
}

impl MotionBuffer {
    /// Creates a nonempty binding even before the first actor history is available.
    pub fn new(device: &RenderDevice, bytes: usize) -> Self {
        Self {
            buffer: allocate(device, bytes),
            contents: Vec::new(),
        }
    }

    /// Writes changed contents and reports whether callers must rebuild the buffer binding.
    pub fn upload(
        &mut self,
        device: &RenderDevice,
        bytes: &[u8],
        write: impl FnOnce(&Buffer, &[u8]),
    ) -> bool {
        if self.contents.get(..bytes.len()) == Some(bytes) {
            return false;
        }
        let resized = self.buffer.size() < bytes.len() as u64;
        if resized {
            self.buffer = allocate(device, bytes.len());
        }
        write(&self.buffer, bytes);
        // Shorter inputs leave the unused GPU tail intact, so retain its shadow too.
        self.contents
            .resize(self.contents.len().max(bytes.len()), 0);
        self.contents[..bytes.len()].copy_from_slice(bytes);
        resized
    }
}

/// Allocates storage owned by one view, retaining capacity when subsequent inputs shrink.
fn allocate(device: &RenderDevice, bytes: usize) -> Buffer {
    device.create_buffer(&BufferDescriptor {
        label: Some("actor velocity history"),
        size: bytes as u64,
        usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_motion_contents_stage_no_bytes_or_allocations() {
        let (device, _) = wgpu::Device::noop(&Default::default());
        let device = RenderDevice::from(device);
        let mut buffer = MotionBuffer::new(&device, render_model::ACTOR_BONE_MATRIX_BYTES);
        let bytes = [1; render_model::ACTOR_BONE_MATRIX_BYTES];
        let mut uploaded = 0;
        assert!(!buffer.upload(&device, &bytes, |_, source| uploaded += source.len()));
        assert_eq!(uploaded, bytes.len());
        let identity = buffer.buffer.id();
        let allocated = test_alloc::thread_allocations();
        for source in [&bytes[..], &bytes[..bytes.len() / 2], &[], &bytes[..]] {
            assert!(!buffer.upload(&device, source, |_, _| panic!(
                "unchanged contents uploaded"
            )));
        }
        assert_eq!(test_alloc::thread_allocations(), allocated);
        assert_eq!(buffer.buffer.id(), identity);
    }

    #[test]
    fn motion_updates_reach_the_buffer_after_changes_growth_and_shrinkage() {
        let (device, _) = wgpu::Device::noop(&Default::default());
        let device = RenderDevice::from(device);
        let mut buffer = MotionBuffer::new(&device, render_model::ACTOR_BONE_MATRIX_BYTES);
        let mut writes = Vec::new();
        let original = vec![1; render_model::ACTOR_BONE_MATRIX_BYTES];
        buffer.upload(&device, &original, |_, bytes| writes.push(bytes.to_vec()));
        let identity = buffer.buffer.id();
        let larger = vec![2; original.len() * 2];
        assert!(buffer.upload(&device, &larger, |_, bytes| writes.push(bytes.to_vec())));
        assert_ne!(buffer.buffer.id(), identity);
        let identity = buffer.buffer.id();
        assert!(!buffer.upload(&device, &original, |_, bytes| writes.push(bytes.to_vec())));
        assert_eq!(buffer.buffer.id(), identity);
        let mut restored = larger.clone();
        restored[..original.len()].copy_from_slice(&original);
        assert!(!buffer.upload(&device, &restored, |_, _| panic!("retained tail uploaded")));
        assert!(!buffer.upload(&device, &larger, |_, bytes| writes.push(bytes.to_vec())));
        assert_eq!(writes, [original.clone(), larger.clone(), original, larger]);
    }
}
