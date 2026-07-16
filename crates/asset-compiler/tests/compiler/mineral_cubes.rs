use super::support::*;

#[derive(Clone, Copy)]
struct MineralFixture {
    sequential_id: u32,
    network_hash: u32,
    name: &'static str,
    path: &'static str,
    colour: [u8; 4],
}

const MINERAL_CUBES: [MineralFixture; 2] = [
    MineralFixture {
        sequential_id: 12_638,
        network_hash: 0xbda0_2665,
        name: "minecraft:cinnabar",
        path: "textures/blocks/cinnabar",
        colour: [176, 32, 24, 255],
    },
    MineralFixture {
        sequential_id: 14_658,
        network_hash: 0x2d65_8dd8,
        name: "minecraft:sulfur",
        path: "textures/blocks/sulfur",
        colour: [228, 208, 48, 255],
    },
];

fn mineral_cube_records() -> Vec<RegistryRecord> {
    let registry = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed protocol-1001 registry");
    MINERAL_CUBES
        .iter()
        .map(|expected| {
            let record = registry[expected.sequential_id as usize].clone();
            assert_eq!(record.sequential_id, expected.sequential_id);
            assert_eq!(record.network_hash, expected.network_hash);
            assert_eq!(record.name.as_ref(), expected.name);
            record
        })
        .collect()
}

fn write_mineral_pack(root: &Path) {
    write_pack(
        root,
        r#"{
            "cinnabar":{"sound":"cinnabar","textures":"cinnabar"},
            "sulfur":{"sound":"sulfur","textures":"sulfur"}
        }"#,
        r#"{"texture_data":{
            "cinnabar":{"textures":"textures/blocks/cinnabar"},
            "sulfur":{"textures":"textures/blocks/sulfur"}
        }}"#,
        "[]",
    );
    for expected in &MINERAL_CUBES {
        write_png(
            root,
            expected.path,
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, expected.colour),
        );
    }
}

#[test]
fn protocol_1001_mineral_cube_inventory_is_exact_and_collision_backed() {
    let records = mineral_cube_records();
    for (record, expected) in records.iter().zip(&MINERAL_CUBES) {
        assert_eq!(record.sequential_id, expected.sequential_id);
        assert_eq!(record.network_hash, expected.network_hash);
        assert_eq!(record.name.as_ref(), expected.name);
        assert_eq!(record.canonical_state.as_ref(), "{}");
        assert_eq!(record.model_family, ModelFamily::Unknown);
        assert_eq!(record.contributor_role, ContributorRole::Primary);
        assert_eq!(record.flags, BlockFlags::empty());
        assert_eq!(record.model_state, ModelState::default());
        assert_eq!(record.face_coverage, 0);
        assert_eq!(record.collision_seed.shape_id, 1);
        assert_eq!(
            record.collision_seed.confidence,
            CollisionConfidence::CollisionOnly
        );
        assert_eq!(
            record.collision_seed.boxes.as_ref(),
            [CollisionBox {
                max_x: 100_000_000,
                max_y: 100_000_000,
                max_z: 100_000_000,
                ..CollisionBox::default()
            }]
        );
        assert_eq!(
            record.provenance,
            assets::RegistryProvenance::PMMP
                | assets::RegistryProvenance::DRAGONFLY
                | assets::RegistryProvenance::PRISMARINE
        );
    }
}

#[test]
fn compiler_admits_exact_opaque_mineral_cubes_with_hash_parity() {
    let directory = tempfile::tempdir().expect("create mineral cube fixture");
    write_mineral_pack(directory.path());
    let records = mineral_cube_records();

    let compiled = compile_pack(directory.path(), &records).expect("compile exact mineral cubes");
    for (record, expected) in records.iter().zip(&MINERAL_CUBES) {
        let visual = compiled.visuals[record.sequential_id as usize];
        assert_eq!(visual.kind, VisualKind::Cube);
        assert_eq!(
            visual.flags,
            BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE
        );
        assert_eq!(visual.model_template, assets::NO_MODEL_TEMPLATE);
        assert_eq!(visual.variant, 0);
        assert!(
            visual
                .faces
                .iter()
                .all(|&material| material != DIAGNOSTIC_MATERIAL)
        );
        assert!(
            visual
                .faces
                .iter()
                .all(|&material| material == visual.faces[0])
        );
        let material = material_for_face(&compiled, record.sequential_id as usize, BlockFace::Up);
        assert_eq!(
            material.flags, 0,
            "no tint, alpha, animation, or UV rotation"
        );
        assert_eq!(material.animation, assets::NO_ANIMATION);
        assert_eq!(
            mip_pixel(&compiled, 0, material.texture.layer(), 0, 0),
            expected.colour
        );
    }
    assert!(compiled.model_templates.is_empty());
    assert!(compiled.model_quads.is_empty());

    let blob = encode_blob(&compiled).expect("encode mineral cube assets");
    let runtime = RuntimeAssets::decode(&blob).expect("decode mineral cube assets");
    for record in &records {
        let sequential = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        let hashed = runtime.resolve(NetworkIdMode::Hashed, record.network_hash);
        assert_eq!(sequential.kind(), VisualKind::Cube);
        assert_eq!(hashed.kind(), VisualKind::Cube);
        assert_eq!(sequential.flags(), hashed.flags());
        for face in BlockFace::ALL {
            assert_eq!(
                sequential.face(face).material_id(),
                hashed.face(face).material_id()
            );
        }
    }
    assert_eq!(runtime.missing_count(), 0);

    let baseline = blob;
    let mut reversed = records.clone();
    reversed.reverse();
    let reversed = compile_pack(directory.path(), &reversed).expect("compile reversed minerals");
    assert_eq!(encode_blob(&reversed).unwrap(), baseline);
}

#[test]
fn mineral_cube_admission_is_atomic_and_pack_routes_are_exact() {
    let directory = tempfile::tempdir().expect("create mineral cube fixture");
    write_mineral_pack(directory.path());
    let records = mineral_cube_records();

    let mut malformed = Vec::new();
    malformed.push(("missing family member", vec![records[0].clone()]));
    let mut wrong_state = records.clone();
    wrong_state[0].canonical_state = "null".into();
    malformed.push(("wrong canonical state", wrong_state));
    let mut wrong_hash = records.clone();
    wrong_hash[0].network_hash ^= 1;
    malformed.push(("wrong network hash", wrong_hash));
    let mut wrong_flags = records.clone();
    wrong_flags[0].flags = BlockFlags::CUBE_GEOMETRY;
    malformed.push(("wrong source flags", wrong_flags));
    let mut wrong_family = records.clone();
    wrong_family[0].model_family = ModelFamily::Cube;
    malformed.push(("wrong source family", wrong_family));
    let mut wrong_collision = records.clone();
    wrong_collision[0].collision_seed.boxes[0].max_y -= 1;
    malformed.push(("wrong collision", wrong_collision));

    for (label, family) in malformed {
        let compiled = compile_pack(directory.path(), &family).expect("compile malformed minerals");
        assert!(
            family.iter().all(|record| {
                compiled.visuals[record.sequential_id as usize].kind == VisualKind::Diagnostic
            }),
            "invalid family `{label}` leaked a supported visual"
        );
    }

    for (label, blocks, terrain, flipbooks) in [
        (
            "wrong block selector",
            r#"{"cinnabar":{"sound":"cinnabar","textures":"cinnabar_wrong"},"sulfur":{"sound":"sulfur","textures":"sulfur"}}"#,
            r#"{"texture_data":{"cinnabar":{"textures":"textures/blocks/cinnabar"},"cinnabar_wrong":{"textures":"textures/blocks/cinnabar"},"sulfur":{"textures":"textures/blocks/sulfur"}}}"#,
            "[]",
        ),
        (
            "wrong terrain path",
            r#"{"cinnabar":{"sound":"cinnabar","textures":"cinnabar"},"sulfur":{"sound":"sulfur","textures":"sulfur"}}"#,
            r#"{"texture_data":{"cinnabar":{"textures":"textures/blocks/cinnabar_wrong"},"sulfur":{"textures":"textures/blocks/sulfur"}}}"#,
            "[]",
        ),
        (
            "texture variant array",
            r#"{"cinnabar":{"sound":"cinnabar","textures":"cinnabar"},"sulfur":{"sound":"sulfur","textures":"sulfur"}}"#,
            r#"{"texture_data":{"cinnabar":{"textures":["textures/blocks/cinnabar","textures/blocks/cinnabar"]},"sulfur":{"textures":"textures/blocks/sulfur"}}}"#,
            "[]",
        ),
        (
            "terrain alias metadata",
            r#"{"cinnabar":{"sound":"cinnabar","textures":"cinnabar"},"sulfur":{"sound":"sulfur","textures":"sulfur"}}"#,
            r#"{"texture_data":{"cinnabar":{"textures":"textures/blocks/cinnabar","alias":"cinnabar_legacy"},"sulfur":{"textures":"textures/blocks/sulfur"}}}"#,
            "[]",
        ),
        (
            "terrain texture extension metadata",
            r#"{"cinnabar":{"sound":"cinnabar","textures":"cinnabar"},"sulfur":{"sound":"sulfur","textures":"sulfur"}}"#,
            r#"{"texture_data":{"cinnabar":{"textures":{"path":"textures/blocks/cinnabar","extension":"review-required"}},"sulfur":{"textures":"textures/blocks/sulfur"}}}"#,
            "[]",
        ),
        (
            "block isotropic metadata",
            r#"{"cinnabar":{"sound":"cinnabar","textures":"cinnabar","isotropic":true},"sulfur":{"sound":"sulfur","textures":"sulfur"}}"#,
            r#"{"texture_data":{"cinnabar":{"textures":"textures/blocks/cinnabar"},"sulfur":{"textures":"textures/blocks/sulfur"}}}"#,
            "[]",
        ),
        (
            "block extra rendering field",
            r#"{"cinnabar":{"sound":"cinnabar","textures":"cinnabar","carried_textures":"cinnabar"},"sulfur":{"sound":"sulfur","textures":"sulfur"}}"#,
            r#"{"texture_data":{"cinnabar":{"textures":"textures/blocks/cinnabar"},"sulfur":{"textures":"textures/blocks/sulfur"}}}"#,
            "[]",
        ),
        (
            "flipbook route",
            r#"{"cinnabar":{"sound":"cinnabar","textures":"cinnabar"},"sulfur":{"sound":"sulfur","textures":"sulfur"}}"#,
            r#"{"texture_data":{"cinnabar":{"textures":"textures/blocks/cinnabar"},"sulfur":{"textures":"textures/blocks/sulfur"}}}"#,
            r#"[{"flipbook_texture":"textures/blocks/cinnabar","atlas_tile":"cinnabar"}]"#,
        ),
    ] {
        let malformed = tempfile::tempdir().expect("create malformed mineral pack");
        write_mineral_pack(malformed.path());
        write_pack(malformed.path(), blocks, terrain, flipbooks);
        let compiled = compile_pack(malformed.path(), &records).expect("compile malformed pack");
        assert!(
            records.iter().all(|record| {
                compiled.visuals[record.sequential_id as usize].kind == VisualKind::Diagnostic
            }),
            "nonexact route `{label}` was accepted"
        );
    }

    let transparent = tempfile::tempdir().expect("create transparent mineral pack");
    write_mineral_pack(transparent.path());
    write_png(
        transparent.path(),
        "textures/blocks/cinnabar",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [176, 32, 24, 254]),
    );
    let compiled = compile_pack(transparent.path(), &records).expect("compile alpha mineral pack");
    assert!(records.iter().all(|record| {
        compiled.visuals[record.sequential_id as usize].kind == VisualKind::Diagnostic
    }));
}

#[test]
#[ignore = "requires PINNED_VANILLA_PACK pointing at ignored Bedrock 1.26.30.32 resource pack"]
fn compiler_real_pinned_pack_admits_exact_mineral_cubes() {
    let pack = std::env::var_os("PINNED_VANILLA_PACK")
        .expect("set PINNED_VANILLA_PACK to the ignored pinned vanilla resource pack");
    let records = mineral_cube_records();
    let compiled = compile_pack(Path::new(&pack), &records).expect("compile pinned mineral cubes");
    for record in records {
        let visual = compiled.visuals[record.sequential_id as usize];
        assert_eq!(visual.kind, VisualKind::Cube);
        assert_eq!(
            visual.flags,
            BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE
        );
        assert!(visual.faces.iter().all(|&face| face != DIAGNOSTIC_MATERIAL));
    }
}
