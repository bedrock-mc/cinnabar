struct SnowGrassFixture {
    assets: RuntimeAssets,
    air: u32,
    grass: u32,
    snow: u32,
    stone: u32,
    plant: u32,
    layers: [[u32; assets::TOP_SNOW_LAYER_COUNT as usize]; 2],
}

fn snow_grass_wire_id(mode: NetworkIdMode, sequential: u32) -> u32 {
    match mode {
        NetworkIdMode::Sequential => sequential,
        NetworkIdMode::Hashed => sequential + 0x10000,
    }
}

fn snow_grass_fixture() -> &'static SnowGrassFixture {
    static FIXTURE: OnceLock<SnowGrassFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let mut records = Vec::new();
        let mut add = |name: &str, state: String, flags: BlockFlags, family: ModelFamily| {
            let id = records.len() as u32;
            records.push(RegistryRecord {
                sequential_id: id,
                network_hash: snow_grass_wire_id(NetworkIdMode::Hashed, id),
                name: name.into(), canonical_state: state.into(), flags, model_family: family,
                contributor_role: if flags.contains(BlockFlags::AIR) {
                    assets::ContributorRole::Air
                } else { assets::ContributorRole::Primary },
                model_state: Default::default(), face_coverage: 0,
                collision_seed: Default::default(), provenance: assets::RegistryProvenance::PMMP,
            });
            id
        };
        let air = add("minecraft:air", "{}".into(), BlockFlags::AIR, ModelFamily::Air);
        let layers = std::array::from_fn(|covered| std::array::from_fn(|height| add(
            "minecraft:snow_layer",
            format!(r#"{{"covered_bit":{{"type":"byte","value":{covered}}},"height":{{"type":"int","value":{height}}}}}"#),
            BlockFlags::empty(), ModelFamily::Unknown)));
        let cube_flags = BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE;
        let grass = add("minecraft:grass_block", "{}".into(), cube_flags, ModelFamily::Cube);
        let snow = add("minecraft:snow", "{}".into(), cube_flags, ModelFamily::Cube);
        let stone = add("minecraft:stone", "{}".into(), cube_flags, ModelFamily::Cube);
        let plant = add("minecraft:fern", "{}".into(), BlockFlags::empty(), ModelFamily::Cross);
        let directory = tempfile::tempdir().unwrap();
        write_slab_render_pack(directory.path(), "snow_layer", "snow", "stone");
        fs::write(directory.path().join("blocks.json"),
            r#"{"grass":{"textures":{"down":"slab_down","side":"grass_side","up":"slab_up"}},"snow_layer":{"textures":"cube_all"},"snow":{"textures":"cube_all"},"stone":{"textures":"cube_all"},"fern":{"textures":"cube_all"}}"#
        ).unwrap();
        let texture_path = directory.path().join("textures/terrain_texture.json");
        fs::write(texture_path, format!(
            r##"{{"texture_data":{{"slab_down":{{"textures":"textures/blocks/slab_down"}},"slab_up":{{"textures":"textures/blocks/slab_up"}},"cube_all":{{"textures":"textures/blocks/cube_all"}},"grass_side":{{"textures":[{{"path":"textures/blocks/slab_side","overlay_color":"#df6827"}},"{}"]}}}}}}"##,
            assets::SNOWED_GRASS_SIDE_TEXTURE)).unwrap();
        fs::copy(directory.path().join("textures/blocks/cube_all.png"),
            directory.path().join(format!("{}.png", assets::SNOWED_GRASS_SIDE_TEXTURE))).unwrap();
        let compiled = compile_pack(directory.path(), &records).unwrap();
        let assets = RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap();
        SnowGrassFixture {assets, air, grass, snow, stone, plant, layers}
    })
}

fn snow_grass_chunk(mode: NetworkIdMode, layers: &[&[([u8; 3], u32)]]) -> SubChunk {
    let fixture = snow_grass_fixture();
    let air = snow_grass_wire_id(mode, fixture.air);
    let mut encoded = vec![9, layers.len() as u8, 0];
    for placements in layers {
        let mut palette = vec![air];
        let indexed = placements
            .iter()
            .map(|&(coordinate, id)| {
                let id = snow_grass_wire_id(mode, id);
                let index = palette
                    .iter()
                    .position(|&value| value == id)
                    .unwrap_or_else(|| {
                        palette.push(id);
                        palette.len() - 1
                    });
                (coordinate, index)
            })
            .collect::<Vec<_>>();
        encoded.extend(packed_storage(5, &palette, &indexed));
    }
    SubChunk::decode(&encoded, &RawBlockIds { air })
}

fn snow_grass_mesh(mode: NetworkIdMode, chunk: &SubChunk, above: Option<&SubChunk>) -> ChunkMesh {
    let fixture = snow_grass_fixture();
    let neighbours = above.map_or_else(Neighbourhood::empty, |above| {
        Neighbourhood::empty().with_positive_y(above)
    });
    mesh_sub_chunk(
        &BlockClassifier::new(snow_grass_wire_id(mode, fixture.air)),
        &fixture.assets,
        mode,
        &neighbours,
        chunk,
    )
}

fn assert_snow_grass_materials(mesh: &ChunkMesh, coordinate: [u8; 3], snowy: bool) {
    let fixture = snow_grass_fixture();
    let visual = fixture
        .assets
        .resolve(NetworkIdMode::Sequential, fixture.grass);
    let snowy_material = visual.variant() & assets::BLOCK_VISUAL_VARIANT_MATERIAL_MASK;
    assert_ne!(snowy_material, DIAGNOSTIC_MATERIAL);
    for face in Face::ALL {
        if let Some(quad) = mesh
            .cube_quads()
            .iter()
            .find(|quad| quad.origin() == coordinate && quad.face() == face)
        {
            let expected = if snowy && !matches!(face, Face::NegativeY | Face::PositiveY) {
                snowy_material
            } else {
                visual.face(BlockFace::ALL[face as usize]).material_id()
            };
            assert_eq!(quad.material_id(), expected, "{coordinate:?} {face:?}");
        } else {
            assert!(
                matches!(face, Face::PositiveY),
                "missing grass side {face:?}"
            );
        }
    }
    assert_eq!(fixture.assets.materials()[snowy_material as usize].flags, 0);
}

#[test]
fn snowy_grass_matches_all_native_snow_heights_and_full_cover_in_both_wire_modes() {
    let fixture = snow_grass_fixture();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        for cover in fixture
            .layers
            .iter()
            .flatten()
            .copied()
            .chain([fixture.snow])
        {
            let chunk =
                snow_grass_chunk(mode, &[&[([8, 8, 8], fixture.grass), ([8, 9, 8], cover)]]);
            let mesh = snow_grass_mesh(mode, &chunk, None);
            assert!(mesh.diagnostic_geometry().entries().is_empty());
            assert_snow_grass_materials(&mesh, [8, 8, 8], true);
        }
        for cover in [fixture.air, fixture.stone, fixture.plant] {
            let chunk = snow_grass_chunk(
                mode,
                &[&[
                    ([8, 8, 8], fixture.grass),
                    ([8, 9, 8], cover),
                    ([7, 9, 8], fixture.snow),
                    ([8, 7, 8], fixture.layers[0][0]),
                ]],
            );
            assert_snow_grass_materials(&snow_grass_mesh(mode, &chunk, None), [8, 8, 8], false);
        }
    }
}

#[test]
fn snowy_grass_above_subchunk_boundary_updates_when_snow_is_added_and_removed() {
    let fixture = snow_grass_fixture();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let chunk = snow_grass_chunk(mode, &[&[([8, 15, 8], fixture.grass)]]);
        let snowy = snow_grass_chunk(mode, &[&[([8, 0, 8], fixture.layers[0][0])]]);
        let empty = snow_grass_chunk(mode, &[&[]]);
        for above in [None, Some(&snowy), Some(&empty), Some(&snowy), None] {
            assert_snow_grass_materials(
                &snow_grass_mesh(mode, &chunk, above),
                [8, 15, 8],
                above.is_some_and(|above| std::ptr::eq(above, &snowy)),
            );
        }
    }
}

#[test]
fn snowy_grass_samples_snow_even_when_foliage_occupies_another_storage_layer() {
    let fixture = snow_grass_fixture();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        for &snow in &fixture.layers[1] {
            for reverse in [false, true] {
                let primary = [([8, 8, 8], fixture.grass), ([8, 9, 8], snow)];
                let plant = [([8, 9, 8], fixture.plant)];
                let layers = if reverse {
                    [plant.as_slice(), primary.as_slice()]
                } else {
                    [primary.as_slice(), plant.as_slice()]
                };
                let mesh = snow_grass_mesh(mode, &snow_grass_chunk(mode, &layers), None);
                assert!(mesh.diagnostic_geometry().entries().is_empty());
                assert_snow_grass_materials(&mesh, [8, 8, 8], true);
            }
        }
    }
}

#[test]
fn greedy_grass_faces_do_not_merge_across_snow_material_boundary() {
    let fixture = snow_grass_fixture();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let placements = [
            ([7, 8, 8], fixture.grass),
            ([8, 8, 8], fixture.grass),
            ([8, 9, 8], fixture.layers[0][0]),
        ];
        let mesh = snow_grass_mesh(mode, &snow_grass_chunk(mode, &[&placements]), None);
        for face in [Face::NegativeZ, Face::PositiveZ] {
            let sides = mesh
                .cube_quads()
                .iter()
                .filter(|quad| quad.face() == face && quad.origin()[1] == 8)
                .collect::<Vec<_>>();
            assert_eq!(sides.len(), 2);
            assert!(sides.iter().all(|quad| quad.width() == 1));
            assert_ne!(sides[0].material_id(), sides[1].material_id());
        }
    }
}
