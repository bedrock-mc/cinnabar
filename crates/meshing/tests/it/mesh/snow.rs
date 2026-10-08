struct CompiledSnowFixture {
    assets: RuntimeAssets,
    air: u32,
    layers: [u32; assets::TOP_SNOW_LAYER_COUNT as usize],
    covered_layers: [u32; assets::TOP_SNOW_LAYER_COUNT as usize],
    cube: u32,
    plants: [u32; 5],
}

fn compiled_snow_fixture() -> &'static CompiledSnowFixture {
    static FIXTURE: OnceLock<CompiledSnowFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let mut records = Vec::new();
        let mut add = |name: &str, state: String, flags: BlockFlags, family: ModelFamily| {
            let id = records.len() as u32;
            records.push(RegistryRecord {
                sequential_id: id,
                network_hash: id,
                name: name.into(),
                canonical_state: state.into(),
                flags,
                model_family: family,
                contributor_role: if flags.contains(BlockFlags::AIR) {
                    assets::ContributorRole::Air
                } else {
                    assets::ContributorRole::Primary
                },
                model_state: Default::default(),
                face_coverage: 0,
                collision_seed: Default::default(),
                provenance: assets::RegistryProvenance::PMMP,
            });
            id
        };
        let air = add("minecraft:air", "{}".into(), BlockFlags::AIR, ModelFamily::Air);
        let mut layer_ids = |covered: u8| {
            std::array::from_fn(|height| {
                add(
                    "minecraft:snow_layer",
                    format!(r#"{{"covered_bit":{{"type":"byte","value":{covered}}},"height":{{"type":"int","value":{height}}}}}"#),
                    BlockFlags::empty(),
                    ModelFamily::Unknown,
                )
            })
        };
        let layers = layer_ids(0);
        let covered_layers = layer_ids(1);
        let cube = add(
            "minecraft:stone", "{}".into(),
            BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
            ModelFamily::Cube,
        );
        let plant_names = ["fern", "short_grass", "poppy", "brown_mushroom", "dandelion"];
        let plants = plant_names.map(|name| {
            add(&format!("minecraft:{name}"), "{}".into(), BlockFlags::empty(), ModelFamily::Cross)
        });
        let directory = tempfile::tempdir().expect("snow fixture directory");
        // Original opaque fixture art only; the compiler selects native snow geometry.
        write_slab_render_pack(directory.path(), "snow_layer", "unused_snow", "stone");
        let plant_routes = plant_names.map(|name| format!(r#""{name}":{{"textures":"cube_all"}}"#)).join(",");
        fs::write(directory.path().join("blocks.json"), format!(
            r#"{{"snow_layer":{{"textures":{{"down":"slab_down","side":"slab_side","up":"slab_up"}}}},"stone":{{"textures":"cube_all"}},{plant_routes}}}"#
        )).expect("write covered vegetation fixture routing");
        let mut lights = vec![assets::LightProperties::default(); records.len()];
        let mushroom = records.iter().find(|record| record.name.as_ref() == "minecraft:brown_mushroom").unwrap();
        lights[mushroom.sequential_id as usize] = assets::LightProperties::new(1, 0).unwrap();
        let compiled = compile_pack_with_lights(directory.path(), &records, &lights).expect("compile snow fixture");
        let blob = encode_blob(&compiled).expect("encode snow fixture");
        CompiledSnowFixture {
            assets: RuntimeAssets::decode(&blob).expect("decode snow fixture"),
            air, layers, covered_layers, cube, plants,
        }
    })
}

fn snow_chunk(placements: &[([u8; 3], u32)]) -> SubChunk {
    let fixture = compiled_snow_fixture();
    let mut palette = vec![fixture.air];
    let mut indexed = Vec::new();
    for &(coordinate, id) in placements {
        let index = match palette.iter().position(|&value| value == id) {
            Some(index) => index,
            None => {
                palette.push(id);
                palette.len() - 1
            }
        };
        indexed.push((coordinate, index));
    }
    sub_chunk(vec![packed_storage(5, &palette, &indexed)])
}

fn mesh_snow(chunk: &SubChunk, neighbours: &Neighbourhood<'_>) -> ChunkMesh {
    let fixture = compiled_snow_fixture();
    mesh_sub_chunk(
        &BlockClassifier::new(fixture.air),
        &fixture.assets,
        NetworkIdMode::Sequential,
        neighbours,
        chunk,
    )
}

fn snow_model_mask(mesh: &ChunkMesh, coordinate: [u8; 3]) -> u32 {
    let [x, y, z] = coordinate;
    let transform = u32::from(x) | (u32::from(y) << 4) | (u32::from(z) << 8);
    mesh.model_refs()
        .iter()
        .find(|reference| reference.words()[0] & 0xfff == transform)
        .expect("snow model reference")
        .words()[3]
}

#[test]
fn compiled_snow_all_heights_preserve_native_bounds_uvs_and_covered_shape() {
    let fixture = compiled_snow_fixture();
    let mut layer_height = None;
    for (&id, &covered_id) in fixture.layers.iter().zip(&fixture.covered_layers) {
        let resolved = fixture.assets.resolve(NetworkIdMode::Sequential, id);
        let covered = fixture
            .assets
            .resolve(NetworkIdMode::Sequential, covered_id);
        assert_eq!(resolved.kind(), covered.kind());
        assert_eq!(resolved.flags(), covered.flags());
        assert_eq!(resolved.model_template(), covered.model_template());
        assert_eq!(resolved.variant(), assets::BLOCK_VISUAL_VARIANT_TOP_SNOW);
        assert_eq!(covered.variant(), assets::BLOCK_VISUAL_VARIANT_TOP_SNOW);
        if let Some(template_id) = resolved.model_template() {
            let template = fixture.assets.model_templates()[template_id as usize];
            assert_eq!(template.flags, assets::MODEL_TEMPLATE_FLAG_SNOW_LAYER);
            let quads = &fixture.assets.model_quads()[template.quad_start as usize
                ..(template.quad_start + template.quad_count) as usize];
            assert_eq!(
                quads.iter().map(|quad| quad.flags).collect::<Vec<_>>(),
                [0x33, 0x44, 0x11, 0x02, 0x55, 0x66]
            );
            let top = quads[3].positions[0][1];
            let step = *layer_height.get_or_insert(top);
            let ordinal = fixture
                .layers
                .iter()
                .position(|&value| value == id)
                .unwrap()
                + 1;
            assert_eq!(top, step * ordinal as i16);
            assert!(quads[3].positions.iter().all(|position| position[1] == top));
            assert!(quads[2].positions.iter().all(|position| position[1] == 0));
            // Side texture follows block Y; it is cropped, never stretched per layer.
            let uv_extent = i32::from(quads[3].uvs[1][1]);
            let uv_per_position = uv_extent / i32::from(quads[3].positions[1][2]);
            for corner in 0..4 {
                assert_eq!(
                    quads[0].uvs[corner][1],
                    (uv_extent - i32::from(quads[0].positions[corner][1]) * uv_per_position) as u16
                );
            }
        } else {
            assert_eq!(resolved.kind(), VisualKind::Cube);
            assert!(resolved.flags().contains(BlockFlags::OCCLUDES_FULL_FACE));
            assert_eq!(id, *fixture.layers.last().unwrap());
        }
    }
}

#[test]
fn snow_inset_top_survives_opaque_above_while_solid_support_hides_bottom() {
    let fixture = compiled_snow_fixture();
    for &id in &fixture.layers[..fixture.layers.len() - 1] {
        let chunk = snow_chunk(&[
            ([8, 8, 8], id),
            ([8, 7, 8], fixture.cube),
            ([8, 9, 8], fixture.cube),
            ([7, 8, 8], fixture.cube),
        ]);
        let mesh = mesh_snow(&chunk, &Neighbourhood::empty());
        assert_eq!(
            snow_model_mask(&mesh, [8, 8, 8]),
            0b11_1111 & !(1 << 2) & !(1 << 0)
        );
        assert_eq!(mesh.model_lighting().len(), 6);
    }
}

#[test]
fn touching_snow_sides_hide_only_when_neighbour_reaches_current_height() {
    let fixture = compiled_snow_fixture();
    for (left_height, &left) in fixture.layers.iter().enumerate() {
        for (right_height, &right) in fixture.layers.iter().enumerate() {
            let mesh = mesh_snow(
                &snow_chunk(&[([7, 8, 8], left), ([8, 8, 8], right)]),
                &Neighbourhood::empty(),
            );
            if fixture
                .assets
                .resolve(NetworkIdMode::Sequential, left)
                .kind()
                == VisualKind::Model
            {
                assert_eq!(
                    snow_model_mask(&mesh, [7, 8, 8]) & (1 << 1) == 0,
                    right_height >= left_height,
                    "left={left_height} right={right_height}"
                );
            } else {
                assert_eq!(
                    !has_face(&mesh, [7, 8, 8], Face::PositiveX),
                    right_height >= left_height
                );
            }
            if fixture
                .assets
                .resolve(NetworkIdMode::Sequential, right)
                .kind()
                == VisualKind::Model
            {
                assert_eq!(
                    snow_model_mask(&mesh, [8, 8, 8]) & (1 << 0) == 0,
                    left_height >= right_height,
                    "left={left_height} right={right_height}"
                );
            } else {
                assert_eq!(
                    !has_face(&mesh, [8, 8, 8], Face::NegativeX),
                    left_height >= right_height
                );
            }
        }
    }
}

#[test]
fn full_height_snow_top_and_bottom_use_touching_cube_boundaries() {
    let fixture = compiled_snow_fixture();
    let full = *fixture.layers.last().unwrap();
    let mesh = mesh_snow(
        &snow_chunk(&[
            ([8, 8, 8], full),
            ([8, 7, 8], fixture.cube),
            ([8, 9, 8], fixture.cube),
        ]),
        &Neighbourhood::empty(),
    );
    assert!(!has_face(&mesh, [8, 8, 8], Face::NegativeY));
    assert!(!has_face(&mesh, [8, 8, 8], Face::PositiveY));
}

#[test]
fn snow_culling_crosses_all_subchunk_boundaries_without_lighting_reindexing() {
    let fixture = compiled_snow_fixture();
    for (index, face, current, remote) in [
        (0, Face::NegativeX, [0, 8, 8], [15, 8, 8]),
        (1, Face::PositiveX, [15, 8, 8], [0, 8, 8]),
        (2, Face::NegativeY, [8, 0, 8], [8, 15, 8]),
        (3, Face::PositiveY, [8, 15, 8], [8, 0, 8]),
        (4, Face::NegativeZ, [8, 8, 0], [8, 8, 15]),
        (5, Face::PositiveZ, [8, 8, 15], [8, 8, 0]),
    ] {
        let center = snow_chunk(&[(current, fixture.layers[0])]);
        let opaque = snow_chunk(&[(remote, fixture.cube)]);
        let mesh = mesh_snow(&center, &neighbourhood_for(face, &opaque));
        let expected = if face == Face::PositiveY {
            0b11_1111
        } else {
            0b11_1111 & !(1 << index)
        };
        assert_eq!(snow_model_mask(&mesh, current), expected, "face={face:?}");
        assert_eq!(mesh.model_refs()[0].words()[2], 0);
        assert_eq!(mesh.model_lighting().len(), 6);
        if !matches!(face, Face::NegativeY | Face::PositiveY) {
            for &neighbor_id in &fixture.layers {
                let snow = snow_chunk(&[(remote, neighbor_id)]);
                let mesh = mesh_snow(&center, &neighbourhood_for(face, &snow));
                assert_eq!(snow_model_mask(&mesh, current), 0b11_1111 & !(1 << index));
            }
        }
    }
}
