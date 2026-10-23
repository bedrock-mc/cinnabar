use super::*;

/// A complete table survives failed growth, retries later, and returns after a limit overflow.
#[test]
fn cull_storage_retains_old_buffers_on_growth_failure_and_limit_overflow() {
    for limited in [false, true] {
        let limits = if limited {
            wgpu::Limits {
                max_storage_buffer_binding_size: super::super::model::draw_offset_bytes(3),
                ..Default::default()
            }
        } else {
            wgpu::Limits::default()
        };
        let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor {
            required_limits: limits,
            ..Default::default()
        });
        let device = RenderDevice::from(device);
        let queue = RenderQueue::new(queue);
        let mut cull = GpuCull::new(&device, GpuCullSubmission::Fixed);
        let mut world = World::new();
        let first = world.spawn_empty().id();
        let overflow = world.spawn_empty().id();
        let tint = ChunkBiomeTintIdentity::default();
        let hidden = HashSet::new();
        cull.table
            .update(first, 0, tint, CullRecord::default(), &hidden);
        assert!(upload_records(&mut cull, &device, &queue));
        let previous = cull.storage.as_ref().unwrap().records.clone();
        let capacity = cull.storage.as_ref().unwrap().capacity;
        cull.table
            .update(overflow, capacity, tint, CullRecord::default(), &hidden);
        let failure = crate::gpu_allocation::inject(|label, _| label == "terrain cull storage");
        assert!(!upload_records(&mut cull, &device, &queue));
        assert_eq!(cull.storage.as_ref().unwrap().records, previous);
        drop(failure);
        if limited {
            assert!(!upload_records(&mut cull, &device, &queue));
            cull.table.remove(overflow);
            cull.table.trim();
        }
        for _ in 0..8 {
            if upload_records(&mut cull, &device, &queue) {
                break;
            }
        }
        assert!(upload_records(&mut cull, &device, &queue));
        assert!(cull.storage.as_ref().unwrap().capacity >= cull.slot_count());
    }
}
