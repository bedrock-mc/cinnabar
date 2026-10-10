use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::json;
use wasmtime::Trap;
use wasmtime::component::{Component, Linker};

use super::{axes, engine, validate_blocks, validate_registration, wit};
use crate::host::{Api, HostState};
use crate::limits::{
    MAX_BLOCK_NAME_BYTES, MAX_BLOCKS, MAX_BONES, MAX_CONDITION_TESTS, MAX_DISPLAY_NAME_BYTES,
    MAX_ITEMS, MAX_PERMUTATIONS, MAX_STACK_SIZE, MAX_STATE_VALUES,
};
use crate::manifest::{ASSETS_DIR, DATA_SCHEMA, Manifest, SERVER_WASM};
use crate::protocol::{ItemDef, PlacementState, StateDef, StateValues};

/// The ticker keeps the epoch on wall time, so a store with fuel to spare still stops at its
/// deadline, and not long before or after it.
#[test]
fn epoch_deadline_stops_a_store_with_fuel_left() {
    const DEADLINE: Duration = Duration::from_millis(200);
    // Seconds of spinning: a stalled epoch ends in a fuel trap instead of a hang.
    const FUEL: u64 = 10_000_000_000;
    let (engine, _ticker) = engine().unwrap();
    let spin = r#"(component
        (core module $m (func (export "spin") (loop (br 0))))
        (core instance $i (instantiate $m))
        (func (export "spin") (canon lift (core func $i "spin"))))"#;
    let component = Component::new(&engine, spin).unwrap();
    let mut store = HostState::store(&engine, "spin", FUEL, DEADLINE).unwrap();
    let instance = Linker::new(&engine)
        .instantiate(&mut store, &component)
        .unwrap();
    let spin = instance
        .get_typed_func::<(), ()>(&mut store, "spin")
        .unwrap();
    let start = Instant::now();
    let error = spin.call(&mut store, ()).unwrap_err();
    let elapsed = start.elapsed();
    assert_eq!(
        error.downcast_ref::<Trap>(),
        Some(&Trap::Interrupt),
        "{error:#}"
    );
    assert!(
        elapsed >= DEADLINE / 2 && elapsed < DEADLINE * 5,
        "stopped after {elapsed:?}"
    );
}

fn binding(slot: &str) -> wit::TextureBinding {
    wit::TextureBinding {
        slot: slot.to_owned(),
        path: "counter.png".to_owned(),
    }
}

/// A valid block: `probe:<name>` with the indexed texture bound to `*`.
fn block(name: &str) -> wit::BlockDef {
    wit::BlockDef {
        id: format!("probe:{name}"),
        display_name: "Probe Counter".to_owned(),
        textures: vec![binding("*")],
        mining: wit::Mining::Breakable(1.0),
    }
}

/// A cube block type of `def`, as every world before 0.5 declares its blocks.
fn cube(def: wit::BlockDef) -> wit::BlockType {
    wit::BlockType {
        def,
        states: Vec::new(),
        placement: wit::PlacementStates::empty(),
        visual: None,
        permutations: Vec::new(),
        network: false,
    }
}

/// `count` valid blocks with distinct names.
fn blocks(count: usize) -> Vec<wit::BlockType> {
    (0..count).map(|i| cube(block(&format!("b{i}")))).collect()
}

/// The valid block `probe:counter` after `edit`.
fn counter(edit: impl FnOnce(&mut wit::BlockDef)) -> Vec<wit::BlockType> {
    let mut def = block("counter");
    edit(&mut def);
    vec![cube(def)]
}

/// The assets the rule tests index, written to `dir`: the 1×1 `counter.png`, the flipbook strips
/// `strip.png` (16×48, three frames) and `ragged.png` (16×40), and the geometries
/// `cable.geo.json` (`geometry.probe.cable`: bones `core`, `north` and `slot_0` to
/// `slot_64`, drawn with `base` and the faces of `core`'s box UV), `alt.geo.json` (another
/// file with `geometry.probe.cable`), `foreign.geo.json` (`geometry.other.cable`), `two.geo.json`
/// (two geometries) and `post.geo.json` (`geometry.probe.post`, drawn with `post` alone).
fn rule_assets(dir: &Path) -> Manifest {
    let assets = dir.join(ASSETS_DIR);
    std::fs::create_dir_all(&assets).unwrap();
    let png = |width: u32, height: u32| {
        let mut header = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        header.extend(width.to_be_bytes());
        header.extend(height.to_be_bytes());
        header.extend([8, 6, 0, 0, 0]);
        header
    };
    let geometry = |identifier: &str, bones: serde_json::Value| {
        json!({
            "format_version": "1.21.0",
            "minecraft:geometry": [{
                "description": { "identifier": identifier },
                "bones": bones,
            }],
        })
    };
    let cable_bones = || {
        let mut bones = vec![
            json!({ "name": "core", "cubes": [{ "origin": [-2, 6, -2], "size": [4, 4, 4], "uv": [0, 0] }] }),
            json!({ "name": "north", "cubes": [{ "uv": {
                "north": { "uv": [0, 0], "material_instance": "base" },
                "up": { "uv": [0, 0], "material_instance": "base" },
            } }] }),
        ];
        bones.extend((0..=MAX_BONES).map(|i| json!({ "name": format!("slot_{i}") })));
        serde_json::Value::Array(bones)
    };
    let post = json!([{ "name": "post", "cubes": [{ "uv": {
        "up": { "uv": [0, 0], "material_instance": "post" },
    } }] }]);
    let mut two = geometry("geometry.probe.two", json!([]));
    let second = two["minecraft:geometry"][0].clone();
    two["minecraft:geometry"]
        .as_array_mut()
        .unwrap()
        .push(second);
    let files: Vec<(&str, Vec<u8>)> = vec![
        ("counter.png", png(1, 1)),
        ("strip.png", png(16, 48)),
        ("ragged.png", png(16, 40)),
        (
            "cable.geo.json",
            geometry("geometry.probe.cable", cable_bones())
                .to_string()
                .into(),
        ),
        (
            "alt.geo.json",
            geometry("geometry.probe.cable", json!([]))
                .to_string()
                .into(),
        ),
        (
            "foreign.geo.json",
            geometry("geometry.other.cable", json!([]))
                .to_string()
                .into(),
        ),
        ("two.geo.json", two.to_string().into()),
        (
            "post.geo.json",
            geometry("geometry.probe.post", post).to_string().into(),
        ),
    ];
    let mut index: Vec<String> = vec![SERVER_WASM.to_owned()];
    for (name, bytes) in files {
        std::fs::write(assets.join(name), bytes).unwrap();
        index.push(format!("{ASSETS_DIR}/{name}"));
    }
    // `validate_blocks` consults only the index's paths, not the files' hashes.
    Manifest {
        id: "probe".to_owned(),
        version: "0.1.0".to_owned(),
        api: Api::V0_2.api().to_owned(),
        data_schema: DATA_SCHEMA,
        files: index
            .into_iter()
            .map(|path| (path, "0".repeat(64)))
            .collect(),
    }
}

/// Runs each case through `validate_blocks` against [`rule_assets`]. `None` means it is
/// accepted; otherwise the refusal must contain the cause and, for a single block, name that
/// block.
fn check_rules(cases: Vec<(&str, Vec<wit::BlockType>, Option<&str>)>) {
    let dir = tempfile::tempdir().unwrap();
    let manifest = rule_assets(dir.path());
    let mut failures = Vec::new();
    for (case, types, refusal) in cases {
        let culprit = match types.as_slice() {
            [block] => Some(format!("block \"{}\"", block.def.id)),
            _ => None,
        };
        let outcome = validate_blocks(dir.path(), &manifest, types);
        match (outcome.map_err(|error| format!("{error:#}")), refusal) {
            (Ok(_), None) => {}
            (Ok(_), Some(cause)) => failures.push(format!("{case}: accepted, not {cause:?}")),
            (Err(error), None) => failures.push(format!("{case}: refused: {error}")),
            (Err(error), Some(cause)) => {
                let named = culprit.is_none_or(|culprit| error.contains(&culprit));
                if !named || !error.contains(cause) {
                    failures.push(format!("{case}: {error:?} lacks the block or {cause:?}"));
                }
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// Each row changes a valid declaration in one way.
#[test]
fn block_rules_accept_limits_and_refuse_violations() {
    let faces = || ["up", "down", "north", "south", "east", "west"].map(binding);
    check_rules(vec![
        ("the baseline", counter(|_| {}), None),
        ("MAX_BLOCKS blocks", blocks(MAX_BLOCKS), None),
        (
            "MAX_BLOCKS + 1 blocks",
            blocks(MAX_BLOCKS + 1),
            Some("the limit is"),
        ),
        (
            "a duplicate id",
            vec![cube(block("counter")), cube(block("counter"))],
            Some("declared twice"),
        ),
        (
            "a foreign namespace",
            counter(|def| def.id = "other:counter".to_owned()),
            Some("outside namespace"),
        ),
        (
            "a name of digits and _",
            vec![cube(block("cell_64k"))],
            None,
        ),
        (
            "a name of MAX_BLOCK_NAME_BYTES",
            vec![cube(block(&"a".repeat(MAX_BLOCK_NAME_BYTES)))],
            None,
        ),
        (
            "a name of MAX_BLOCK_NAME_BYTES + 1",
            vec![cube(block(&"a".repeat(MAX_BLOCK_NAME_BYTES + 1)))],
            Some("invalid name"),
        ),
        ("an empty name", vec![cube(block(""))], Some("invalid name")),
        (
            "a name with /",
            vec![cube(block("a/b"))],
            Some("invalid name"),
        ),
        ("the name ..", vec![cube(block(".."))], Some("invalid name")),
        (
            "an uppercase name",
            vec![cube(block("Counter"))],
            Some("invalid name"),
        ),
        (
            "an empty display name",
            counter(|def| def.display_name.clear()),
            Some("display name has"),
        ),
        (
            "a display name of MAX_DISPLAY_NAME_BYTES",
            counter(|def| def.display_name = "a".repeat(MAX_DISPLAY_NAME_BYTES)),
            None,
        ),
        (
            "a display name of MAX_DISPLAY_NAME_BYTES + 1",
            counter(|def| def.display_name = "a".repeat(MAX_DISPLAY_NAME_BYTES + 1)),
            Some("display name has"),
        ),
        (
            "a newline in the display name",
            counter(|def| def.display_name = "Probe\nCounter".to_owned()),
            Some("control character"),
        ),
        (
            "the slot top",
            counter(|def| def.textures.push(binding("top"))),
            Some("slot \"top\""),
        ),
        (
            "a repeated slot",
            counter(|def| def.textures.push(binding("*"))),
            Some("bound twice"),
        ),
        (
            "an unindexed texture",
            counter(|def| def.textures[0].path = "missing.png".to_owned()),
            Some("not an indexed file"),
        ),
        (
            "a texture outside assets/",
            counter(|def| def.textures[0].path = "../server.wasm".to_owned()),
            Some("not an indexed file"),
        ),
        (
            "no textures",
            counter(|def| def.textures.clear()),
            Some("bind neither"),
        ),
        (
            "five faces without *",
            counter(|def| def.textures = faces().into_iter().take(5).collect()),
            Some("bind neither"),
        ),
        (
            "six faces without *",
            counter(|def| def.textures = faces().into()),
            None,
        ),
        (
            "* and a face",
            counter(|def| def.textures.push(binding("up"))),
            None,
        ),
        (
            "hardness 0",
            counter(|def| def.mining = wit::Mining::Breakable(0.0)),
            None,
        ),
        (
            "unbreakable",
            counter(|def| def.mining = wit::Mining::Unbreakable),
            None,
        ),
        (
            "hardness NaN",
            counter(|def| def.mining = wit::Mining::Breakable(f32::NAN)),
            Some("not a finite number"),
        ),
        (
            "hardness inf",
            counter(|def| def.mining = wit::Mining::Breakable(f32::INFINITY)),
            Some("not a finite number"),
        ),
        (
            "hardness -1",
            counter(|def| def.mining = wit::Mining::Breakable(-1.0)),
            Some("not a finite number"),
        ),
    ]);
}

fn bool_state(name: &str) -> wit::StateDef {
    wit::StateDef {
        name: format!("probe:{name}"),
        values: wit::StateValues::Bool,
    }
}

fn choice_state(name: &str, values: &[&str]) -> wit::StateDef {
    wit::StateDef {
        name: format!("probe:{name}"),
        values: wit::StateValues::Choices(values.iter().map(|&value| value.to_owned()).collect()),
    }
}

/// `state == value` when `equal`, else `state != value`.
fn test(state: &str, value: wit::StateValue, equal: bool) -> wit::StateTest {
    wit::StateTest {
        state: wit::BlockState {
            name: state.to_owned(),
            value,
        },
        equal,
    }
}

fn is(state: &str, value: bool) -> wit::StateTest {
    test(
        &format!("probe:{state}"),
        wit::StateValue::Bool(value),
        true,
    )
}

fn choice(value: &str) -> wit::StateValue {
    wit::StateValue::Choice(value.to_owned())
}

fn material(instance: &str, path: &str) -> wit::Material {
    wit::Material {
        instance: instance.to_owned(),
        path: path.to_owned(),
        render_method: wit::RenderMethod::AlphaTest,
        flipbook: None,
    }
}

fn pixel_box(min: f32, max: f32) -> wit::PixelBox {
    let pixel = |v: f32| wit::Pixel { x: v, y: v, z: v };
    wit::PixelBox {
        min: pixel(min),
        max: pixel(max),
    }
}

fn permutation(when: wit::Condition) -> wit::Permutation {
    wit::Permutation {
        when,
        geometry: None,
        materials: None,
        bones: None,
        collision: None,
        selection: None,
        rotation: None,
    }
}

fn turns(x: u8, y: u8, z: u8) -> wit::QuarterTurns {
    wit::QuarterTurns { x, y, z }
}

/// A valid cable shaped like the converter's: six connection states, a facing trait, the cable
/// geometry with a bone shown by a condition over two states, a flipbook, boxes, and a
/// permutation that turns it by the trait's state and one that swaps its geometry.
fn cable() -> wit::BlockType {
    let mut strip = material("base", "strip.png");
    strip.flipbook = Some(wit::Flipbook {
        ticks_per_frame: 25,
        frames: vec![0, 1, 2, 1],
        blend_frames: true,
    });
    let mut turned = permutation(vec![vec![test(
        "minecraft:facing_direction",
        choice("east"),
        true,
    )]]);
    turned.rotation = Some(turns(0, 1, 0));
    let mut post = permutation(vec![vec![is("north", false), is("up", true)]]);
    post.geometry = Some("post.geo.json".to_owned());
    post.materials = Some(vec![material("post", "counter.png")]);
    wit::BlockType {
        def: wit::BlockDef {
            textures: Vec::new(),
            ..block("cable")
        },
        states: ["down", "up", "north", "south", "west", "east"]
            .map(bool_state)
            .into(),
        placement: wit::PlacementStates::FACING_DIRECTION,
        visual: Some(wit::Visual {
            geometry: Some("cable.geo.json".to_owned()),
            materials: vec![strip, material("*", "counter.png")],
            bones: vec![wit::BoneVisibility {
                bone: "north".to_owned(),
                visible: vec![vec![is("north", true)], vec![is("south", false)]],
            }],
            collision: Some(pixel_box(6.0, 10.0)),
            selection: Some(pixel_box(0.0, 16.0)),
            rotation: None,
        }),
        permutations: vec![turned, post],
        network: false,
    }
}

/// The valid cable after `edit`.
fn cable_with(edit: impl FnOnce(&mut wit::BlockType)) -> Vec<wit::BlockType> {
    let mut block = cable();
    edit(&mut block);
    vec![block]
}

fn visual(block: &mut wit::BlockType) -> &mut wit::Visual {
    block.visual.as_mut().unwrap()
}

/// Each row changes the valid cable, or a counter, in one way: the WIT 0.5 refusals of the SP5
/// gate (an unknown state in a condition, over 16 values, a geometry outside the namespace) and
/// every other bound on states, visuals and permutations.
#[test]
fn block_type_rules_accept_limits_and_refuse_violations() {
    let many_bools = |count: usize| {
        cable_with(|block| {
            block.placement = wit::PlacementStates::empty();
            block.states = (0..count).map(|i| bool_state(&format!("s{i}"))).collect();
            block.permutations.clear();
            visual(block).bones.clear();
        })
    };
    let values = |count: usize| (0..count).map(|i| format!("v{i}")).collect::<Vec<_>>();
    let with_values = |count: usize| {
        cable_with(|block| {
            block.states.push(wit::StateDef {
                name: "probe:colour".to_owned(),
                values: wit::StateValues::Choices(values(count)),
            });
        })
    };
    let slot_bones = |count: usize| {
        cable_with(|block| {
            visual(block).bones = (0..count)
                .map(|i| wit::BoneVisibility {
                    bone: format!("slot_{i}"),
                    visible: vec![vec![is("north", true)]],
                })
                .collect();
        })
    };
    check_rules(vec![
        ("the cable", vec![cable()], None),
        (
            "a counter with states and traits but no visual",
            vec![wit::BlockType {
                states: vec![bool_state("on")],
                placement: wit::PlacementStates::all(),
                ..cube(block("counter"))
            }],
            None,
        ),
        (
            "a state outside the namespace",
            cable_with(|block| block.states[0].name = "other:down".to_owned()),
            Some("is not probe:<name>"),
        ),
        (
            "a state named in uppercase",
            cable_with(|block| block.states[0].name = "probe:Down".to_owned()),
            Some("is not probe:<name>"),
        ),
        (
            "a state declared twice",
            cable_with(|block| block.states.push(bool_state("down"))),
            Some("declared twice"),
        ),
        (
            "MAX_STATE_VALUES values",
            with_values(MAX_STATE_VALUES),
            None,
        ),
        (
            "MAX_STATE_VALUES + 1 values",
            with_values(MAX_STATE_VALUES + 1),
            Some("has 17 values"),
        ),
        ("no values", with_values(0), Some("has 0 values")),
        (
            "a value twice",
            cable_with(|block| block.states.push(choice_state("colour", &["red", "red"]))),
            Some("twice"),
        ),
        (
            "a value with a quote",
            cable_with(|block| block.states.push(choice_state("colour", &["it's"]))),
            Some("does not match"),
        ),
        ("2^16 combinations", many_bools(16), None),
        (
            "2^17 combinations",
            many_bools(17),
            Some("more than 65536 combinations"),
        ),
        (
            "2^16 combinations and a trait",
            cable_with(|block| {
                block.states = (0..16).map(|i| bool_state(&format!("s{i}"))).collect();
                block.permutations.clear();
                visual(block).bones.clear();
            }),
            Some("more than 65536 combinations"),
        ),
        (
            "a condition on a state the block lacks",
            cable_with(|block| {
                visual(block).bones[0].visible = vec![vec![is("sideways", true)]];
            }),
            Some("does not have"),
        ),
        (
            "a condition on a trait the block lacks",
            cable_with(|block| block.placement = wit::PlacementStates::BLOCK_FACE),
            Some("\"minecraft:facing_direction\", which the block does not have"),
        ),
        (
            "a bool state tested against a choice",
            cable_with(|block| {
                visual(block).bones[0].visible =
                    vec![vec![test("probe:north", choice("yes"), true)]];
            }),
            Some("a value it does not take"),
        ),
        (
            "a trait state tested against a value it lacks",
            cable_with(|block| {
                block.permutations[0].when = vec![vec![test(
                    "minecraft:facing_direction",
                    choice("sideways"),
                    true,
                )]];
            }),
            Some("a value it does not take"),
        ),
        (
            "MAX_CONDITION_TESTS tests",
            cable_with(|block| {
                visual(block).bones[0].visible = vec![
                    (0..MAX_CONDITION_TESTS)
                        .map(|_| is("north", true))
                        .collect(),
                ];
            }),
            None,
        ),
        (
            "MAX_CONDITION_TESTS + 1 tests",
            cable_with(|block| {
                visual(block).bones[0].visible = vec![
                    (0..=MAX_CONDITION_TESTS)
                        .map(|_| is("north", true))
                        .collect(),
                ];
            }),
            Some("65 tests"),
        ),
        (
            "a geometry outside the namespace",
            cable_with(|block| visual(block).geometry = Some("foreign.geo.json".to_owned())),
            Some("outside namespace \"geometry.probe.\""),
        ),
        (
            "an unindexed geometry",
            cable_with(|block| visual(block).geometry = Some("missing.geo.json".to_owned())),
            Some("not an indexed file"),
        ),
        (
            "a file of two geometries",
            cable_with(|block| visual(block).geometry = Some("two.geo.json".to_owned())),
            Some("holds 2 geometries"),
        ),
        (
            "one identifier in two files",
            cable_with(|block| {
                block.permutations[1].geometry = Some("alt.geo.json".to_owned());
            }),
            Some("in two files"),
        ),
        (
            "a bone the geometry lacks",
            cable_with(|block| visual(block).bones[0].bone = "south".to_owned()),
            Some("has no bone \"south\""),
        ),
        ("MAX_BONES bones", slot_bones(MAX_BONES), None),
        (
            "MAX_BONES + 1 bones",
            slot_bones(MAX_BONES + 1),
            Some("65 bones"),
        ),
        (
            "a bone shown twice",
            cable_with(|block| {
                let bone = visual(block).bones[0].clone();
                visual(block).bones.push(bone);
            }),
            Some("two visibilities"),
        ),
        (
            "a bone on the full cube",
            cable_with(|block| {
                visual(block).geometry = None;
                visual(block).materials.remove(0);
                block.permutations.truncate(1);
            }),
            Some("the full cube has no bones"),
        ),
        (
            "an instance the geometry does not draw with",
            cable_with(|block| visual(block).materials.push(material("lid", "counter.png"))),
            Some("material instance \"lid\" is neither"),
        ),
        (
            "an instance twice",
            cable_with(|block| visual(block).materials.push(material("*", "counter.png"))),
            Some("listed twice"),
        ),
        (
            "an instance without a material and no *",
            cable_with(|block| visual(block).materials.truncate(1)),
            Some("has no material"),
        ),
        (
            "an unindexed material",
            cable_with(|block| visual(block).materials[1].path = "missing.png".to_owned()),
            Some("not an indexed file"),
        ),
        (
            "no materials",
            cable_with(|block| visual(block).materials.clear()),
            Some("it lists 0 materials"),
        ),
        (
            "a ragged flipbook strip",
            cable_with(|block| visual(block).materials[0].path = "ragged.png".to_owned()),
            Some("not whole square frames"),
        ),
        (
            "a flipbook frame past the strip",
            cable_with(|block| {
                visual(block).materials[0].flipbook.as_mut().unwrap().frames = vec![0, 3];
            }),
            Some("shows frame 3 of a strip of 3"),
        ),
        (
            "a flipbook frame of 0 ticks",
            cable_with(|block| {
                visual(block).materials[0]
                    .flipbook
                    .as_mut()
                    .unwrap()
                    .ticks_per_frame = 0;
            }),
            Some("0 ticks"),
        ),
        (
            "a flipbook of every frame",
            cable_with(|block| {
                visual(block).materials[0].flipbook.as_mut().unwrap().frames = Vec::new();
            }),
            None,
        ),
        (
            "a box past the block",
            cable_with(|block| visual(block).collision = Some(pixel_box(0.0, 17.0))),
            Some("collision box spans 0 to 17"),
        ),
        (
            "an empty box",
            cable_with(|block| visual(block).selection = Some(pixel_box(4.0, 4.0))),
            Some("selection box spans 4 to 4"),
        ),
        (
            "a box at NaN",
            cable_with(|block| visual(block).collision = Some(pixel_box(f32::NAN, 4.0))),
            Some("collision box spans NaN"),
        ),
        (
            "zero rotation after a matching turned permutation",
            cable_with(|block| {
                let mut reset = block.permutations[0].clone();
                reset.rotation = Some(turns(0, 0, 0));
                block.permutations.push(reset);
            }),
            Some("zero rotation cannot replace an earlier permutation"),
        ),
        (
            "zero rotation after a disjoint turned permutation",
            cable_with(|block| {
                let mut reset = block.permutations[0].clone();
                reset.when = vec![vec![test(
                    "minecraft:facing_direction",
                    choice("west"),
                    true,
                )]];
                reset.rotation = Some(turns(0, 0, 0));
                block.permutations.push(reset);
            }),
            None,
        ),
        (
            "zero rotation after a negated disjoint turned permutation",
            cable_with(|block| {
                let mut reset = block.permutations[0].clone();
                reset.when = vec![vec![test(
                    "minecraft:facing_direction",
                    choice("east"),
                    false,
                )]];
                reset.rotation = Some(turns(0, 0, 0));
                block.permutations.push(reset);
            }),
            None,
        ),
        (
            "four quarter turns",
            cable_with(|block| visual(block).rotation = Some(turns(0, 4, 0))),
            Some("each axis takes 0 to 3"),
        ),
        (
            "textures beside a visual",
            cable_with(|block| block.def.textures = vec![binding("*")]),
            Some("binds textures and declares a visual"),
        ),
        (
            "permutations without a visual",
            vec![wit::BlockType {
                states: vec![bool_state("on")],
                permutations: vec![wit::Permutation {
                    rotation: Some(turns(0, 1, 0)),
                    ..permutation(vec![vec![is("on", true)]])
                }],
                ..cube(block("counter"))
            }],
            Some("no visual"),
        ),
        (
            "a permutation that never holds",
            cable_with(|block| block.permutations[0].when.clear()),
            Some("never holds"),
        ),
        (
            "a permutation whose clause always holds",
            cable_with(|block| block.permutations[0].when = vec![Vec::new()]),
            None,
        ),
        (
            "a permutation that sets nothing",
            cable_with(|block| block.permutations[0].rotation = None),
            Some("sets nothing"),
        ),
        (
            "MAX_PERMUTATIONS permutations",
            cable_with(|block| block.permutations = vec![cable().permutations[0].clone(); 64]),
            None,
        ),
        (
            "MAX_PERMUTATIONS + 1 permutations",
            cable_with(|block| {
                block.permutations = vec![cable().permutations[0].clone(); MAX_PERMUTATIONS + 1];
            }),
            Some("65 permutations"),
        ),
        (
            "a permutation geometry the materials do not cover",
            cable_with(|block| {
                visual(block).materials.truncate(1);
                let faces = ["up", "down", "north", "south", "east", "west"];
                let faces = faces.map(|face| material(face, "counter.png"));
                visual(block).materials.extend(faces);
                block.permutations[1].materials = None;
            }),
            Some("material instance \"post\" is drawn but has no material"),
        ),
        (
            "a permutation bone of its own geometry",
            cable_with(|block| {
                block.permutations[1].bones = Some(vec![wit::BoneVisibility {
                    bone: "post".to_owned(),
                    visible: vec![vec![is("up", true)]],
                }]);
            }),
            None,
        ),
        (
            "a permutation bone of the visual's geometry under its own",
            cable_with(|block| {
                block.permutations[1].bones = Some(vec![wit::BoneVisibility {
                    bone: "north".to_owned(),
                    visible: Vec::new(),
                }]);
            }),
            Some("geometry \"geometry.probe.post\" has no bone \"north\""),
        ),
        (
            "a zero permutation rotation under a turned visual",
            cable_with(|block| {
                visual(block).rotation = Some(turns(0, 2, 0));
                block.permutations[0].rotation = Some(turns(0, 0, 0));
            }),
            Some("cannot replace the visual's rotation"),
        ),
        (
            "a network member",
            cable_with(|block| block.network = true),
            None,
        ),
    ]);
}

fn item(name: &str) -> wit::ItemDef {
    wit::ItemDef {
        id: format!("probe:{name}"),
        display_name: "Probe Cell".to_owned(),
        icon: "counter.png".to_owned(),
        max_stack: 1,
    }
}

/// Each row changes a valid registration of one block and one item in one way. Items are
/// `<id>:<name>` like blocks, distinct from every block, with an indexed icon, a display name like
/// a block's, and 1 to MAX_STACK_SIZE to a stack, at most MAX_ITEMS of them.
#[test]
fn item_rules_accept_limits_and_refuse_violations() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = rule_assets(dir.path());
    let items = |count: usize| {
        (0..count)
            .map(|i| item(&format!("i{i}")))
            .collect::<Vec<_>>()
    };
    let with = |edit: fn(&mut wit::ItemDef)| {
        let mut cell = item("cell");
        edit(&mut cell);
        vec![cell]
    };
    let cases: Vec<(&str, Vec<wit::ItemDef>, Option<&str>)> = vec![
        ("the cell", with(|_| {}), None),
        ("MAX_ITEMS items", items(MAX_ITEMS), None),
        (
            "MAX_ITEMS + 1 items",
            items(MAX_ITEMS + 1),
            Some("the limit is"),
        ),
        (
            "a foreign namespace",
            with(|i| i.id = "other:cell".to_owned()),
            Some("outside namespace"),
        ),
        (
            "an invalid name",
            with(|i| i.id = "probe:Cell".to_owned()),
            Some("invalid name"),
        ),
        (
            "an item twice",
            vec![item("cell"), item("cell")],
            Some("declared twice"),
        ),
        (
            "an item with a block's id",
            with(|i| i.id = "probe:cable".to_owned()),
            Some("is also a block"),
        ),
        (
            "an empty display name",
            with(|i| i.display_name.clear()),
            Some("display name has"),
        ),
        (
            "an unindexed icon",
            with(|i| i.icon = "missing.png".to_owned()),
            Some("not an indexed file"),
        ),
        ("a stack of 0", with(|i| i.max_stack = 0), Some("1 to 64")),
        (
            "a stack of MAX_STACK_SIZE",
            with(|i| i.max_stack = MAX_STACK_SIZE),
            None,
        ),
        (
            "a stack of MAX_STACK_SIZE + 1",
            with(|i| i.max_stack = MAX_STACK_SIZE + 1),
            Some("1 to 64"),
        ),
    ];
    let mut failures = Vec::new();
    for (case, items, refusal) in cases {
        let registration = wit::Registration {
            blocks: vec![cable()],
            items,
        };
        let outcome = validate_registration(dir.path(), &manifest, registration)
            .map_err(|error| format!("{error:#}"));
        match (outcome, refusal) {
            (Ok(_), None) => {}
            (Ok(_), Some(cause)) => failures.push(format!("{case}: accepted, not {cause:?}")),
            (Err(error), None) => failures.push(format!("{case}: refused: {error}")),
            (Err(error), Some(cause)) if !error.contains(cause) => {
                failures.push(format!("{case}: {error:?} lacks {cause:?}"));
            }
            (Err(_), Some(_)) => {}
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));

    let (blocks, items) = validate_registration(
        dir.path(),
        &manifest,
        wit::Registration {
            blocks: vec![cable()],
            items: vec![item("cell")],
        },
    )
    .unwrap();
    assert_eq!(blocks.len(), 1);
    let icon = dir.path().join(ASSETS_DIR).join("counter.png");
    assert_eq!(
        items,
        vec![ItemDef {
            id: "probe:cell".to_owned(),
            display_name: "Probe Cell".to_owned(),
            icon: icon.to_str().unwrap().to_owned(),
            max_stack: 1,
        }]
    );
}

/// The cable's axes: its trait's state, with the client's values, before its own states, in
/// their order.
#[test]
fn axes_put_placement_states_first() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = rule_assets(dir.path());
    let mut member = cable();
    member.network = true;
    let blocks = validate_blocks(dir.path(), &manifest, vec![member]).unwrap();
    assert!(blocks[0].network, "network membership reaches the adapter");
    let axes = axes(&blocks[0].states, &blocks[0].placement);
    let names: Vec<&str> = axes.iter().map(|axis| axis.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "minecraft:facing_direction",
            "probe:down",
            "probe:up",
            "probe:north",
            "probe:south",
            "probe:west",
            "probe:east",
        ]
    );
    let (_, faces) = PlacementState::FacingDirection.state();
    assert_eq!(
        axes[0],
        StateDef {
            name: "minecraft:facing_direction".to_owned(),
            values: StateValues::Choices(faces.iter().map(|&face| face.to_owned()).collect()),
        }
    );
}
