use super::support::*;

fn write_door_trapdoor_pack(root: &Path) {
    write_pack(
        root,
        r#"{
            "wooden_door":{"textures":{"down":"door_lower","side":"door_upper","up":"door_lower"}},
            "trapdoor":{"textures":"trapdoor"}
        }"#,
        r#"{"texture_data":{
            "door_lower":{"textures":"textures/blocks/door_lower"},
            "door_upper":{"textures":"textures/blocks/door_upper"},
            "trapdoor":{"textures":"textures/blocks/trapdoor"}
        }}"#,
        "[]",
    );
    for (path, colour) in [
        ("door_lower", [40, 80, 120, 0]),
        ("door_upper", [80, 120, 160, 127]),
        ("trapdoor", [120, 160, 200, 200]),
    ] {
        write_png(
            root,
            &format!("textures/blocks/{path}"),
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, colour),
        );
    }
}

fn pinned_collision_bounds(record: &RegistryRecord) -> ([i16; 3], [i16; 3]) {
    assert_eq!(
        record.collision_seed.confidence,
        CollisionConfidence::CollisionOnly,
        "{} {} collision authority",
        record.name,
        record.canonical_state
    );
    let [collision] = record.collision_seed.boxes.as_ref() else {
        panic!(
            "{} {} must have one pinned collision cuboid, got {:?}",
            record.name, record.canonical_state, record.collision_seed.boxes
        );
    };
    let convert = |value: i32| {
        let scaled = i64::from(value) * 256;
        let rounded = if scaled >= 0 {
            (scaled + 50_000_000) / 100_000_000
        } else {
            (scaled - 50_000_000) / 100_000_000
        };
        i16::try_from(rounded).expect("bounded collision coordinate")
    };
    (
        [
            convert(collision.min_x),
            convert(collision.min_y),
            convert(collision.min_z),
        ],
        [
            convert(collision.max_x),
            convert(collision.max_y),
            convert(collision.max_z),
        ],
    )
}

fn assert_bounds_within(
    rendered: ([i16; 3], [i16; 3]),
    collision: ([i16; 3], [i16; 3]),
    tolerance: i16,
) {
    for (rendered, collision) in rendered
        .0
        .into_iter()
        .chain(rendered.1)
        .zip(collision.0.into_iter().chain(collision.1))
    {
        assert!(
            (rendered - collision).abs() <= tolerance,
            "render/collision selector bounds differ: rendered={rendered} collision={collision} tolerance={tolerance}"
        );
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DoorFacing {
    North,
    South,
    West,
    East,
}

impl DoorFacing {
    fn rotate_right(self) -> Self {
        match self {
            Self::North => Self::East,
            Self::East => Self::South,
            Self::South => Self::West,
            Self::West => Self::North,
        }
    }

    fn rotate_left(self) -> Self {
        match self {
            Self::North => Self::West,
            Self::West => Self::South,
            Self::South => Self::East,
            Self::East => Self::North,
        }
    }
}

fn decoded_door_facing(encoded_orientation: u32) -> DoorFacing {
    let encoded = match encoded_orientation {
        0 => DoorFacing::South,
        1 => DoorFacing::West,
        2 => DoorFacing::North,
        3 => DoorFacing::East,
        _ => panic!("invalid encoded door orientation {encoded_orientation}"),
    };
    // Dragonfly encodes Door.Facing.RotateRight(), so recover the logical
    // closed facing by applying the inverse rotation.
    encoded.rotate_left()
}

fn expected_door_bounds(orientation: u32, open: u32, hinge: u32) -> ([i16; 3], [i16; 3]) {
    const T: i16 = 48;
    const H: i16 = 256 - T;
    let facing = decoded_door_facing(orientation);
    let effective = match (open, hinge) {
        (0, 0 | 1) => facing,
        (1, 0) => facing.rotate_right(),
        (1, 1) => facing.rotate_left(),
        _ => panic!("invalid door selector {orientation}/{open}/{hinge}"),
    };
    match effective {
        DoorFacing::North => ([0, 0, H], [256, 256, 256]),
        DoorFacing::South => ([0, 0, 0], [256, 256, T]),
        DoorFacing::West => ([H, 0, 0], [256, 256, 256]),
        DoorFacing::East => ([0, 0, 0], [T, 256, 256]),
    }
}

fn expected_trapdoor_bounds(orientation: u32, open: u32, half: u32) -> ([i16; 3], [i16; 3]) {
    const T: i16 = 48;
    const H: i16 = 256 - T;
    match (open, orientation, half) {
        (0, _, 0) => ([0, 0, 0], [256, T, 256]),
        (0, _, 1) => ([0, H, 0], [256, 256, 256]),
        (1, 0, _) => ([0, 0, 0], [T, 256, 256]),
        (1, 1, _) => ([H, 0, 0], [256, 256, 256]),
        (1, 2, _) => ([0, 0, 0], [256, 256, T]),
        (1, 3, _) => ([0, 0, H], [256, 256, 256]),
        _ => panic!("invalid trapdoor selector {orientation}/{open}/{half}"),
    }
}

fn assert_cutout_cuboid(compiled: &CompiledAssets, visual_id: usize) {
    let visual = compiled.visuals[visual_id];
    assert_eq!(visual.kind, VisualKind::Model);
    assert!(!visual.flags.intersects(
        BlockFlags::AIR
            | BlockFlags::CUBE_GEOMETRY
            | BlockFlags::OCCLUDES_FULL_FACE
            | BlockFlags::LEAF_MODEL
    ));
    let template = compiled.model_templates[visual.model_template as usize];
    assert_eq!(template.quad_count, 6);
    assert_eq!(template.flags, 0);
    let quads = compiled_model_quads(compiled, visual_id);
    for quad in quads {
        assert!(
            quad.positions
                .iter()
                .flatten()
                .all(|value| (0..=256).contains(value))
        );
        assert!(quad.uvs.iter().flatten().all(|value| *value <= 4096));
        assert!((1..=6).contains(&(quad.flags & MODEL_QUAD_FLAG_FACE_MASK)));
        assert_eq!(quad.flags & MODEL_QUAD_FLAG_CULL_FACE_MASK, 0);
        assert_eq!(quad.flags & MODEL_QUAD_FLAG_TWO_SIDED, 0);
        assert_ne!(quad.material, DIAGNOSTIC_MATERIAL);
        assert_eq!(
            compiled.materials[quad.material as usize].flags,
            MATERIAL_FLAG_ALPHA_CUTOUT
        );
    }
}

#[test]
fn compiler_routes_every_generated_door_and_trapdoor_selector_to_exact_cutout_cuboids() {
    let directory = tempfile::tempdir().expect("create door/trapdoor fixture");
    write_door_trapdoor_pack(directory.path());
    let doors = generated_family_records("minecraft:wooden_door", ModelFamily::Door);
    let trapdoors = generated_family_records("minecraft:trapdoor", ModelFamily::Trapdoor);
    assert_eq!(doors.len(), 32);
    assert_eq!(trapdoors.len(), 16);

    let compiled_doors = compile_pack(directory.path(), &doors).expect("compile all door states");
    let compiled_trapdoors =
        compile_pack(directory.path(), &trapdoors).expect("compile all trapdoor states");
    assert_eq!(
        compiled_doors.materials.len(),
        3,
        "diagnostic + lower + upper"
    );
    assert_eq!(
        compiled_trapdoors.materials.len(),
        2,
        "diagnostic + trapdoor"
    );
    assert_eq!(
        compiled_doors.model_templates.len(),
        8,
        "four spatial bounds times lower/upper materials"
    );
    assert_eq!(compiled_trapdoors.model_templates.len(), 6);
    let door_collision_bounds = doors
        .iter()
        .map(pinned_collision_bounds)
        .collect::<HashSet<_>>();
    let trapdoor_collision_bounds = trapdoors
        .iter()
        .map(pinned_collision_bounds)
        .collect::<HashSet<_>>();
    assert_eq!(
        door_collision_bounds,
        HashSet::from([([0, 0, 0], [47, 256, 256])]),
        "Prismarine exposes one uniform collision-only door seed for all typed states"
    );
    assert_eq!(
        trapdoor_collision_bounds,
        HashSet::from([
            ([0, 0, 0], [256, 47, 256]),
            ([0, 209, 0], [256, 256, 256]),
            ([0, 0, 209], [256, 256, 256]),
            ([0, 0, 0], [256, 256, 47]),
            ([209, 0, 0], [256, 256, 256]),
            ([0, 0, 0], [47, 256, 256]),
        ]),
        "trapdoor collision-only seeds must cover both halves and all four open boundaries"
    );
    // The pinned collision slabs are 0.1825 blocks thick (47/256 after
    // rounding) and contain no state transition. They therefore audit the
    // source limitation only; render geometry remains the exact typed 3/16
    // contract below and never reads CollisionSeed.

    for (id, record) in doors.iter().enumerate() {
        let orientation = record
            .model_state
            .get(ModelStateField::Orientation)
            .unwrap();
        let open = record.model_state.get(ModelStateField::Open).unwrap();
        let hinge = record.model_state.get(ModelStateField::Hinge).unwrap();
        let flags = record.model_state.get(ModelStateField::Flags).unwrap();
        assert!(orientation <= 3 && open <= 1 && hinge <= 1 && flags <= MODEL_FLAG_UPPER);
        assert_cutout_cuboid(&compiled_doors, id);
        let bounds = model_bounds(compiled_model_quads(&compiled_doors, id));
        assert_eq!(
            bounds,
            expected_door_bounds(orientation, open, hinge),
            "{}",
            record.canonical_state
        );
        let expected_material = usize::from(flags & MODEL_FLAG_UPPER != 0) + 1;
        assert!(
            compiled_model_quads(&compiled_doors, id)
                .iter()
                .all(|quad| quad.material as usize == expected_material)
        );
        if open == 0 {
            let peer = doors
                .iter()
                .position(|candidate| {
                    candidate.model_state.get(ModelStateField::Orientation) == Some(orientation)
                        && candidate.model_state.get(ModelStateField::Open) == Some(open)
                        && candidate.model_state.get(ModelStateField::Flags) == Some(flags)
                        && candidate.model_state.get(ModelStateField::Hinge) == Some(hinge ^ 1)
                })
                .unwrap();
            assert_eq!(
                compiled_doors.visuals[id].model_template,
                compiled_doors.visuals[peer].model_template,
                "closed door hinge must deduplicate"
            );
        }
    }

    for (id, record) in trapdoors.iter().enumerate() {
        let orientation = record
            .model_state
            .get(ModelStateField::Orientation)
            .unwrap();
        let open = record.model_state.get(ModelStateField::Open).unwrap();
        let half = record.model_state.get(ModelStateField::Half).unwrap();
        assert!(orientation <= 3 && open <= 1 && half <= 1);
        assert_cutout_cuboid(&compiled_trapdoors, id);
        let bounds = model_bounds(compiled_model_quads(&compiled_trapdoors, id));
        assert_eq!(
            bounds,
            expected_trapdoor_bounds(orientation, open, half),
            "{}",
            record.canonical_state
        );
        assert_bounds_within(bounds, pinned_collision_bounds(record), 1);
        let peer = trapdoors
            .iter()
            .position(|candidate| {
                let same_open = candidate.model_state.get(ModelStateField::Open) == Some(open);
                if open == 0 {
                    same_open
                        && candidate.model_state.get(ModelStateField::Half) == Some(half)
                        && candidate.model_state.get(ModelStateField::Orientation)
                            == Some(orientation ^ 1)
                } else {
                    same_open
                        && candidate.model_state.get(ModelStateField::Orientation)
                            == Some(orientation)
                        && candidate.model_state.get(ModelStateField::Half) == Some(half ^ 1)
                }
            })
            .unwrap();
        assert_eq!(
            compiled_trapdoors.visuals[id].model_template,
            compiled_trapdoors.visuals[peer].model_template,
            "inactive trapdoor selector must deduplicate"
        );
    }
}

#[test]
fn compiler_door_and_trapdoor_selectors_fail_closed_when_required_fields_are_missing() {
    let directory = tempfile::tempdir().expect("create fail-closed fixture");
    write_door_trapdoor_pack(directory.path());
    let mut door = model_record(0, 16_000, "minecraft:wooden_door", "{}", ModelFamily::Door);
    let mut trapdoor = model_record(1, 16_001, "minecraft:trapdoor", "{}", ModelFamily::Trapdoor);
    door.collision_seed = CollisionSeed {
        shape_id: 99,
        confidence: CollisionConfidence::CollisionOnly,
        boxes: vec![CollisionBox {
            max_x: 100_000_000,
            max_y: 1,
            max_z: 100_000_000,
            ..CollisionBox::default()
        }]
        .into_boxed_slice(),
    };
    trapdoor.collision_seed = door.collision_seed.clone();
    let compiled = compile_pack(directory.path(), &[door, trapdoor]).expect("fail closed");
    assert!(
        compiled
            .visuals
            .iter()
            .all(|visual| visual.kind == VisualKind::Diagnostic)
    );
    assert!(compiled.model_templates.is_empty());
    assert!(compiled.model_quads.is_empty());
}

#[test]
fn compiler_door_and_trapdoor_selectors_fail_closed_for_every_out_of_range_field() {
    let directory = tempfile::tempdir().expect("create invalid-selector fixture");
    write_door_trapdoor_pack(directory.path());
    let mut records = vec![
        encoded_model_record(
            0,
            16_100,
            "minecraft:wooden_door",
            ModelFamily::Door,
            &[
                (ModelStateField::Orientation, 0),
                (ModelStateField::Open, 0),
                (ModelStateField::Hinge, 0),
                (ModelStateField::Flags, 0),
            ],
        ),
        encoded_model_record(
            1,
            16_101,
            "minecraft:trapdoor",
            ModelFamily::Trapdoor,
            &[
                (ModelStateField::Orientation, 0),
                (ModelStateField::Open, 0),
                (ModelStateField::Half, 0),
            ],
        ),
    ];
    for (field, value) in [
        (ModelStateField::Orientation, 4),
        (ModelStateField::Open, 2),
        (ModelStateField::Hinge, 2),
        (ModelStateField::Flags, 1),
    ] {
        let id = records.len() as u32;
        let mut fields = vec![
            (ModelStateField::Orientation, 0),
            (ModelStateField::Open, 0),
            (ModelStateField::Hinge, 0),
            (ModelStateField::Flags, 0),
        ];
        fields.iter_mut().find(|entry| entry.0 == field).unwrap().1 = value;
        records.push(encoded_model_record(
            id,
            16_100 + id,
            "minecraft:wooden_door",
            ModelFamily::Door,
            &fields,
        ));
    }
    for (field, value) in [
        (ModelStateField::Orientation, 4),
        (ModelStateField::Open, 2),
        (ModelStateField::Half, 2),
    ] {
        let id = records.len() as u32;
        let mut fields = vec![
            (ModelStateField::Orientation, 0),
            (ModelStateField::Open, 0),
            (ModelStateField::Half, 0),
        ];
        fields.iter_mut().find(|entry| entry.0 == field).unwrap().1 = value;
        records.push(encoded_model_record(
            id,
            16_100 + id,
            "minecraft:trapdoor",
            ModelFamily::Trapdoor,
            &fields,
        ));
    }
    let compiled = compile_pack(directory.path(), &records).expect("compile bounded selectors");
    assert_eq!(compiled.visuals[0].kind, VisualKind::Model);
    assert_eq!(compiled.visuals[1].kind, VisualKind::Model);
    assert!(
        compiled.visuals[2..]
            .iter()
            .all(|visual| visual.kind == VisualKind::Diagnostic)
    );
}

#[test]
fn compiler_selects_the_exact_legacy_door_terrain_variant_for_each_material_family() {
    let directory = tempfile::tempdir().expect("create legacy door-array fixture");
    let names = [
        "wooden_door",
        "spruce_door",
        "birch_door",
        "jungle_door",
        "acacia_door",
        "dark_oak_door",
        "iron_door",
    ];
    let blocks = serde_json::Value::Object(
        names
            .iter()
            .map(|name| {
                (
                    (*name).to_owned(),
                    serde_json::json!({"textures":{"down":"door_lower","side":"door_upper","up":"door_lower"}}),
                )
            })
            .collect(),
    );
    let lower_paths = (0..7)
        .map(|index| format!("textures/blocks/lower_{index}"))
        .collect::<Vec<_>>();
    let upper_paths = (0..7)
        .map(|index| format!("textures/blocks/upper_{index}"))
        .collect::<Vec<_>>();
    let terrain = serde_json::json!({"texture_data":{
        "door_lower":{"textures":lower_paths},
        "door_upper":{"textures":upper_paths}
    }});
    write_pack(
        directory.path(),
        &serde_json::to_string(&blocks).unwrap(),
        &serde_json::to_string(&terrain).unwrap(),
        "[]",
    );
    for index in 0..7_u8 {
        for half in ["lower", "upper"] {
            write_png(
                directory.path(),
                &format!("textures/blocks/{half}_{index}"),
                TILE_SIZE,
                TILE_SIZE,
                &solid(TILE_SIZE, TILE_SIZE, [index * 31 + 1, 2, 3, 127]),
            );
        }
    }
    let all = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed registry");
    let mut records = names
        .iter()
        .enumerate()
        .map(|(id, name)| {
            let mut record = all
                .iter()
                .find(|record| {
                    record.name.as_ref() == format!("minecraft:{name}")
                        && record.model_state.get(ModelStateField::Flags) == Some(0)
                        && record.model_state.get(ModelStateField::Open) == Some(0)
                        && record.model_state.get(ModelStateField::Hinge) == Some(0)
                        && record.model_state.get(ModelStateField::Orientation) == Some(0)
                })
                .unwrap_or_else(|| panic!("missing lower {name}"))
                .clone();
            record.sequential_id = id as u32;
            record.network_hash = 17_000 + id as u32;
            record
        })
        .collect::<Vec<_>>();
    let compiled = compile_pack(directory.path(), &records).expect("compile legacy door variants");
    for (id, name) in names.iter().enumerate() {
        let visual = compiled.visuals[id];
        assert_eq!(visual.kind, VisualKind::Model, "{name}");
        let material = compiled.materials[compiled_model_quads(&compiled, id)[0].material as usize];
        assert_eq!(
            &mip_layer(&compiled, 0, material.texture.layer())[0..4],
            &[id as u8 * 31 + 1, 2, 3, 127],
            "{name} selected the wrong legacy terrain-array entry"
        );
    }
    let baseline = encode_blob(&compiled).expect("encode legacy door variants");
    records.reverse();
    let reversed = compile_pack(directory.path(), &records).expect("compile reversed door records");
    assert_eq!(encode_blob(&reversed).unwrap(), baseline);
}

#[test]
fn compiler_real_pinned_pack_has_zero_diagnostic_door_and_trapdoor_states_when_requested() {
    let Some(pack) = std::env::var_os("PINNED_VANILLA_PACK") else {
        return;
    };
    let mut records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry")
    .into_iter()
    .filter(|record| {
        matches!(
            record.model_family,
            ModelFamily::Door | ModelFamily::Trapdoor
        )
    })
    .collect::<Vec<_>>();
    assert_eq!(
        records
            .iter()
            .filter(|record| record.model_family == ModelFamily::Door)
            .count(),
        672
    );
    assert_eq!(
        records
            .iter()
            .filter(|record| record.model_family == ModelFamily::Trapdoor)
            .count(),
        336
    );
    records.sort_unstable_by_key(|record| record.sequential_id);
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 60_000 + id as u32;
    }

    let compiled = compile_pack(Path::new(&pack), &records).expect("compile pinned doors");
    for (id, record) in records.iter().enumerate() {
        assert_cutout_cuboid(&compiled, id);
        assert_eq!(record.face_coverage, 0, "{}", record.name);
    }
    for modern in [
        "minecraft:bamboo_door",
        "minecraft:cherry_door",
        "minecraft:mangrove_door",
        "minecraft:pale_oak_door",
        "minecraft:crimson_door",
        "minecraft:warped_door",
        "minecraft:copper_door",
        "minecraft:exposed_copper_door",
        "minecraft:weathered_copper_door",
        "minecraft:oxidized_copper_door",
        "minecraft:waxed_copper_door",
        "minecraft:waxed_exposed_copper_door",
        "minecraft:waxed_weathered_copper_door",
        "minecraft:waxed_oxidized_copper_door",
    ] {
        let matching = records
            .iter()
            .enumerate()
            .filter(|(_, record)| record.name.as_ref() == modern)
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), 32, "{modern} state count");
        assert!(matching.into_iter().all(|(id, _)| {
            compiled.visuals[id].kind == VisualKind::Model
                && compiled_model_quads(&compiled, id).iter().all(|quad| {
                    compiled.materials[quad.material as usize].flags == MATERIAL_FLAG_ALPHA_CUTOUT
                })
        }));
    }

    let baseline = encode_blob(&compiled).expect("encode exhaustive doors");
    records.reverse();
    let reversed = compile_pack(Path::new(&pack), &records).expect("compile reversed doors");
    assert_eq!(
        encode_blob(&reversed).expect("encode reversed exhaustive doors"),
        baseline,
        "door/trapdoor compiler output depends on registry order"
    );
}
