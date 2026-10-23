use super::{compile, fixture};
use assets::block_shapes::ShapeBox;
use assets::facts::BlockProperty;

#[test]
fn class_visual_contracts_cover_passable_and_solid_families() {
    let cases: &[(&[&str], [f32; 3], [f32; 3])] = &[
        (
            &[
                "mud",
                "soul_sand",
                "anvil",
                "cactus",
                "brewing_stand",
                "dragon_egg",
                "structure_void",
                "powder_snow",
                "firefly_bush",
                "cave_vines",
                "twisting_vines",
                "weeping_vines",
                "pale_hanging_moss",
                "end_gateway",
                "chorus_plant",
                "oak_double_slab",
                "smooth_stone_double_slab",
                "double_cut_copper_slab",
            ],
            [0.0; 3],
            [1.0; 3],
        ),
        (
            &[
                "short_grass",
                "fern",
                "deadbush",
                "oak_sapling",
                "seagrass",
                "bamboo_sapling",
                "brain_coral",
            ],
            [0.1, 0.0, 0.1],
            [0.9, 0.8, 0.9],
        ),
        (
            &[
                "dandelion",
                "poppy",
                "torchflower",
                "crimson_roots",
                "warped_roots",
                "small_dripleaf_block",
            ],
            [0.15, 0.0, 0.15],
            [0.7, 0.6, 0.7],
        ),
        (
            &["brown_mushroom", "red_mushroom"],
            [0.3, 0.0, 0.3],
            [0.7, 0.4, 0.7],
        ),
        (&["nether_sprouts"], [0.15, 0.0, 0.15], [0.85, 0.3, 0.85]),
        (
            &["short_dry_grass"],
            [0.125, 0.0, 0.125],
            [0.875, 0.625, 0.875],
        ),
        (
            &["tall_dry_grass"],
            [0.0625, 0.0, 0.0625],
            [0.9375, 1.0, 0.9375],
        ),
        (
            &["cactus_flower"],
            [0.0625, 0.0, 0.0625],
            [0.9375, 0.875, 0.9375],
        ),
        (&["reeds", "kelp"], [0.125, 0.0, 0.125], [0.875, 1.0, 0.875]),
        (
            &[
                "wheat",
                "carrots",
                "potatoes",
                "beetroot",
                "nether_wart",
                "torchflower_crop",
            ],
            [0.0; 3],
            [1.0, 0.25, 1.0],
        ),
        (&["bush"], [0.0; 3], [1.0, 0.8, 1.0]),
        (&["hanging_roots"], [0.15, 0.7, 0.15], [0.85, 1.0, 0.85]),
        (&["spore_blossom"], [0.15, 0.75, 0.15], [0.85, 1.0, 0.85]),
        (&["frog_spawn"], [0.0; 3], [1.0, 0.015625, 1.0]),
        (
            &["pink_petals", "wildflowers"],
            [0.0; 3],
            [1.0, 0.1875, 1.0],
        ),
        (
            &["leaf_litter", "redstone_wire", "moss_carpet"],
            [0.0; 3],
            [1.0, 0.0625, 1.0],
        ),
        (&["sea_pickle"], [0.0; 3], [1.0, 0.5, 1.0]),
        (&["farmland", "grass_path"], [0.0; 3], [1.0, 0.9375, 1.0]),
        (&["honey_block"], [0.0625, 0.0, 0.0625], [0.9375; 3]),
        (&["lectern"], [0.0; 3], [1.0, 0.9, 1.0]),
        (
            &["conduit", "heavy_core"],
            [0.25, 0.0, 0.25],
            [0.75, 0.5, 0.75],
        ),
        (&["end_portal_frame"], [0.0; 3], [1.0, 0.8125, 1.0]),
        (
            &["chest", "trapped_chest", "ender_chest", "copper_chest"],
            [0.025, 0.0, 0.025],
            [0.975, 0.95, 0.975],
        ),
        (&["bed", "stonecutter_block"], [0.0; 3], [1.0, 0.5625, 1.0]),
        (
            &["flower_pot"],
            [0.3125, 0.0, 0.3125],
            [0.6875, 0.375, 0.6875],
        ),
        (&["sniffer_egg"], [0.0625, 0.0, 0.125], [0.9375, 1.0, 0.875]),
        (
            &["waterlily"],
            [0.0625, 0.0, 0.0625],
            [0.9375, 0.09375, 0.9375],
        ),
        (&["end_portal"], [0.0; 3], [1.0, 0.25, 1.0]),
        (&["enchanting_table"], [0.0; 3], [1.0, 0.75, 1.0]),
        (
            &["daylight_detector", "daylight_detector_inverted"],
            [0.0; 3],
            [1.0, 0.375, 1.0],
        ),
        (
            &["decorated_pot"],
            [0.0625, 0.0, 0.0625],
            [0.9375, 1.0, 0.9375],
        ),
        (
            &["dried_ghast"],
            [0.1875, 0.0, 0.1875],
            [0.8125, 0.625, 0.8125],
        ),
        (
            &["sculk_sensor", "calibrated_sculk_sensor"],
            [0.0; 3],
            [1.0, 0.5, 1.0],
        ),
        (&["campfire", "soul_campfire"], [0.0; 3], [1.0, 0.4375, 1.0]),
        (
            &[
                "powered_repeater",
                "unpowered_repeater",
                "powered_comparator",
                "unpowered_comparator",
            ],
            [0.0; 3],
            [1.0, 0.125, 1.0],
        ),
        (
            &["candle_cake", "black_candle_cake"],
            [0.0625, 0.0, 0.0625],
            [0.9375, 0.5, 0.9375],
        ),
    ];
    for &(names, min, max) in cases {
        let expected = ShapeBox::new(min.map(f64::from), max.map(f64::from));
        for name in names {
            let name = format!("minecraft:{name}");
            let matching: Vec<_> = fixture()
                .0
                .iter()
                .filter(|record| record.name.as_ref() == name)
                .collect();
            assert!(!matching.is_empty(), "missing family fixture {name}");
            for record in matching {
                let mut input = fixture()
                    .1
                    .by_sequential_id(record.sequential_id)
                    .unwrap()
                    .clone();
                input.boxes = Box::new([]);
                assert_eq!(
                    compile(record, &input).shapes.unwrap().selection.as_ref(),
                    [expected],
                    "{name} {}",
                    record.canonical_state
                );
            }
        }
    }
}

#[test]
fn stair_picking_ignores_corner_collision_state() {
    let matching: Vec<_> = fixture()
        .0
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:oak_stairs")
        .collect();
    for record in matching {
        let state = super::super::state::State::read(record).unwrap();
        let direction = state.number(BlockProperty::WeirdoDirection).unwrap();
        let top = state.bit(BlockProperty::UpsideDownBit).unwrap();
        let shapes = compile(
            record,
            fixture().1.by_sequential_id(record.sequential_id).unwrap(),
        )
        .shapes
        .unwrap();
        assert_eq!(shapes.selection.len(), 2);
        let low = shapes
            .selection
            .iter()
            .filter(|shape| shape.min[1] == 0.0)
            .count();
        assert_eq!(low, 1);
        let absent = match direction {
            0 => [0.25, if top { 0.25 } else { 0.75 }, 0.25],
            1 => [0.75, if top { 0.25 } else { 0.75 }, 0.25],
            2 => [0.25, if top { 0.25 } else { 0.75 }, 0.25],
            3 => [0.25, if top { 0.25 } else { 0.75 }, 0.75],
            _ => unreachable!(),
        };
        assert!(!shapes.selection.iter().any(|shape| {
            (0..3).all(|axis| shape.min[axis] < absent[axis] && absent[axis] < shape.max[axis])
        }));
    }
}

#[test]
fn end_rod_selection_follows_the_facing_axis() {
    let expected = [
        ([0.375, 0.0, 0.375], [0.625, 1.0, 0.625]),
        ([0.375, 0.0, 0.375], [0.625, 1.0, 0.625]),
        ([0.375, 0.375, 0.0], [0.625, 0.625, 1.0]),
        ([0.375, 0.375, 0.0], [0.625, 0.625, 1.0]),
        ([0.0, 0.375, 0.375], [1.0, 0.625, 0.625]),
        ([0.0, 0.375, 0.375], [1.0, 0.625, 0.625]),
    ];
    for record in fixture()
        .0
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:end_rod")
    {
        let facing = super::super::state::State::read(record)
            .unwrap()
            .number(BlockProperty::FacingDirection)
            .unwrap() as usize;
        let input = fixture().1.by_sequential_id(record.sequential_id).unwrap();
        assert_eq!(
            compile(record, input).shapes.unwrap().selection.as_ref(),
            [ShapeBox::new(
                expected[facing].0.map(f64::from),
                expected[facing].1.map(f64::from),
            )],
            "{} {}",
            record.name,
            record.canonical_state
        );
    }
}

#[test]
fn lightning_rod_selection_follows_the_facing_axis() {
    let rods: Vec<_> = fixture()
        .0
        .iter()
        .filter(|record| {
            assets::facts::block_render_fact(&record.name).kind
                == assets::facts::BlockRenderKind::LightningRod
        })
        .collect();
    assert!(!rods.is_empty());
    let mut facings = std::collections::BTreeSet::new();
    for record in rods {
        let facing = super::super::state::State::read(record)
            .unwrap()
            .number(BlockProperty::FacingDirection)
            .unwrap();
        let (min, max) = match facing {
            0 | 1 => ([0.375, 0.0, 0.375], [0.625, 1.0, 0.625]),
            2 | 3 => ([0.375, 0.375, 0.0], [0.625, 0.625, 1.0]),
            4 | 5 => ([0.0, 0.375, 0.375], [1.0, 0.625, 0.625]),
            _ => panic!("invalid lightning rod facing {facing}"),
        };
        let shapes = compile(
            record,
            fixture().1.by_sequential_id(record.sequential_id).unwrap(),
        )
        .shapes
        .unwrap();
        assert_eq!(
            shapes.selection.as_ref(),
            [ShapeBox::new(min.map(f64::from), max.map(f64::from))],
            "{} {}",
            record.name,
            record.canonical_state
        );
        facings.insert(facing);
    }
    assert_eq!(facings, (0..=5).collect());
}
