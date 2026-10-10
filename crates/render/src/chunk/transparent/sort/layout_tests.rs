use super::*;

/// A sub-chunk's water: a level surface plus a wall, so its face order depends on the camera.
fn group(x: i32, generation: u64, record_start: u32, faces: u32) -> Arc<TransparentGroupInput> {
    let key = SubChunkKey::new(0, x, 3, 0);
    let origin = Vec3::from_array(chunk_origin(key).map(|value| value as f32));
    let centroids = (0..faces)
        .map(|index| {
            let (a, b) = ((index % 16) as f32 + 0.5, (index / 16 % 16) as f32 + 0.5);
            origin
                + if index % 3 == 0 {
                    Vec3::new(0.001, a, b)
                } else {
                    Vec3::new(a, 14.9, b)
                }
        })
        .collect();
    let liquid = record_start * 4..(record_start + faces) * 4;
    Arc::new(TransparentGroupInput {
        identity: TransparentAllocationIdentity::new(
            key,
            generation,
            liquid,
            0..faces * 2,
            x.unsigned_abs(),
        ),
        tint_identity: ChunkBiomeTintIdentity::default(),
        centroids,
        tint_colors: Box::new([]),
    })
}

/// Copies group allocation identities in the same key order.
fn allocations(groups: &[Arc<TransparentGroupInput>]) -> Vec<TransparentAllocationIdentity> {
    groups.iter().map(|group| group.identity.clone()).collect()
}

/// Plans a fixture water sort against the chosen base and upload budget.
fn plan(
    camera: Vec3,
    groups: &[Arc<TransparentGroupInput>],
    base: Option<&TransparentLayoutBase>,
    upload_cap: usize,
) -> TransparentSortOutput {
    plan_transparent_slot(camera, &allocations(groups), groups, base, upload_cap)
}

/// Builds a committed layout base from a prior output and its group identities.
fn base_of(
    output: &TransparentSortOutput,
    groups: &[Arc<TransparentGroupInput>],
) -> TransparentLayoutBase {
    TransparentLayoutBase {
        refs: Arc::clone(&output.refs),
        allocations: allocations(groups).into(),
        layout: output.layout.clone(),
    }
}

/// Each key's refs as the slot's layout places them.
fn by_key(output: &TransparentSortOutput) -> BTreeMap<SubChunkKey, Vec<PackedTransparentDrawRef>> {
    output
        .layout
        .groups
        .iter()
        .map(|group| {
            let range = group.ref_range.start as usize..group.ref_range.end as usize;
            (group.key, output.refs[range].to_vec())
        })
        .collect()
}

const CAMERA: Vec3 = Vec3::new(40.5, 70.0, -20.5);

#[test]
fn a_new_group_writes_only_its_own_refs() {
    let old = [group(0, 1, 0, 40), group(4, 1, 100, 40)];
    let packed = plan(CAMERA, &old, None, usize::MAX);
    assert!(packed.patch.is_none());
    assert_eq!(packed.sorted_refs, 80);
    let base = base_of(&packed, &old);

    let new = [old[0].clone(), group(2, 1, 200, 30), old[1].clone()];
    let patched = plan(CAMERA, &new, Some(&base), usize::MAX);
    let patch = patched.patch.as_ref().expect("planned in place");
    assert_eq!(patched.sorted_refs, 30, "only the new group is sorted");
    assert!(Arc::ptr_eq(&patch.base, &packed.refs));
    assert_eq!(patch.urgent.len(), 1);
    assert_eq!(patch.urgent[0], 80..110);
    assert!(patch.deferred.is_empty());
    assert_eq!(patched.refs[..80], packed.refs[..]);
    assert_eq!(
        by_key(&patched),
        by_key(&plan(CAMERA, &new, None, usize::MAX))
    );
}

#[test]
fn a_removed_group_frees_its_range_for_the_next_new_group() {
    let old = [
        group(0, 1, 0, 40),
        group(2, 1, 100, 30),
        group(4, 1, 200, 40),
    ];
    let packed = plan(CAMERA, &old, None, usize::MAX);
    let removed = [old[0].clone(), old[2].clone()];
    let patched = plan(CAMERA, &removed, Some(&base_of(&packed, &old)), usize::MAX);
    let patch = patched.patch.as_ref().unwrap();
    assert!(
        patch.urgent.is_empty() && patch.deferred.is_empty(),
        "nothing uploads"
    );
    assert_eq!(patched.layout.free[..].len(), 1);
    assert_eq!(patched.layout.free[..][0], 40..70);

    let added = [removed[0].clone(), group(3, 1, 300, 25), removed[1].clone()];
    let refilled = plan(
        CAMERA,
        &added,
        Some(&base_of(&patched, &removed)),
        usize::MAX,
    );
    assert_eq!(refilled.patch.as_ref().unwrap().urgent.len(), 1);
    assert_eq!(refilled.patch.as_ref().unwrap().urgent[0], 40..65);
    assert_eq!(refilled.layout.free[..].len(), 1);
    assert_eq!(refilled.layout.free[..][0], 65..70);
    assert_eq!(refilled.refs.len(), 110, "the slot does not grow");
    assert_eq!(
        by_key(&refilled),
        by_key(&plan(CAMERA, &added, None, usize::MAX))
    );
}

#[test]
fn a_class_change_rewrites_in_place_and_may_lag() {
    let groups = [group(0, 1, 0, 40), group(4, 1, 100, 40)];
    let packed = plan(CAMERA, &groups, None, usize::MAX);
    // Crossing into the next sub-chunk on x changes the far class of the group at x = 4.
    let moved = CAMERA + Vec3::new(32.0, 0.0, 0.0);
    let patched = plan(moved, &groups, Some(&base_of(&packed, &groups)), usize::MAX);
    let patch = patched.patch.as_ref().unwrap();
    assert!(
        patch.urgent.is_empty(),
        "every range still holds its allocation's refs"
    );
    assert!(!patch.deferred.is_empty());
    assert!(
        patch
            .deferred
            .iter()
            .all(|span| span.start >= 40 && span.end <= 80)
    );
    assert_eq!(patched.layout.groups, packed.layout.groups);
    assert_eq!(
        by_key(&patched),
        by_key(&plan(moved, &groups, None, usize::MAX))
    );

    // An unchanged class keeps everything and sorts nothing.
    let again = plan(
        moved,
        &groups,
        Some(&base_of(&patched, &groups)),
        usize::MAX,
    );
    assert_eq!(again.sorted_refs, 0);
    assert!(again.patch.unwrap().deferred.is_empty());
}

#[test]
fn a_remeshed_group_is_written_before_it_draws() {
    let old = [group(0, 1, 0, 40), group(4, 1, 100, 40)];
    let packed = plan(CAMERA, &old, None, usize::MAX);
    let remeshed = [old[0].clone(), group(4, 2, 500, 40)];
    let patched = plan(CAMERA, &remeshed, Some(&base_of(&packed, &old)), usize::MAX);
    let patch = patched.patch.as_ref().unwrap();
    // Its old range may hold the retired mesh's records, so the new refs are urgent.
    assert_eq!(patch.urgent.len(), 1);
    assert_eq!(patch.urgent[0], 40..80);
    assert_eq!(
        by_key(&patched),
        by_key(&plan(CAMERA, &remeshed, None, usize::MAX))
    );
}

#[test]
fn oversized_or_fragmenting_changes_pack_afresh() {
    let old = [group(0, 1, 0, 40), group(4, 1, 100, 40)];
    let packed = plan(CAMERA, &old, None, usize::MAX);
    let base = base_of(&packed, &old);
    let new = [old[0].clone(), group(2, 1, 200, 30), old[1].clone()];
    let capped = plan(CAMERA, &new, Some(&base), 29);
    assert!(capped.patch.is_none(), "30 urgent refs exceed the cap");
    assert!(capped.layout.free.is_empty());
    assert_eq!(
        capped.sorted_refs, 30,
        "kept groups still come from the base"
    );
    assert_eq!(
        capped
            .layout
            .groups
            .iter()
            .map(|group| group.ref_range.clone())
            .collect::<Vec<_>>(),
        [0..40, 40..70, 70..110]
    );

    // Growing one group far beyond the rest would leave most of the slot free.
    let large = [
        group(0, 2, 1_000, 4_096),
        group(1, 1, 9_000, 4_096),
        old[1].clone(),
    ];
    let mut base = base_of(&plan(CAMERA, &large, None, usize::MAX), &large);
    let mut packed_afresh = false;
    for generation in 3..40 {
        let regrown = [
            group(0, generation, 1_000, 4_096),
            large[1].clone(),
            large[2].clone(),
        ];
        let output = plan(CAMERA, &regrown, Some(&base), usize::MAX);
        packed_afresh |= output.patch.is_none();
        assert!(output.refs.len() <= 2 * output.layout.live_refs() + MIN_FRAGMENTED_REFS);
        base = base_of(&output, &regrown);
    }
    assert!(
        !packed_afresh,
        "a freed range is reused rather than appended"
    );
}
