use super::*;
use assets::RegistryProvenance;

fn record(name: &str, family: ModelFamily, state: &str) -> RegistryRecord {
    RegistryRecord {
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
            canonical_upper_half(&record(name, family, &state), key),
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
fn wool_uses_a_proven_catch_component_and_does_not_infer_from_wood_names() {
    for name in [
        "minecraft:red_wool",
        "minecraft:light_blue_wool_stairs",
        "minecraft:black_wool_double_slab",
    ] {
        assert!(known_wool_flammable(name));
    }
    for name in [
        "minecraft:oak_planks",
        "minecraft:oak_log",
        "custom:red_wool",
        "minecraft:imaginary_wool",
    ] {
        assert!(!known_wool_flammable(name));
    }
}
