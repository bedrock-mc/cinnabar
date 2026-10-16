use super::*;
use std::cell::Cell;

#[test]
fn inactive_wrappers_consume_draws_without_allocating_identity_storage() {
    let probe = ActiveFrameProbe::default();
    let entity = Entity::from_bits(1);
    let allocation = FrameAllocationIdentity {
        entity,
        key: SubChunkKey::new(0, 0, 0, 0),
        generation: 1,
    };
    let consumed = Cell::new(0);
    let draws = || {
        std::iter::repeat_n((entity, allocation), 3).inspect(|_| {
            consumed.set(consumed.get() + 1);
        })
    };
    let before = test_alloc::thread_allocations();
    let counts = [
        probe.record_mdi_draws(draws()),
        probe.record_mdi_streams(draws(), ChunkStreamMask::CUBE),
        probe.record_transparent_draw(ViewSortGeneration::new(1), draws()),
    ];
    let allocations = test_alloc::thread_allocations() - before;
    assert_eq!(counts, [3; 3]);
    assert_eq!(consumed.get(), 9);
    assert_eq!(allocations, 0);
}
