struct CompiledSlabFixture {
    assets: RuntimeAssets,
    air: u32,
    lower: u32,
    upper: u32,
    full: u32,
    cube: u32,
}

fn write_slab_render_pack(root: &Path, slab_name: &str, double_name: &str, cube_name: &str) {
    fs::create_dir_all(root.join("textures/blocks")).expect("create slab render fixture tree");
    fs::write(
        root.join("blocks.json"),
        format!(
            r#"{{"{slab_name}":{{"textures":{{"down":"slab_down","side":"slab_side","up":"slab_up"}}}},"{double_name}":{{"textures":{{"down":"slab_down","side":"slab_side","up":"slab_up"}}}},"{cube_name}":{{"textures":"cube_all"}}}}"#
        ),
    )
    .expect("write slab render block routing");
    fs::write(
        root.join("textures/terrain_texture.json"),
        r#"{"texture_data":{"slab_down":{"textures":"textures/blocks/slab_down"},"slab_side":{"textures":"textures/blocks/slab_side"},"slab_up":{"textures":"textures/blocks/slab_up"},"cube_all":{"textures":"textures/blocks/cube_all"}}}"#,
    )
    .expect("write slab render terrain routing");
    fs::write(root.join("textures/flipbook_textures.json"), "[]")
        .expect("write empty slab flipbook inventory");

    for (index, name) in ["slab_down", "slab_side", "slab_up", "cube_all"]
        .into_iter()
        .enumerate()
    {
        let mut rgba = vec![0_u8; 16 * 16 * 4];
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[40 + index as u8 * 60, 80, 120, 255]);
        }
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&rgba, 16, 16, ExtendedColorType::Rgba8)
            .expect("encode slab fixture PNG");
        fs::write(root.join(format!("textures/blocks/{name}.png")), png)
            .expect("write slab fixture PNG");
    }
}

fn compiled_slab_fixture() -> &'static CompiledSlabFixture {
    static FIXTURE: OnceLock<CompiledSlabFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let generated = read_registry(include_bytes!("../../../assets/data/block-registry-v1001.bin"))
            .expect("decode committed slab registry");
        let air = generated
            .iter()
            .find(|record| record.name.as_ref() == "minecraft:air")
            .expect("committed registry air record")
            .clone();
        let lower = generated
            .iter()
            .find(|record| {
                record.model_family == ModelFamily::Slab
                    && record.model_state.get(ModelStateField::Half) == Some(0)
            })
            .expect("committed lower slab record")
            .clone();
        let upper = generated
            .iter()
            .find(|record| {
                record.name == lower.name
                    && record.model_state.get(ModelStateField::Half) == Some(1)
            })
            .expect("matching committed upper slab record")
            .clone();
        let full = generated
            .iter()
            .find(|record| {
                record.model_family == ModelFamily::Slab
                    && record.model_state.get(ModelStateField::Half) == Some(2)
            })
            .expect("committed full slab record")
            .clone();
        let cube = generated
            .iter()
            .find(|record| {
                record.model_family == ModelFamily::Cube
                    && record.flags.contains(BlockFlags::CUBE_GEOMETRY)
                    && record.flags.contains(BlockFlags::OCCLUDES_FULL_FACE)
            })
            .expect("committed full cube record")
            .clone();
        let slab_name = lower
            .name
            .strip_prefix("minecraft:")
            .expect("canonical slab namespace")
            .to_owned();
        let double_name = full
            .name
            .strip_prefix("minecraft:")
            .expect("canonical double slab namespace")
            .to_owned();
        let cube_name = cube
            .name
            .strip_prefix("minecraft:")
            .expect("canonical cube namespace")
            .to_owned();
        let directory = tempfile::tempdir().expect("create compiled slab render fixture");
        write_slab_render_pack(directory.path(), &slab_name, &double_name, &cube_name);
        let ids = [
            air.sequential_id,
            lower.sequential_id,
            upper.sequential_id,
            full.sequential_id,
            cube.sequential_id,
        ];
        let compiled = compile_pack(directory.path(), &[air, lower, upper, full, cube])
            .expect("compile slab fixture through assets compiler");
        let blob = encode_blob(&compiled).expect("encode compiled slab render fixture");
        CompiledSlabFixture {
            assets: RuntimeAssets::decode(&blob).expect("decode compiled slab render fixture"),
            air: ids[0],
            lower: ids[1],
            upper: ids[2],
            full: ids[3],
            cube: ids[4],
        }
    })
}

fn mesh_compiled_slab(runtime_id: u32, coordinates: &[[u8; 3]]) -> ChunkMesh {
    let fixture = compiled_slab_fixture();
    let placements = coordinates
        .iter()
        .copied()
        .map(|coordinate| (coordinate, 1))
        .collect::<Vec<_>>();
    let sub_chunk = sub_chunk(vec![packed_storage(
        1,
        &[fixture.air, runtime_id],
        &placements,
    )]);
    mesh_sub_chunk(
        &BlockClassifier::new(fixture.air),
        &fixture.assets,
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub_chunk,
    )
}

#[test]
fn compiled_slabs_emit_only_exact_bounded_model_and_lighting_streams() {
    let fixture = compiled_slab_fixture();
    for (runtime_id, expected_flags, full_occluder) in [
        (fixture.lower, [0x33, 0x44, 0x11, 0x02, 0x55, 0x66], false),
        (fixture.upper, [0x33, 0x44, 0x01, 0x22, 0x55, 0x66], false),
        (fixture.full, [0x33, 0x44, 0x11, 0x22, 0x55, 0x66], true),
    ] {
        let resolved = fixture
            .assets
            .resolve(NetworkIdMode::Sequential, runtime_id);
        assert_eq!(resolved.kind(), VisualKind::Model);
        assert_eq!(
            resolved.flags().contains(BlockFlags::OCCLUDES_FULL_FACE),
            full_occluder
        );
        assert!(!resolved.flags().contains(BlockFlags::CUBE_GEOMETRY));
        let template_id = resolved.model_template().expect("compiled slab template");
        let template = fixture.assets.model_templates()[template_id as usize];
        assert_eq!(template.quad_count, 6);
        assert_eq!(template.flags, 0);
        let quads = &fixture.assets.model_quads()
            [template.quad_start as usize..(template.quad_start + template.quad_count) as usize];
        assert_eq!(
            quads.iter().map(|quad| quad.flags).collect::<Vec<_>>(),
            expected_flags
        );
        for (index, face) in BlockFace::ALL.into_iter().enumerate() {
            assert_eq!(quads[index].material, resolved.face(face).material_id());
            assert_eq!(quads[index].flags & MODEL_QUAD_FLAG_TWO_SIDED, 0);
            assert_ne!(quads[index].flags & MODEL_QUAD_FLAG_FACE_MASK, 0);
            assert_eq!(
                quads[index].flags & !(MODEL_QUAD_FLAG_FACE_MASK | MODEL_QUAD_FLAG_CULL_FACE_MASK),
                0
            );
        }

        let mesh = mesh_compiled_slab(runtime_id, &[[2, 3, 4]]);
        assert!(mesh.cube_quads().is_empty());
        assert!(mesh.liquid_quads().is_empty());
        assert!(mesh.liquid_lighting().is_empty());
        assert_eq!(mesh.model_refs().len(), 1);
        assert_eq!(size_of::<PackedModelRef>(), 16);
        assert_eq!(
            mesh.model_refs()[0].words(),
            [2 | (3 << 4) | (4 << 8), template_id, 0, 0b11_1111]
        );
        assert_eq!(mesh.model_lighting().len(), 6);
        assert_eq!(size_of_val(mesh.model_lighting()), 6 * 8);
    }
}

#[test]
fn compiled_slabs_scale_one_ref_and_six_lighting_records_per_block() {
    let fixture = compiled_slab_fixture();
    let coordinates = [[1, 2, 3], [4, 5, 6], [7, 8, 9]];
    let mesh = mesh_compiled_slab(fixture.lower, &coordinates);
    assert_eq!(mesh.model_refs().len(), 3);
    assert_eq!(size_of_val(mesh.model_refs()), 3 * 16);
    assert_eq!(mesh.model_lighting().len(), 18);
    assert_eq!(size_of_val(mesh.model_lighting()), 18 * 8);
    for (index, reference) in mesh.model_refs().iter().enumerate() {
        assert_eq!(reference.words()[2], (index * 6) as u32);
        assert_eq!(reference.words()[3], 0b11_1111);
    }
}

fn mesh_compiled_fixture<'a>(sub_chunk: &SubChunk, neighbours: &Neighbourhood<'a>) -> ChunkMesh {
    let fixture = compiled_slab_fixture();
    mesh_sub_chunk(
        &BlockClassifier::new(fixture.air),
        &fixture.assets,
        NetworkIdMode::Sequential,
        neighbours,
        sub_chunk,
    )
}

#[test]
fn compiled_full_slab_and_cube_cull_shared_model_and_cube_faces_in_subchunk() {
    let fixture = compiled_slab_fixture();
    let center = sub_chunk(vec![packed_storage(
        2,
        &[fixture.air, fixture.full, fixture.cube],
        &[([7, 8, 8], 1), ([8, 8, 8], 2)],
    )]);
    let mesh = mesh_compiled_fixture(&center, &Neighbourhood::empty());

    assert_eq!(mesh.model_refs().len(), 1);
    assert_eq!(mesh.model_refs()[0].words()[2], 0);
    assert_eq!(mesh.model_refs()[0].words()[3], 0b11_1101);
    assert_eq!(mesh.model_lighting().len(), 6);
    assert_eq!(
        mesh.model_draw_refs()
            .iter()
            .copied()
            .map(PackedModelDrawRef::words)
            .collect::<Vec<_>>(),
        [[0, 0], [0, 2], [0, 3], [0, 4], [0, 5]]
    );
    assert_eq!(mesh.cube_quads().len(), 5);
    assert!(!has_face(&mesh, [8, 8, 8], Face::NegativeX));
    assert!(mesh.liquid_quads().is_empty());
}

#[test]
fn fully_occluded_model_emits_no_model_triplet_stream() {
    let fixture = compiled_slab_fixture();
    let center = sub_chunk(vec![packed_storage(
        2,
        &[fixture.air, fixture.full, fixture.cube],
        &[
            ([8, 8, 8], 1),
            ([7, 8, 8], 2),
            ([9, 8, 8], 2),
            ([8, 7, 8], 2),
            ([8, 9, 8], 2),
            ([8, 8, 7], 2),
            ([8, 8, 9], 2),
        ],
    )]);
    let mesh = mesh_compiled_fixture(&center, &Neighbourhood::empty());

    assert!(mesh.model_refs().is_empty());
    assert!(mesh.model_lighting().is_empty());
    assert!(mesh.model_draw_refs().is_empty());
}

#[test]
fn compiled_model_cull_faces_cross_subchunk_boundaries_without_reindexing_lighting() {
    let fixture = compiled_slab_fixture();
    let center = sub_chunk(vec![packed_storage(
        2,
        &[fixture.air, fixture.lower, fixture.full],
        &[([2, 3, 4], 1), ([15, 8, 8], 2)],
    )]);
    let positive_x = sub_chunk(vec![packed_storage(
        1,
        &[fixture.air, fixture.cube],
        &[([0, 8, 8], 1)],
    )]);
    let neighbourhood = Neighbourhood::empty().with_positive_x(&positive_x);
    let mesh = mesh_compiled_fixture(&center, &neighbourhood);

    assert_eq!(mesh.model_refs().len(), 2);
    assert_eq!(mesh.model_refs()[0].words()[2], 0);
    assert_eq!(mesh.model_refs()[0].words()[3], 0b11_1111);
    assert_eq!(mesh.model_refs()[1].words()[2], 6);
    assert_eq!(mesh.model_refs()[1].words()[3], 0b11_1101);
    assert_eq!(mesh.model_lighting().len(), 12);
    assert!(mesh.cube_quads().is_empty());

    let without_neighbour = mesh_compiled_fixture(&center, &Neighbourhood::empty());
    assert_eq!(without_neighbour.model_refs()[1].words()[3], 0b11_1111);
    assert_eq!(without_neighbour.model_refs()[1].words()[2], 6);
    assert_eq!(without_neighbour.model_lighting().len(), 12);
}

#[test]
fn compiled_model_cull_faces_map_all_six_subchunk_boundaries_for_cube_and_model_occluders() {
    let fixture = compiled_slab_fixture();
    for (quad_index, face, current, remote) in [
        (0, Face::NegativeX, [0, 8, 8], [15, 8, 8]),
        (1, Face::PositiveX, [15, 8, 8], [0, 8, 8]),
        (2, Face::NegativeY, [8, 0, 8], [8, 15, 8]),
        (3, Face::PositiveY, [8, 15, 8], [8, 0, 8]),
        (4, Face::NegativeZ, [8, 8, 0], [8, 8, 15]),
        (5, Face::PositiveZ, [8, 8, 15], [8, 8, 0]),
    ] {
        let center = sub_chunk(vec![packed_storage(
            1,
            &[fixture.air, fixture.full],
            &[(current, 1)],
        )]);
        for occluder in [fixture.cube, fixture.full] {
            let neighbour = sub_chunk(vec![packed_storage(
                1,
                &[fixture.air, occluder],
                &[(remote, 1)],
            )]);
            let neighbourhood = neighbourhood_for(face, &neighbour);
            let mesh = mesh_compiled_fixture(&center, &neighbourhood);
            assert_eq!(mesh.model_refs().len(), 1);
            assert_eq!(
                mesh.model_refs()[0].words()[3],
                0b11_1111 & !(1 << quad_index),
                "face={face:?} occluder={occluder}"
            );
            assert_eq!(mesh.model_refs()[0].words()[2], 0);
            assert_eq!(mesh.model_lighting().len(), 6);
        }
    }
}

#[test]
fn compiled_partial_slab_walls_are_cave_open_but_full_slab_walls_occlude() {
    let fixture = compiled_slab_fixture();
    let wall = (0..16)
        .flat_map(|y| (0..16).map(move |z| [8, y, z]))
        .collect::<Vec<_>>();
    for runtime_id in [fixture.lower, fixture.upper] {
        let mesh = mesh_compiled_slab(runtime_id, &wall);
        assert!(
            mesh.connectivity().is_all_connected(),
            "partial slabs must remain conservatively cave-open"
        );
    }

    let full = mesh_compiled_slab(fixture.full, &wall);
    assert!(
        !full
            .connectivity()
            .is_connected(Face::NegativeX, Face::PositiveX),
        "a complete wall of full slabs must separate opposite cave faces"
    );
}
