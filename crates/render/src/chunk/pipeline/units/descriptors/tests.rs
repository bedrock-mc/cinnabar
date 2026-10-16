use super::*;
use render_model::terrain_units::QUADS_PER_UNIT;

/// Uses actual unit splitting so changing counts also exercises a partial final unit.
fn section(entity: Entity, start: u32, quads: u32, generation: u64) -> Arc<SectionUnits> {
    SectionUnits::fixture(
        entity,
        TerrainUnit::build(
            start..start + quads,
            start,
            [[0; 3], [16; 3]],
            None,
            generation,
        )
        .collect(),
    )
}

/// Applies one source publication with the same retained lookup as preparation.
fn sync(cache: &mut Descriptors, sections: &[Arc<SectionUnits>], capacity: usize) -> bool {
    let lookup = sections
        .iter()
        .enumerate()
        .map(|(index, section)| (section.entity, index))
        .collect();
    cache.synchronize(sections, &lookup, capacity)
}

/// Resolves visibility through the slots exactly as the shader does.
fn visible(cache: &Descriptors, selected: &[Entity]) -> Vec<VisibleTerrainUnit> {
    let mut references = Vec::new();
    cache.select(selected, None, &mut references);
    references
        .iter()
        .map(|&slot| cache.records[slot as usize])
        .collect()
}

#[test]
fn changing_one_section_keeps_other_slots_and_only_uploads_changed_descriptors() {
    let mut world = World::new();
    let [a, b] = [(); 2].map(|_| world.spawn_empty().id());
    let mut cache = Descriptors::default();
    let mut sources = [
        section(a, 10, QUADS_PER_UNIT + 1, 1),
        section(b, 200, 2 * QUADS_PER_UNIT + 1, 1),
    ];
    assert!(sync(&mut cache, &sources, 8));
    let b_slots = cache.sections[&b].slots.clone();
    let b_records = visible(&cache, &[b]);
    assert!(sync(&mut cache, &sources, 8));
    assert!(cache.dirty.is_empty());
    sources[0] = section(a, 400, QUADS_PER_UNIT + 1, 2);
    assert!(sync(&mut cache, &sources, 8));
    assert_eq!(cache.sections[&b].slots, b_slots);
    assert_eq!(visible(&cache, &[b]), b_records);
    assert_eq!(cache.dirty, cache.sections[&a].slots);
    assert_eq!(cache.dirty.len(), 2);
    assert_eq!(
        visible(&cache, &[a]),
        sources[0]
            .units
            .iter()
            .map(TerrainUnit::descriptor)
            .collect::<Vec<_>>()
    );
}

#[test]
fn visibility_reordering_and_facing_never_rewrite_geometry() {
    let mut world = World::new();
    let [a, b] = [(); 2].map(|_| world.spawn_empty().id());
    let directions = CubeQuadLayout::SOLID_FACE_ORDER
        .into_iter()
        .enumerate()
        .map(|(index, face)| {
            TerrainUnit::build(
                index as u32..index as u32 + 1,
                7,
                [[0; 3], [16; 3]],
                Some(face as u8),
                1,
            )
            .next()
            .unwrap()
        })
        .collect();
    let sources = [SectionUnits::fixture(a, directions), section(b, 20, 1, 1)];
    let mut cache = Descriptors::default();
    assert!(sync(&mut cache, &sources, 8));
    let records = cache.records.clone();
    assert!(sync(&mut cache, &sources, 8));
    for camera in [None, Some([-1.0; 3]), Some([17.0; 3])] {
        let mut references = Vec::new();
        cache.select(&[b, a], camera, &mut references);
        let actual = references
            .iter()
            .map(|&slot| cache.records[slot as usize])
            .collect::<Vec<_>>();
        let expected = [1, 0]
            .into_iter()
            .flat_map(|index| {
                let source = &sources[index];
                let mask = facing(source.key, camera);
                source
                    .units
                    .iter()
                    .filter(move |unit| faces(unit, mask))
                    .map(TerrainUnit::descriptor)
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }
    assert_eq!(cache.records, records);
    assert!(cache.dirty.is_empty());
    assert_eq!(
        visible(&cache, &[b]),
        vec![sources[1].units[0].descriptor()]
    );
    assert!(visible(&cache, &[]).is_empty());
}

#[test]
fn shrinking_before_growth_reuses_slots_even_when_the_growing_section_is_first() {
    let mut world = World::new();
    let [a, b] = [(); 2].map(|_| world.spawn_empty().id());
    let mut cache = Descriptors::default();
    assert!(sync(
        &mut cache,
        &[
            section(a, 0, 1, 1),
            section(b, 100, 2 * QUADS_PER_UNIT + 1, 1)
        ],
        4
    ));
    let a_slot = cache.sections[&a].slots[0];
    let b_slot = cache.sections[&b].slots[0];
    assert!(sync(
        &mut cache,
        &[
            section(a, 0, 2 * QUADS_PER_UNIT + 1, 2),
            section(b, 100, 1, 2)
        ],
        4
    ));
    assert_eq!(cache.records.len(), 4);
    assert_eq!(cache.sections[&a].slots[0], a_slot);
    assert_eq!(cache.sections[&b].slots, [b_slot]);
    assert_eq!(visible(&cache, &[a, b]).len(), 4);
}

#[test]
fn removal_reuse_and_new_generation_cannot_leave_old_visible_descriptors() {
    let mut world = World::new();
    let [old, kept, replacement] = [(); 3].map(|_| world.spawn_empty().id());
    let mut cache = Descriptors::default();
    let kept_source = section(kept, 100, 1, 1);
    assert!(sync(
        &mut cache,
        &[section(old, 0, QUADS_PER_UNIT + 1, 1), kept_source.clone()],
        3
    ));
    let kept_slot = cache.sections[&kept].slots.clone();
    let replacement_source = section(replacement, 800, QUADS_PER_UNIT + 1, 2);
    assert!(sync(
        &mut cache,
        &[kept_source, replacement_source.clone()],
        3
    ));
    assert!(visible(&cache, &[old]).is_empty());
    assert_eq!(cache.sections[&kept].slots, kept_slot);
    assert_eq!(
        visible(&cache, &[replacement]),
        replacement_source
            .units
            .iter()
            .map(TerrainUnit::descriptor)
            .collect::<Vec<_>>()
    );
    assert_eq!(cache.records.len(), 3);
}

#[test]
fn capacity_fallback_and_empty_views_release_retained_slots() {
    let mut world = World::new();
    let a = world.spawn_empty().id();
    let mut cache = Descriptors::default();
    assert!(sync(&mut cache, &[section(a, 0, QUADS_PER_UNIT + 1, 1)], 2));
    assert!(!sync(
        &mut cache,
        &[section(a, 0, 2 * QUADS_PER_UNIT + 1, 2)],
        2
    ));
    assert!(cache.records.is_empty() && cache.sections.is_empty());
    assert!(sync(
        &mut cache,
        &[section(a, 500, QUADS_PER_UNIT + 1, 3)],
        2
    ));
    assert_eq!(cache.dirty, [0, 1]);
    assert!(sync(&mut cache, &[], 2));
    assert!(cache.records.is_empty() && cache.sections.is_empty() && cache.free.is_empty());
}

#[test]
fn upload_spans_do_not_copy_unchanged_slots_between_changed_sections() {
    assert_eq!(
        super::super::buffers::dirty_ranges(&[0, 1, 4, 8, 9, 10]).collect::<Vec<_>>(),
        [0..2, 4..5, 8..11]
    );
    assert!(super::super::buffers::dirty_ranges(&[]).next().is_none());
}
