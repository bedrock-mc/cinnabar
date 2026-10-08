struct CompiledFireFixture {
    assets: RuntimeAssets,
    air: u32,
    fire: u32,
    floor: u32,
    wool: u32,
    glass: u32,
}

fn compiled_fire_fixture() -> &'static CompiledFireFixture {
    static FIXTURE: OnceLock<CompiledFireFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let target: serde_json::Value =
            serde_json::from_slice(include_bytes!("../../../../../assets/bedrock-target.json"))
                .unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let records = assets::read_registry_for_protocol(
            &fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap(),
            target["wire_protocol"].as_u64().unwrap() as u32,
        )
        .unwrap();
        let names = ["air", "fire", "netherrack", "red_wool", "glass"];
        let selected = names.map(|name| {
            records
                .iter()
                .find(|record| record.name.as_ref() == format!("minecraft:{name}"))
                .unwrap()
                .clone()
        });
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir_all(directory.path().join("textures/blocks")).unwrap();
        fs::write(
            directory.path().join("blocks.json"),
            r#"{
            "fire":{"textures":{"up":"fire_0","down":"fire_1","side":"fire_0"}},
            "netherrack":{"textures":"floor"},"red_wool":{"textures":"floor"},
            "glass":{"textures":"glass"}}
        "#,
        )
        .unwrap();
        fs::write(
            directory.path().join("textures/terrain_texture.json"),
            r#"{"texture_data":{
            "fire_0":{"textures":"textures/blocks/up"},
            "fire_1":{"textures":"textures/blocks/down"},
            "floor":{"textures":"textures/blocks/floor"},
            "glass":{"textures":"textures/blocks/glass"}}}
        "#,
        )
        .unwrap();
        fs::write(
            directory.path().join("textures/flipbook_textures.json"),
            "[]",
        )
        .unwrap();
        for (name, colour) in [
            ("up", [180, 40, 10, 255]),
            ("down", [230, 160, 30, 255]),
            ("floor", [80, 70, 60, 255]),
            ("glass", [100, 150, 200, 0]),
        ] {
            let pixels = colour.repeat((assets::TILE_SIZE * assets::TILE_SIZE) as usize);
            let mut png = Vec::new();
            PngEncoder::new(&mut png)
                .write_image(
                    &pixels,
                    assets::TILE_SIZE,
                    assets::TILE_SIZE,
                    ExtendedColorType::Rgba8,
                )
                .unwrap();
            fs::write(
                directory.path().join(format!("textures/blocks/{name}.png")),
                png,
            )
            .unwrap();
        }
        let compiled = compile_pack(directory.path(), &selected).unwrap();
        CompiledFireFixture {
            assets: RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap(),
            air: selected[0].sequential_id,
            fire: selected[1].sequential_id,
            floor: selected[2].sequential_id,
            wool: selected[3].sequential_id,
            glass: selected[4].sequential_id,
        }
    })
}

fn fire_mesh(fixture: &CompiledFireFixture, center: &SubChunk, origin: [i32; 3]) -> ChunkMesh {
    mesh_sub_chunk_in_neighbourhood(
        &BlockClassifier::new(fixture.air),
        &fixture.assets,
        NetworkIdMode::Sequential,
        &MeshNeighbourhood::new(center).with_block_origin(origin),
    )
}

fn fire_reference<'a>(
    fixture: &CompiledFireFixture,
    mesh: &'a ChunkMesh,
) -> Option<&'a PackedModelRef> {
    mesh.model_refs().iter().find(|reference| {
        fixture.assets.model_templates()[reference.words()[1] as usize].flags
            == assets::MODEL_TEMPLATE_FLAG_FIRE
    })
}

#[test]
fn fire_supported_mesh_ignores_attached_neighbours_and_keeps_both_textures() {
    let fixture = compiled_fire_fixture();
    for floor in [fixture.floor, fixture.wool, fixture.glass] {
        let center = sub_chunk(vec![packed_storage(
            2,
            &[fixture.air, fixture.fire, floor, fixture.wool],
            &[
                ([7, 8, 9], 1),
                ([7, 7, 9], 2),
                ([6, 8, 9], 3),
                ([7, 9, 9], 3),
            ],
        )]);
        let mesh = fire_mesh(fixture, &center, [0; 3]);
        let reference = fire_reference(fixture, &mesh).unwrap();
        let base = fixture
            .assets
            .resolve(NetworkIdMode::Sequential, fixture.fire)
            .model_template()
            .unwrap();
        assert_eq!(reference.words()[1], base);
        assert_eq!(
            reference.words()[3],
            (1 << assets::FIRE_SUPPORTED_QUAD_COUNT) - 1
        );
        let template = fixture.assets.model_templates()[base as usize];
        let quads = &fixture.assets.model_quads()
            [template.quad_start as usize..(template.quad_start + template.quad_count) as usize];
        assert_eq!(
            quads
                .iter()
                .map(|q| q.material)
                .collect::<HashSet<_>>()
                .len(),
            2
        );
        assert!(
            quads
                .iter()
                .all(|q| q.flags & MODEL_QUAD_FLAG_FACE_MASK == 0)
        );
    }
}

#[test]
fn fire_attachment_masks_use_flammability_without_solid_face_occlusion() {
    let fixture = compiled_fire_fixture();
    let neighbours = [[6, 8, 9], [8, 8, 9], [7, 8, 8], [7, 8, 10], [7, 9, 9]];
    let base = fixture
        .assets
        .resolve(NetworkIdMode::Sequential, fixture.fire)
        .model_template()
        .unwrap();
    for mask in 0..assets::FIRE_ATTACHMENT_MASK_COUNT as u8 {
        let mut placements = vec![([7, 8, 9], 1)];
        for (index, coordinate) in neighbours.into_iter().enumerate() {
            if mask & (1 << index) != 0 {
                placements.push((coordinate, 2));
            }
        }
        let center = sub_chunk(vec![packed_storage(
            2,
            &[fixture.air, fixture.fire, fixture.wool],
            &placements,
        )]);
        let mesh = fire_mesh(fixture, &center, [0; 3]);
        if mask == 0 {
            assert!(
                fire_reference(fixture, &mesh).is_none(),
                "unsupported fire has no central cross"
            );
        } else {
            let reference = fire_reference(fixture, &mesh).unwrap();
            // (7,8,9) has even full sum, odd sum after native signed /2.
            let expected = base + assets::fire_attachment_template_offset(mask, false, true);
            assert_eq!(reference.words()[1], expected);
            let count = (mask & 15).count_ones() * 2 + u32::from(mask & 16 != 0) * 2;
            assert_eq!(reference.words()[3], (1 << count) - 1);
            assert_eq!(mesh.model_lighting().len(), count as usize);
        }
        assert!(
            meshing::mesh_output_byte_len(&mesh, &meshing::PackedBiomeRecord::fallback())
                <= meshing::MeshOutputBounds::new(&fixture.assets).for_sub_chunk(
                    &center,
                    &fixture.assets,
                    NetworkIdMode::Sequential
                )
        );
    }
    // Stone beside a fire does not attach it, even though its face is opaque.
    let center = sub_chunk(vec![packed_storage(
        2,
        &[fixture.air, fixture.fire, fixture.floor],
        &[([7, 8, 9], 1), ([6, 8, 9], 2)],
    )]);
    assert!(fire_reference(fixture, &fire_mesh(fixture, &center, [0; 3])).is_none());
}

#[test]
fn fire_attachment_uv_parity_uses_signed_world_position() {
    let fixture = compiled_fire_fixture();
    let center = sub_chunk(vec![packed_storage(
        2,
        &[fixture.air, fixture.fire, fixture.wool],
        &[([1, 2, 3], 1), ([0, 2, 3], 2)],
    )]);
    let mut u_coordinates = Vec::new();
    for origin in [[0; 3], [-(world::SUB_CHUNK_SIDE as i32), 0, 0]] {
        let mesh = fire_mesh(fixture, &center, origin);
        let reference = fire_reference(fixture, &mesh).unwrap();
        let template = fixture.assets.model_templates()[reference.words()[1] as usize];
        let quad = fixture.assets.model_quads()[template.quad_start as usize];
        u_coordinates.push(quad.uvs[0][0]);
    }
    assert_eq!(
        u_coordinates,
        [4096, 0],
        "negative odd /2 must truncate toward zero"
    );
}

#[test]
fn fire_floor_support_resolves_across_the_sub_chunk_boundary() {
    let fixture = compiled_fire_fixture();
    let center = sub_chunk(vec![packed_storage(
        1,
        &[fixture.air, fixture.fire],
        &[([7, 0, 9], 1)],
    )]);
    let below = sub_chunk(vec![packed_storage(
        1,
        &[fixture.air, fixture.floor],
        &[([7, (world::SUB_CHUNK_SIDE - 1) as u8, 9], 1)],
    )]);
    let mut neighbourhood = MeshNeighbourhood::new(&center).with_block_origin([0, -64, 0]);
    assert!(neighbourhood.insert([0, -1, 0], &below));
    let mesh = mesh_sub_chunk_in_neighbourhood(
        &BlockClassifier::new(fixture.air),
        &fixture.assets,
        NetworkIdMode::Sequential,
        &neighbourhood,
    );
    let base = fixture
        .assets
        .resolve(NetworkIdMode::Sequential, fixture.fire)
        .model_template()
        .unwrap();
    assert_eq!(fire_reference(fixture, &mesh).unwrap().words()[1], base);
}
