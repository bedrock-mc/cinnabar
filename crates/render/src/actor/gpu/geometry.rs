//! Retained GPU storage for immutable geometry pages.

use crate::queue_writes::ProfiledQueue as _;
use bevy::render::{
    render_resource::{Buffer, BufferDescriptor, BufferUsages, CommandEncoderDescriptor},
    renderer::{RenderDevice, RenderQueue},
};
use render_model::ActorRigVertex;
use render_model::ActorRigVertexSegments;
use std::collections::BTreeSet;

/// Mirrors changed pages; growth and relocation copy retained GPU bytes instead of uploading them.
#[derive(Default)]
pub(crate) struct SegmentedVertexBuffer {
    buffer: Option<Buffer>,
    vertices: ActorRigVertexSegments,
}

#[derive(Debug, Default)]
struct PageTransfer {
    replace: bool,
    writes: Vec<usize>,
    copies: Vec<(usize, usize, usize)>,
}

impl PageTransfer {
    /// Plans page transfers by immutable identity and coalesces adjacent GPU copies.
    fn between(
        old: &ActorRigVertexSegments,
        new: &ActorRigVertexSegments,
        capacity: usize,
    ) -> Self {
        let old_pages: BTreeSet<_> = old
            .segments
            .iter()
            .zip(old.offsets.iter())
            .map(|(page, &offset)| (page.as_ptr() as usize, offset))
            .collect();
        let retained: Vec<_> = new
            .segments
            .iter()
            .zip(new.offsets.iter())
            .map(|(page, &offset)| {
                if old.epoch != new.epoch {
                    return None;
                }
                let identity = page.as_ptr() as usize;
                if old_pages.contains(&(identity, offset)) {
                    return Some(offset);
                }
                old_pages
                    .range((identity, 0)..=(identity, usize::MAX))
                    .next()
                    .map(|&(_, offset)| offset)
            })
            .collect();
        let replace = old.epoch != new.epoch
            || capacity < new.len()
            || retained
                .iter()
                .zip(new.offsets.iter())
                .any(|(old, new)| old.is_some_and(|old| old != *new));
        let mut plan = Self {
            replace,
            ..Self::default()
        };
        for (index, source) in retained.into_iter().enumerate() {
            match source {
                Some(source) if replace => {
                    let target = new.offsets[index];
                    let len = new.segments[index].len();
                    if let Some((last_source, last_target, last_len)) = plan.copies.last_mut()
                        && *last_source + *last_len == source
                        && *last_target + *last_len == target
                    {
                        *last_len += len;
                    } else {
                        plan.copies.push((source, target, len));
                    }
                }
                Some(_) => {}
                None => plan.writes.push(index),
            }
        }
        plan
    }
}

impl SegmentedVertexBuffer {
    /// Synchronizes the snapshot; false retains storage but forbids drawing this publication.
    pub(crate) fn sync(
        &mut self,
        device: &RenderDevice,
        queue: &RenderQueue,
        label: &'static str,
        vertices: &ActorRigVertexSegments,
    ) -> bool {
        if vertices.is_empty() {
            *self = Self::default();
            return true;
        }
        let stride = std::mem::size_of::<ActorRigVertex>();
        let capacity = self
            .buffer
            .as_ref()
            .map_or(0, |buffer| buffer.size() as usize / stride);
        let plan = PageTransfer::between(&self.vertices, vertices, capacity);
        #[cfg(feature = "tracy")]
        let _span = bevy::log::info_span!(
            "actor.geometry_transfer",
            label,
            replace = plan.replace,
            writes = plan.writes.len(),
            copies = plan.copies.len(),
            vertices = vertices.len(),
        )
        .entered();
        let replacement = if plan.replace {
            let binding = usize::try_from(storage_binding_bytes(device)).unwrap_or(usize::MAX);
            if vertices.len() > binding / stride {
                return false;
            }
            let capacity = vertex_buffer_capacity(vertices.len(), binding / stride);
            let Some(buffer) = crate::gpu_allocation::buffer(
                device,
                &BufferDescriptor {
                    label: Some(label),
                    size: (capacity * stride) as u64,
                    usage: BufferUsages::STORAGE | BufferUsages::COPY_DST | BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                },
            ) else {
                return false;
            };
            Some(buffer)
        } else {
            None
        };
        let buffer = replacement
            .as_ref()
            .or(self.buffer.as_ref())
            .expect("nonempty pages allocate a buffer");
        let uploaded = crate::gpu_allocation::checked(
            device,
            "actor rig transfer",
            (vertices.len() * stride) as u64,
            || {
                if !plan.copies.is_empty() {
                    let old = self.buffer.as_ref().expect("retained pages have a buffer");
                    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
                        label: Some("relocate retained actor geometry pages"),
                    });
                    for (source, target, len) in plan.copies {
                        encoder.copy_buffer_to_buffer(
                            old,
                            (source * stride) as u64,
                            buffer,
                            (target * stride) as u64,
                            (len * stride) as u64,
                        );
                    }
                    let command = encoder.finish();
                    #[cfg(feature = "tracy")]
                    let _span = bevy::log::info_span!("actor.geometry_submit").entered();
                    queue.submit([command]);
                }
                for index in plan.writes {
                    #[cfg(feature = "tracy")]
                    let _span = bevy::log::info_span!(
                        "actor.geometry_write",
                        page = index,
                        bytes = vertices.segments[index].len() * stride
                    )
                    .entered();
                    queue.profiled_write_buffer(
                        buffer,
                        (vertices.offsets[index] * stride) as u64,
                        bytemuck::cast_slice::<ActorRigVertex, u8>(&vertices.segments[index]),
                    );
                }
            },
        );
        if uploaded.is_none() {
            self.vertices = ActorRigVertexSegments::default();
            return false;
        }
        if let Some(buffer) = replacement {
            self.buffer = Some(buffer);
        }
        self.vertices = vertices.clone();
        true
    }

    /// Returns the storage buffer bound by the actor and hand passes.
    pub(crate) const fn buffer(&self) -> Option<&Buffer> {
        self.buffer.as_ref()
    }
}

/// Bytes one storage buffer binding of `device` can hold.
#[must_use]
pub fn storage_binding_bytes(device: &RenderDevice) -> u64 {
    let limits = device.limits();
    limits
        .max_storage_buffer_binding_size
        .min(limits.max_buffer_size)
}

/// Grows with headroom, never past what one storage binding of this device holds.
fn vertex_buffer_capacity(vertices: usize, binding_vertices: usize) -> usize {
    (vertices + vertices / 4)
        .min(binding_vertices)
        .max(vertices)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn rig_allocation_failure_retains_previous_buffer_and_snapshot() {
        let (device, queue) = wgpu::Device::noop(&Default::default());
        let device = RenderDevice::from(device);
        let queue = RenderQueue::new(queue);
        let old = ActorRigVertexSegments::from_vertices(page(4, 1.0));
        let mut mirror = SegmentedVertexBuffer::default();
        mirror.sync(&device, &queue, "test actor rig", &old);
        let previous = mirror.buffer().unwrap().id();
        let _failure = crate::gpu_allocation::inject(|label, _| label == "test actor rig");
        assert!(!mirror.sync(
            &device,
            &queue,
            "test actor rig",
            &ActorRigVertexSegments::from_vertices(page(64, 2.0)),
        ));
        assert_eq!(mirror.buffer().unwrap().id(), previous);
        assert_eq!(mirror.vertices.len(), old.len());
    }

    #[test]
    fn buffer_growth_stays_within_the_device_binding() {
        let binding = render_model::MAX_ACTOR_CATALOG_VERTICES * 4;
        for vertices in [
            render_model::MAX_ACTOR_RIG_VERTICES + 1,
            render_model::MAX_ACTOR_CATALOG_VERTICES,
            binding,
        ] {
            let capacity = vertex_buffer_capacity(vertices, binding);
            assert!(capacity >= vertices);
            assert!(capacity <= binding);
        }
    }

    /// Makes distinguishable page contents for transfer equality checks.
    fn page(count: usize, marker: f32) -> Vec<ActorRigVertex> {
        vec![
            ActorRigVertex {
                position: [marker; 3],
                ..ActorRigVertex::default()
            };
            count
        ]
    }

    /// CPU replay of transfers proves every destination vertex matches its immutable page.
    fn replay(
        old: &ActorRigVertexSegments,
        new: &ActorRigVertexSegments,
        capacity: usize,
    ) -> PageTransfer {
        let plan = PageTransfer::between(old, new, capacity);
        let mut source = vec![ActorRigVertex::default(); old.len()];
        for (page, &offset) in old.segments.iter().zip(old.offsets.iter()) {
            source[offset..offset + page.len()].copy_from_slice(page);
        }
        let mut target = if plan.replace {
            vec![ActorRigVertex::default(); new.len()]
        } else {
            source.clone()
        };
        target.resize(new.len(), ActorRigVertex::default());
        for &(from, to, len) in &plan.copies {
            target[to..to + len].copy_from_slice(&source[from..from + len]);
        }
        for &index in &plan.writes {
            let offset = new.offsets[index];
            let page = &new.segments[index];
            target[offset..offset + page.len()].copy_from_slice(page);
        }
        for (page, &offset) in new.segments.iter().zip(new.offsets.iter()) {
            assert_eq!(
                bytemuck::cast_slice::<ActorRigVertex, u8>(&target[offset..offset + page.len()]),
                bytemuck::cast_slice::<ActorRigVertex, u8>(page)
            );
        }
        plan
    }

    /// Growth copies the unchanged prefix on the GPU and uploads only the added page.
    #[test]
    fn growth_retains_payload_and_coalesces_copies() {
        let old = ActorRigVertexSegments::from_vertices(page(64, 1.0))
            .with_segment(Arc::from(page(8, 2.0)));
        let new = old.with_segment(Arc::from(page(64, 1.0)));
        let plan = replay(&old, &new, old.len());
        assert_eq!(plan.writes, [2]);
        assert_eq!(plan.copies, [(0, 0, old.len())]);
    }

    /// Relocation retains exact page bytes and does not upload them again.
    #[test]
    fn moved_pages_copy_without_upload() {
        let old = ActorRigVertexSegments::from_vertices(page(64, 1.0))
            .with_segment(Arc::from(page(8, 2.0)));
        let mut new = old.clone();
        new.offsets = Arc::from([8, 0]);
        let plan = replay(&old, &new, old.len());
        assert!(plan.replace);
        assert!(plan.writes.is_empty());
        assert_eq!(plan.copies, [(0, 8, 64), (64, 0, 8)]);
    }
    /// A changed page writes in place while unchanged pages retain their GPU bytes.
    #[test]
    fn replacement_uploads_only_the_changed_page() {
        let old = ActorRigVertexSegments::from_vertices(page(64, 1.0))
            .with_segment(Arc::from(page(8, 2.0)));
        let mut new = old.clone();
        new.segments = Arc::from([Arc::clone(&old.segments[0]), Arc::from(page(8, 3.0))]);
        let plan = replay(&old, &new, old.len());
        assert!(!plan.replace);
        assert_eq!(plan.writes, [1]);
        assert!(plan.copies.is_empty());
    }
}
