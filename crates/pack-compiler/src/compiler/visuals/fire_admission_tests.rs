use super::*;
use assets::RegistryProvenance;

fn record(name: &str, family: ModelFamily, state: &str) -> RegistryRecord {
    RegistryRecord {
        beacon_tint: assets::BeaconTint::Unresolved,
        traits: assets::registry::BlockTraits::empty(),
        survival: assets::registry::SurvivalRule::None,
        properties: Box::new([]),
        sequential_id: 1,
        network_hash: 1,
        name: name.into(),
        canonical_state: state.into(),
        flags: BlockFlags::empty(),
        model_family: family,
        contributor_role: ContributorRole::Primary,
        model_state: Default::default(),
        face_coverage: 0,
        collision_seed: Default::default(),
        provenance: RegistryProvenance::PMMP,
    }
}

fn visual(kind: VisualKind) -> BlockVisual {
    let mut visual = BlockVisual::diagnostic(BlockFlags::empty(), ContributorRole::Primary);
    visual.kind = kind;
    visual
}

#[test]
fn full_glass_support_is_independent_of_greedy_cube_visual_flags() {
    let record = record("minecraft:glass", ModelFamily::Cube, "{}");
    let mut visual = visual(VisualKind::Model);
    apply(&record, &mut visual);
    assert!(visual.flags.contains(BlockFlags::FIRE_TOP_SUPPORT));
    assert!(!visual.flags.contains(BlockFlags::FIRE_FLAMMABLE));
}

#[test]
fn lowered_soul_sand_model_retains_native_top_support() {
    let record = record("minecraft:soul_sand", ModelFamily::Cube, "{}");
    let mut visual = visual(VisualKind::Model);
    apply(&record, &mut visual);
    assert!(visual.flags.contains(BlockFlags::FIRE_TOP_SUPPORT));
    assert!(!visual.flags.contains(BlockFlags::FIRE_FLAMMABLE));
}

#[test]
fn native_leaves_override_cube_support_and_unknown_catch_chance_stays_unset() {
    let mut record = record("minecraft:oak_leaves", ModelFamily::Leaves, "{}");
    record.flags = BlockFlags::CUBE_GEOMETRY | BlockFlags::LEAF_MODEL;
    let mut visual = visual(VisualKind::Cube);
    apply(&record, &mut visual);
    assert!(!visual.flags.contains(BlockFlags::FIRE_TOP_SUPPORT));
    assert!(!visual.flags.contains(BlockFlags::FIRE_FLAMMABLE));
}

#[test]
fn native_upper_orientation_controls_partial_block_top_support() {
    for (family, name, key) in [
        (
            ModelFamily::Slab,
            "minecraft:stone_block_slab",
            "top_slot_bit",
        ),
        (
            ModelFamily::Stair,
            "minecraft:stone_stairs",
            "upside_down_bit",
        ),
    ] {
        for (half, expected) in [("bottom", false), ("top", true)] {
            let state = format!(
                "{{\"minecraft:vertical_half\":{{\"type\":\"string\",\"value\":\"{half}\"}}}}"
            );
            let record = record(name, family, &state);
            let mut visual = visual(VisualKind::Model);
            apply(&record, &mut visual);
            assert_eq!(
                visual.flags.contains(BlockFlags::FIRE_TOP_SUPPORT),
                expected
            );
        }
        let state = format!("{{\"{key}\":{{\"type\":\"byte\",\"value\":1}}}}");
        assert_eq!(
            canonical_upper_half(
                &record(name, family, &state),
                BlockProperty::from_name(key).unwrap()
            ),
            Some(true)
        );
    }
}

#[test]
fn malformed_orientation_is_lenient_and_keeps_no_support_fact() {
    for state in [
        "not json",
        "{\"upside_down_bit\":{\"type\":\"byte\",\"value\":7}}",
        "{\"minecraft:vertical_half\":{\"type\":\"string\",\"value\":\"side\"}}",
    ] {
        let record = record("minecraft:stone_stairs", ModelFamily::Stair, state);
        let mut visual = visual(VisualKind::Model);
        apply(&record, &mut visual);
        assert!(!visual.flags.contains(BlockFlags::FIRE_TOP_SUPPORT));
    }
}

#[test]
fn admitted_wool_catches_fire_without_inferring_from_wood_material() {
    let records = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    for (name, expected) in [
        ("minecraft:red_wool", true),
        ("minecraft:oak_planks", false),
    ] {
        let record = records
            .iter()
            .find(|record| record.name.as_ref() == name)
            .unwrap();
        let mut visual = visual(VisualKind::Cube);
        apply(record, &mut visual);
        assert_eq!(visual.flags.contains(BlockFlags::FIRE_FLAMMABLE), expected);
    }
}
