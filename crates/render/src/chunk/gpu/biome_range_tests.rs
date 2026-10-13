//! Biome ranges a new generation keeps from its predecessor, and when they are freed.

use super::*;

/// A generation keeps its predecessor's biome range only for a word-identical, non-fallback record.
#[test]
fn identical_biome_record_keeps_its_resident_range() {
    let column = world::DecodedBiomeColumn::decode(
        -4,
        1,
        &[1, 84],
        &world::RawBiomeIds { default_biome: 0 },
    );
    let storage = column.storage(-4).expect("decoded biome section");
    let record = meshing::PackedBiomeRecord::from_storage(&storage, |id| id + 1);
    let recoloured = meshing::PackedBiomeRecord::from_storage(&storage, |id| id + 2);
    assert!(!record.is_fallback());
    let mut old = retirement_test_allocation();
    assert_eq!(
        old.reusable_biome(&record),
        None,
        "the fallback holds no record"
    );

    old.biome_record = Some(record.clone());
    assert_eq!(old.reusable_biome(&record), Some((2..6, 4)));
    assert_eq!(old.reusable_biome(&recoloured), None);
}

/// A shared biome range stays allocated while any generation holding it is still drawable: the
/// newer holder retiring first must not free the range an older, still-referenced one reads.
#[test]
fn shared_biome_range_outlives_every_holder() {
    let (device, _queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let mut arena = ChunkGpuArena::new(&RenderDevice::from(device));
    arena.biome_len = 32;
    let shared = 8..12;
    let range_is_free = |arena: &ChunkGpuArena| {
        arena.biome_len <= shared.start as usize
            || arena
                .free_biomes
                .iter()
                .any(|free| free.start <= shared.start && free.end >= shared.end)
    };
    let retire = |arena: &mut ChunkGpuArena, entity: u32, release_epoch: Option<u64>| {
        let retirement = RetiredArenaAllocation {
            entity: Entity::from_raw_u32(entity).unwrap(),
            identity: retirement_test_allocation().gpu,
            quad: None,
            geometry: None,
            biome: Some((shared.clone(), 4)),
            origin: None,
            release_epoch,
        };
        assert!(
            arena
                .retirement_budget
                .try_reserve(1, retirement.owned_bytes())
        );
        arena.retired_allocations.push(retirement);
    };
    // Generation B kept generation A's range; a transparent snapshot still draws A.
    arena.shared_biome_ranges.share(shared.start);
    retire(&mut arena, 1, None);
    retire(&mut arena, 2, Some(1));

    release_completed_transparent_retirements(&mut arena, 1);
    assert_eq!(arena.retired_allocations.len(), 1);
    assert!(
        !range_is_free(&arena),
        "B's release freed a range A still reads"
    );

    arena.retired_allocations[0].release_epoch = Some(2);
    release_completed_transparent_retirements(&mut arena, 2);
    assert!(arena.retired_allocations.is_empty());
    assert!(
        range_is_free(&arena),
        "the last holder's release frees the range"
    );
}
