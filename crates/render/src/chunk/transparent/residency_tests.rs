use super::transparent::resident_transparent_allocation;
use super::*;

#[test]
fn grown_same_start_liquid_range_keeps_old_refs_physically_resident() {
    let texture_identity = ChunkTextureAssetIdentity::new(1, 1);
    let tint_identity = ChunkBiomeTintIdentity::new(2, 2);
    let identity =
        TransparentAllocationIdentity::new(SubChunkKey::new(0, 0, 0, 0), 3, 8..16, 32..36, 1);
    let key = ViewSortKey::try_new(
        [0.0; 3],
        [0.0, 0.0, 0.0, 1.0],
        vec![identity.clone()],
        texture_identity,
        tint_identity,
    )
    .unwrap();
    let snapshot = committed_transparent_state(
        &key,
        vec![PackedTransparentDrawRef::new(2, identity.metadata_index)],
    )
    .committed()
    .unwrap()
    .clone();
    let mut resident = resident_transparent_allocation(&identity, tint_identity);
    resident.generation += 1;
    resident.liquid_range = Some(8..24);
    resident.liquid_lighting_range = Some(40..48);
    assert!(transparent_snapshot_addresses_are_resident(
        &snapshot,
        [&resident],
        std::iter::empty(),
        texture_identity,
        tint_identity,
    ));
}

#[test]
fn physical_residency_rejects_moved_shrunk_or_structurally_invalid_streams() {
    let texture_identity = ChunkTextureAssetIdentity::new(1, 1);
    let tint_identity = ChunkBiomeTintIdentity::new(2, 2);
    let identity =
        TransparentAllocationIdentity::new(SubChunkKey::new(0, 0, 0, 0), 3, 8..16, 32..36, 1);
    let key = ViewSortKey::try_new(
        [0.0; 3],
        [0.0, 0.0, 0.0, 1.0],
        vec![identity.clone()],
        texture_identity,
        tint_identity,
    )
    .unwrap();
    let snapshot = committed_transparent_state(
        &key,
        vec![PackedTransparentDrawRef::new(2, identity.metadata_index)],
    )
    .committed()
    .unwrap()
    .clone();
    let exact = resident_transparent_allocation(&identity, tint_identity);
    let mut moved = exact.clone();
    moved.liquid_range = Some(4..16);
    let mut shrunk = exact.clone();
    shrunk.liquid_range = Some(8..12);
    let mut missing_lighting = exact.clone();
    missing_lighting.liquid_lighting_range = None;
    let mut invalid_lighting_count = exact.clone();
    invalid_lighting_count.liquid_lighting_range = Some(32..34);
    let mut changed_tint = exact.clone();
    changed_tint.tint_identity = ChunkBiomeTintIdentity::new(9, 9);
    let mut changed_key = exact;
    changed_key.key = SubChunkKey::new(0, 1, 0, 0);

    for resident in [
        moved,
        shrunk,
        missing_lighting,
        invalid_lighting_count,
        changed_tint,
        changed_key,
    ] {
        assert!(!transparent_snapshot_addresses_are_resident(
            &snapshot,
            [&resident],
            std::iter::empty(),
            texture_identity,
            tint_identity,
        ));
    }
    assert!(!transparent_snapshot_addresses_are_resident(
        &snapshot,
        [&resident_transparent_allocation(&identity, tint_identity)],
        std::iter::empty(),
        ChunkTextureAssetIdentity::new(9, 9),
        tint_identity,
    ));
}

fn direct_scan_residency(
    snapshot: &TransparentOrderedSnapshot,
    resident: &[GpuChunkAllocation],
    retired: &[GpuChunkAllocation],
    tint_identity: ChunkBiomeTintIdentity,
) -> bool {
    snapshot.key.visible_allocations.iter().all(|identity| {
        resident.iter().any(|allocation| {
            allocation.tint_identity == tint_identity
                && transparent_resident_allocation_contains(identity, allocation)
        }) || retired.iter().any(|allocation| {
            allocation.tint_identity == tint_identity
                && transparent_allocation_is_exact(identity, allocation)
        })
    })
}

/// The keyed residency lookup agrees with a direct scan across resident, retired and stale entries.
#[test]
fn keyed_residency_lookup_matches_direct_scan() {
    let texture_identity = ChunkTextureAssetIdentity::new(1, 1);
    let tint_identity = ChunkBiomeTintIdentity::new(2, 2);
    let other_tint = ChunkBiomeTintIdentity::new(9, 9);
    let identities = (0..36_u32)
        .map(|index| {
            TransparentAllocationIdentity::new(
                SubChunkKey::new(0, index as i32, 0, 0),
                3,
                index * 8..index * 8 + 8,
                1000 + index * 4..1004 + index * 4,
                index,
            )
        })
        .collect::<Vec<_>>();
    let (mut resident, mut retired) = (Vec::new(), Vec::new());
    for (index, identity) in identities.iter().enumerate() {
        let exact = resident_transparent_allocation(identity, tint_identity);
        let mut stale = exact.clone();
        stale.generation += 1;
        match index % 6 {
            0 => resident.push(exact),
            // Generation-only updates stay physically resident.
            1 => resident.push(stale),
            2 => {
                let mut moved = exact.clone();
                moved.liquid_range =
                    Some(identity.liquid_range.start + 4..identity.liquid_range.end + 4);
                resident.push(moved);
                retired.push(exact);
            }
            // A retired allocation must match the exact generation.
            3 => retired.push(stale),
            4 => {
                let mut tinted = exact;
                tinted.tint_identity = other_tint;
                resident.push(tinted.clone());
                retired.push(tinted);
            }
            _ => {}
        }
        let mut unrelated = resident_transparent_allocation(identity, tint_identity);
        unrelated.key = SubChunkKey::new(1, index as i32, 0, 0);
        resident.push(unrelated);
    }
    let snapshot_of = |visible: Vec<TransparentAllocationIdentity>| {
        let key = ViewSortKey::try_new(
            [0.0; 3],
            [0.0, 0.0, 0.0, 1.0],
            visible,
            texture_identity,
            tint_identity,
        )
        .unwrap();
        committed_transparent_state(&key, vec![PackedTransparentDrawRef::new(2, 0)])
            .committed()
            .unwrap()
            .clone()
    };
    let check = |snapshot: &TransparentOrderedSnapshot| {
        let keyed = transparent_snapshot_addresses_are_resident(
            snapshot,
            resident.iter(),
            retired.iter(),
            texture_identity,
            tint_identity,
        );
        assert_eq!(
            keyed,
            direct_scan_residency(snapshot, &resident, &retired, tint_identity)
        );
        keyed
    };
    for (index, identity) in identities.iter().enumerate() {
        assert_eq!(check(&snapshot_of(vec![identity.clone()])), index % 6 < 3);
    }
    let drawable = identities
        .iter()
        .enumerate()
        .filter(|(index, _)| index % 6 < 3)
        .map(|(_, identity)| identity.clone())
        .collect::<Vec<_>>();
    assert!(check(&snapshot_of(drawable)));
    assert!(!check(&snapshot_of(identities)));
}
