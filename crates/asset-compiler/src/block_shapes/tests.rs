use super::{UNIT, compile, selection};
use assets::{
    PhysicsRegistry, RegistryRecord,
    block_shapes::{PickRule, SelectionContext, ShapeFlags},
};
use std::sync::OnceLock;

mod families;
mod state_variants;

/// Loads the paired identity fixture and compiler seed once for these tests.
fn fixture() -> &'static (Box<[RegistryRecord]>, PhysicsRegistry) {
    static FIXTURE: OnceLock<(Box<[RegistryRecord]>, PhysicsRegistry)> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let breg = assets::pinned_block_registry_bytes();
        let version = assets::active_content_registry_protocol();
        let records = assets::read_registry_for_protocol(breg, version).unwrap();
        let physics = assets::read_physics_registry_for_protocol(
            include_bytes!("../../../assets/data/block-physics-seed-v2193.bin"),
            breg,
            &records,
            version,
        )
        .unwrap();
        (records, physics)
    })
}

/// Finds a fixture state by its full identifier.
fn record(name: &str) -> &'static RegistryRecord {
    fixture()
        .0
        .iter()
        .find(|record| record.name.as_ref() == name)
        .unwrap()
}

#[test]
fn selectable_states_never_silently_inherit_an_empty_movement_shape() {
    let (records, seed) = fixture();
    let missing: Vec<_> = records
        .iter()
        .filter_map(|record| {
            let output = compile(record, seed.by_sequential_id(record.sequential_id).unwrap());
            let shapes = output.shapes.unwrap();
            (shapes.pick_rule != PickRule::Never
                && shapes.selection.is_empty()
                && shapes.contextual_selection.is_empty()
                && shapes.selection_context != SelectionContext::MovingBlock)
                .then(|| format!("{} {}", record.name, record.canonical_state))
        })
        .collect();
    assert!(
        missing.is_empty(),
        "selectable states have no geometry:\n{}",
        missing.join("\n")
    );
}

#[test]
fn reviewed_foliage_routes_compile_without_changing_movement() {
    let (records, seed) = fixture();
    for record in records {
        if record.name.as_ref() == "minecraft:border_block" {
            continue;
        }
        let Some(expected) = selection::shape(record) else {
            continue;
        };
        let input = seed.by_sequential_id(record.sequential_id).unwrap();
        let output = compile(record, input);
        assert_eq!(
            output.shapes.as_ref().unwrap().selection.as_ref(),
            [expected],
            "{} {}",
            record.name,
            record.canonical_state
        );
        if record.model_family != assets::ModelFamily::Wall && !record.name.ends_with("_fence") {
            assert_eq!(output.boxes, input.boxes, "{}", record.name);
        }
    }
}

#[test]
fn malformed_snow_states_cannot_create_unbounded_geometry() {
    let mut snow = record("minecraft:snow_layer").clone();
    for height in [
        serde_json::json!(-1),
        serde_json::json!(assets::TOP_SNOW_LAYER_COUNT),
        serde_json::json!(u64::MAX),
        serde_json::Value::Null,
    ] {
        snow.canonical_state = serde_json::json!({"height": {"type": "int", "value": height}})
            .to_string()
            .into();
        assert!(selection::shape(&snow).is_none());
    }
}

#[test]
fn bamboo_offset_requires_an_admitted_primary_stalk_state() {
    let mut stalk = record("minecraft:bamboo").clone();
    let input = fixture().1.by_sequential_id(stalk.sequential_id).unwrap();
    for rejection in 0..3 {
        match rejection {
            0 => stalk.flags = assets::BlockFlags::AIR,
            1 => {
                stalk = record("minecraft:bamboo").clone();
                stalk.contributor_role = assets::ContributorRole::LiquidAdditional;
            }
            _ => {
                stalk = record("minecraft:bamboo").clone();
                stalk.canonical_state = "{}".into();
            }
        }
        assert!(
            !compile(&stalk, input)
                .shapes
                .unwrap()
                .flags
                .contains(ShapeFlags::BAMBOO_OFFSET)
        );
    }
}

#[test]
fn movement_height_does_not_define_a_class_visual_box() {
    for name in [
        "minecraft:mud",
        "minecraft:soul_sand",
        "minecraft:anvil",
        "minecraft:cactus",
        "minecraft:brewing_stand",
        "minecraft:firefly_bush",
    ] {
        let record = record(name);
        let input = fixture().1.by_sequential_id(record.sequential_id).unwrap();
        let output = compile(record, input);
        assert_eq!(output.boxes, input.boxes);
        assert_eq!(output.shapes.unwrap().selection.as_ref(), [UNIT], "{name}");
    }
}

#[test]
fn native_pick_contexts_do_not_follow_collision_or_game_mode_alone() {
    use assets::block_shapes::PickContext;
    let context = PickContext::default();
    assert!(!PickRule::HeldLight.allows(context, None));
    assert!(!PickRule::HeldLight.allows(
        PickContext {
            creative: true,
            ..context
        },
        None
    ));
    assert!(PickRule::HeldLight.allows(
        PickContext {
            holding_light: true,
            ..context
        },
        None
    ));
    assert!(!PickRule::Creative.allows(context, None));
    assert!(PickRule::Creative.allows(
        PickContext {
            creative: true,
            ..context
        },
        None
    ));
    let liquid = PickContext {
        liquids: true,
        ..context
    };
    assert!(PickRule::LiquidSource.allows(liquid, Some(0)));
    assert!(!PickRule::LiquidSource.allows(liquid, Some(1)));
    assert!(!PickRule::LiquidSource.allows(context, Some(0)));
    assert!(PickRule::Liquid.allows(liquid, Some(1)));
}

#[test]
fn multiface_clips_leave_the_unoccupied_centre_clear() {
    let mut record = record("minecraft:glow_lichen").clone();
    let input = fixture().1.by_sequential_id(record.sequential_id).unwrap();
    record.canonical_state = r#"{"multi_face_direction_bits":{"type":"byte","value":6}}"#.into();
    let shapes = compile(&record, input).shapes.unwrap();
    assert_eq!(
        shapes.selection.as_ref(),
        [
            assets::block_shapes::ShapeBox::new([0.0, 0.9375, 0.0], [1.0; 3]),
            assets::block_shapes::ShapeBox::new([0.0, 0.0, 0.9375], [1.0; 3]),
        ]
    );
    for shape in shapes.selection {
        assert!(!(0..3).all(|axis| shape.min[axis] <= 0.5 && shape.max[axis] >= 0.5));
    }
}

#[test]
fn pale_moss_empty_upper_state_and_floor_have_distinct_admission() {
    let (records, seed) = fixture();
    for record in records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:pale_moss_carpet")
    {
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let empty_sides = ["north", "east", "south", "west"]
            .iter()
            .all(|side| state[format!("pale_moss_carpet_side_{side}")]["value"] == "none");
        if !empty_sides {
            continue;
        }
        let shapes = compile(record, seed.by_sequential_id(record.sequential_id).unwrap())
            .shapes
            .unwrap();
        if state["upper_block_bit"]["value"] == 1 {
            assert_eq!(shapes.pick_rule, PickRule::Never);
            assert!(shapes.selection.is_empty());
        } else {
            assert_eq!(
                shapes.selection.as_ref(),
                [assets::block_shapes::ShapeBox::new(
                    [0.0; 3],
                    [1.0, 0.0625, 1.0]
                )]
            );
        }
    }
}

#[test]
fn visual_overrides_do_not_depend_on_the_movement_input() {
    for name in [
        "minecraft:lectern",
        "minecraft:dragon_egg",
        "minecraft:end_portal_frame",
        "minecraft:copper_chest",
        "minecraft:glass_pane",
        "minecraft:rail",
        "minecraft:mud",
        "minecraft:soul_sand",
        "minecraft:conduit",
        "minecraft:red_shrub",
        "minecraft:torch",
    ] {
        let record = record(name);
        let input = fixture().1.by_sequential_id(record.sequential_id).unwrap();
        let expected = compile(record, input).shapes.unwrap().selection;
        let mut no_collision = input.clone();
        no_collision.boxes = Box::new([]);
        assert_eq!(
            compile(record, &no_collision).shapes.unwrap().selection,
            expected,
            "{name}"
        );
    }
}

#[test]
fn selection_ownership_is_independent_of_every_movement_state() {
    for record in fixture().0.iter() {
        let input = fixture().1.by_sequential_id(record.sequential_id).unwrap();
        let expected = compile(record, input).shapes.unwrap();
        let mut changed = input.clone();
        changed.boxes = Box::new([]);
        let actual = compile(record, &changed).shapes.unwrap();
        assert_eq!(
            actual.selection, expected.selection,
            "{} {}",
            record.name, record.canonical_state
        );
        assert_eq!(
            actual.actor_visual, expected.actor_visual,
            "{}",
            record.name
        );
    }
}

#[test]
fn shelf_stored_visual_is_independent_of_its_world_facing_box() {
    for record in fixture()
        .0
        .iter()
        .filter(|record| record.name.ends_with("_shelf"))
    {
        let shapes = compile(
            record,
            fixture().1.by_sequential_id(record.sequential_id).unwrap(),
        )
        .shapes
        .unwrap();
        assert_eq!(
            shapes.actor_visual,
            Some(assets::block_shapes::ShapeBox::new(
                [0.0; 3],
                [0.3125, 1.0, 1.0]
            ))
        );
    }
}

#[test]
fn top_support_is_compiled_independently_of_collision_height() {
    for (name, expected) in [
        ("stone", true),
        ("mud", true),
        ("soul_sand", true),
        ("jigsaw", false),
        ("structure_block", false),
        ("oak_slab", false),
        ("oak_double_slab", true),
        ("farmland", false),
        ("oak_leaves", false),
        ("sculk_sensor", false),
        ("powder_snow", false),
        ("undyed_shulker_box", false),
        ("scaffolding", true),
        ("hopper", true),
        ("cauldron", true),
        ("composter", true),
    ] {
        let record = record(&format!("minecraft:{name}"));
        let shapes = compile(
            record,
            fixture().1.by_sequential_id(record.sequential_id).unwrap(),
        )
        .shapes
        .unwrap();
        assert_eq!(
            shapes.flags.contains(ShapeFlags::FULL_TOP_SUPPORT),
            expected,
            "{name}"
        );
    }
    for record in fixture()
        .0
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:oak_stairs")
    {
        let top = super::state::State::read(record)
            .unwrap()
            .bit("upside_down_bit")
            .unwrap();
        let shapes = compile(
            record,
            fixture().1.by_sequential_id(record.sequential_id).unwrap(),
        )
        .shapes
        .unwrap();
        assert_eq!(shapes.flags.contains(ShapeFlags::FULL_TOP_SUPPORT), top);
    }
}

#[test]
fn end_portal_eye_has_collision_without_extending_the_visual_shape() {
    for record in fixture()
        .0
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:end_portal_frame")
    {
        let input = fixture().1.by_sequential_id(record.sequential_id).unwrap();
        let output = compile(record, input);
        assert_eq!(output.boxes, input.boxes);
        assert_eq!(
            output.shapes.unwrap().selection.as_ref(),
            [assets::block_shapes::ShapeBox::new(
                [0.0; 3],
                [1.0, 0.8125, 1.0]
            )]
        );
    }
}

#[test]
fn pane_clipping_keeps_the_centre_extent_for_one_sided_connections() {
    let mut pane = record("minecraft:glass_pane").clone();
    pane.canonical_state=r#"{"minecraft:connection_west":{"type":"byte","value":1},"minecraft:connection_east":{"type":"byte","value":0},"minecraft:connection_north":{"type":"byte","value":0},"minecraft:connection_south":{"type":"byte","value":0}}"#.into();
    let input = fixture().1.by_sequential_id(pane.sequential_id).unwrap();
    let output = compile(&pane, input);
    assert_eq!(
        output.shapes.unwrap().selection.as_ref(),
        [assets::block_shapes::ShapeBox::new(
            [0.0, 0.0, 0.4375],
            [0.5625, 1.0, 0.5625]
        )]
    );
    assert_eq!(
        output.boxes[0].max_x,
        (assets::block_shapes::COLLISION_UNITS_PER_BLOCK / 2) as i32
    );
}

#[test]
fn vine_property_and_scaffold_full_top_are_distinct_facts() {
    for (name, vine, scaffold) in [
        ("oak_leaves", true, false),
        ("sculk_sensor", true, false),
        ("scaffolding", false, true),
        ("cauldron", false, true),
        ("composter", false, true),
        ("hopper", false, true),
        ("lectern", false, false),
        ("cactus", false, false),
    ] {
        let record = record(&format!("minecraft:{name}"));
        let shapes = compile(
            record,
            fixture().1.by_sequential_id(record.sequential_id).unwrap(),
        )
        .shapes
        .unwrap();
        assert_eq!(
            shapes.flags.contains(ShapeFlags::VINE_SUPPORT),
            vine,
            "{name}"
        );
        assert_eq!(
            shapes.flags.contains(ShapeFlags::FULL_TOP_SUPPORT),
            scaffold,
            "{name}"
        );
    }
}
