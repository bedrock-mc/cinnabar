//! The sort manifest follows resident changes without rebuilding unchanged inputs.
use super::transparent::resident_transparent_allocation;
use super::*;

const TINT: ChunkBiomeTintIdentity = ChunkBiomeTintIdentity::new(2, 2);

fn arena() -> ChunkGpuArena {
    let (device, _queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    ChunkGpuArena::new(&RenderDevice::from(device))
}

/// A resident with two transparent faces at sub-chunk `x`.
fn identity(x: i32, generation: u64) -> TransparentAllocationIdentity {
    let start = x as u32 * 8;
    TransparentAllocationIdentity::new(
        SubChunkKey::new(0, x, 0, 0),
        generation,
        start..start + 8,
        1_000 + start..1_004 + start,
        x as u32,
    )
}

fn record(arena: &mut ChunkGpuArena, x: i32, generation: u64) {
    arena.transparent_liquids.record(
        Entity::from_bits(x as u64 + 1),
        &resident_transparent_allocation(&identity(x, generation), TINT),
    );
}

/// Builds the manifest, counting how many sort inputs it had to build.
fn manifest(
    runtime: &mut TransparentSortRuntime,
    arena: &ChunkGpuArena,
    camera_chunk: [i32; 3],
    metrics: &TransparentSortMetrics,
) -> (Arc<[TransparentAllocationIdentity]>, usize) {
    let mut built = 0;
    let allocations = runtime.resident_manifest(
        arena,
        false,
        TINT,
        camera_chunk,
        |resident| {
            built += 1;
            Some(TransparentGroupInput {
                identity: resident.identity.clone(),
                tint_identity: TINT,
                centroids: Box::new([Vec3::ZERO; 2]),
                tint_colors: Box::new([]),
            })
        },
        metrics,
    );
    (allocations, built)
}

#[test]
fn unchanged_residents_keep_their_inputs_and_one_change_builds_one() {
    let mut arena = arena();
    for x in 0..4 {
        record(&mut arena, x, 1);
    }
    let mut runtime = TransparentSortRuntime::default();
    let metrics = TransparentSortMetrics::default();
    let (first, built) = manifest(&mut runtime, &arena, [0; 3], &metrics);
    assert_eq!((first.len(), built), (4, 4));
    let first_groups = runtime.manifest_groups();

    let (again, built) = manifest(&mut runtime, &arena, [9, 0, 0], &metrics);
    assert!(
        Arc::ptr_eq(&first, &again),
        "an unchanged index reuses the manifest"
    );
    assert_eq!(built, 0);

    record(&mut arena, 2, 2);
    let (changed, built) = manifest(&mut runtime, &arena, [0; 3], &metrics);
    assert_eq!(built, 1, "only the re-meshed resident is rebuilt");
    assert_eq!(changed[2], identity(2, 2));
    let groups = runtime.manifest_groups();
    for index in [0, 1, 3] {
        assert!(Arc::ptr_eq(&groups[index], &first_groups[index]));
    }
    assert_eq!(metrics.snapshot().ceiling_reject_count, 0);
}

#[test]
fn the_ref_ceiling_keeps_the_nearest_residents() {
    let mut arena = arena();
    for x in 0..6 {
        record(&mut arena, x, 1);
    }
    let mut runtime = TransparentSortRuntime {
        // Two faces each: room for three residents.
        ref_ceiling: 6,
        ..Default::default()
    };
    let metrics = TransparentSortMetrics::default();
    let keys = |allocations: &[TransparentAllocationIdentity]| {
        allocations
            .iter()
            .map(|identity| identity.key.x)
            .collect::<Vec<_>>()
    };
    let (near_origin, _) = manifest(&mut runtime, &arena, [0; 3], &metrics);
    assert_eq!(keys(&near_origin), [0, 1, 2]);
    assert_eq!(metrics.snapshot().ceiling_reject_count, 1);
    let (moved, built) = manifest(&mut runtime, &arena, [5, 0, 0], &metrics);
    assert_eq!(keys(&moved), [3, 4, 5]);
    assert_eq!(built, 3);
}
