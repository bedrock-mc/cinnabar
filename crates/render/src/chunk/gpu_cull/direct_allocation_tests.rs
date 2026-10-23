use super::*;

/// Failed growth retains all old buffers and readbacks, then retries without losing records.
#[test]
fn direct_occlusion_retains_storage_after_failed_growth() {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let device = RenderDevice::from(device);
    let queue = RenderQueue::new(queue);
    let mut occlusion = DirectOcclusion::new(&device);
    let mut world = World::new();
    let entity = world.spawn_empty().id();
    let tint = ChunkBiomeTintIdentity::default();
    let hidden = HashSet::new();
    assert!(occlusion.upload(&device, &queue));
    let previous = occlusion.storage.as_ref().unwrap().records.clone();
    let readbacks = occlusion.readbacks.clone();
    let capacity = occlusion.storage.as_ref().unwrap().capacity;
    occlusion.table.update(
        entity,
        capacity,
        tint,
        super::super::model::CullRecord::default(),
        &hidden,
    );
    let failure = crate::gpu_allocation::inject(|label, _| label == "terrain occlusion storage");
    assert!(!occlusion.upload(&device, &queue));
    assert_eq!(occlusion.storage.as_ref().unwrap().records, previous);
    assert_eq!(occlusion.readbacks, readbacks);
    drop(failure);
    for _ in 0..8 {
        if occlusion.upload(&device, &queue) {
            break;
        }
    }
    assert!(occlusion.upload(&device, &queue));
    assert!(occlusion.storage.as_ref().unwrap().capacity > capacity);
}
