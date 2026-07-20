struct CompiledVineFixture {
    assets: RuntimeAssets,
    air: u32,
    cube: u32,
    by_mask: [u32; 16],
}

#[test]
fn compiled_blank_signs_emit_exact_bounded_model_and_lighting_streams() {
    let generated = read_registry(include_bytes!("../../../assets/data/block-registry-v1001.bin"))
        .expect("decode committed sign registry");
    let air = generated
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:air")
        .expect("committed air record")
        .clone();
    let standing = generated
        .iter()
        .find(|record| {
            record.name.as_ref() == "minecraft:standing_sign"
                && record.model_state.get(ModelStateField::Orientation) == Some(7)
        })
        .expect("standing sign rotation seven")
        .clone();
    let wall = generated
        .iter()
        .find(|record| {
            record.name.as_ref() == "minecraft:wall_sign"
                && record.model_state.get(ModelStateField::Orientation) == Some(4)
        })
        .expect("west wall sign")
        .clone();
    let hanging = generated
        .iter()
        .find(|record| {
            record.name.as_ref() == "minecraft:oak_hanging_sign"
                && record.model_state.get(ModelStateField::Orientation) == Some(9 | (3 << 4))
                && record.model_state.get(ModelStateField::Flags) == Some((1 << 2) | (1 << 3))
        })
        .expect("attached ceiling hanging sign")
        .clone();

    let directory = tempfile::tempdir().expect("create sign render fixture");
    fs::create_dir_all(directory.path().join("textures/blocks"))
        .expect("create sign render texture tree");
    fs::write(
        directory.path().join("blocks.json"),
        r#"{"standing_sign":{"textures":"sign_texture"},"wall_sign":{"textures":"sign_texture"},"oak_hanging_sign":{"textures":"sign_texture"}}"#,
    )
    .expect("write sign block routing");
    fs::write(
        directory.path().join("textures/terrain_texture.json"),
        r#"{"texture_data":{"sign_texture":{"textures":"textures/blocks/sign_texture"}}}"#,
    )
    .expect("write sign terrain routing");
    fs::write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    )
    .expect("write empty sign flipbooks");
    let mut rgba = vec![0_u8; 16 * 16 * 4];
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.copy_from_slice(&[139, 98, 55, 255]);
    }
    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(&rgba, 16, 16, ExtendedColorType::Rgba8)
        .expect("encode sign fixture texture");
    fs::write(
        directory.path().join("textures/blocks/sign_texture.png"),
        png,
    )
    .expect("write sign fixture texture");

    let ids = [
        standing.sequential_id,
        wall.sequential_id,
        hanging.sequential_id,
    ];
    let compiled = compile_pack(directory.path(), &[air.clone(), standing, wall, hanging])
        .expect("compile representative blank signs");
    let runtime = RuntimeAssets::decode(&encode_blob(&compiled).expect("encode sign fixture"))
        .expect("decode sign fixture");
    for (&runtime_id, expected_quads) in ids.iter().zip([12_usize, 6, 18]) {
        let resolved = runtime.resolve(NetworkIdMode::Sequential, runtime_id);
        assert_eq!(resolved.kind(), VisualKind::Model);
        assert!(!resolved.flags().intersects(
            BlockFlags::AIR | BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE
        ));
        let template_id = resolved.model_template().expect("compiled sign template");
        let template = runtime.model_templates()[template_id as usize];
        assert_eq!(template.quad_count as usize, expected_quads);
        let center = sub_chunk(vec![packed_storage(
            1,
            &[air.sequential_id, runtime_id],
            &[([7, 8, 9], 1)],
        )]);
        let mesh = mesh_sub_chunk(
            &BlockClassifier::new(air.sequential_id),
            &runtime,
            NetworkIdMode::Sequential,
            &Neighbourhood::empty(),
            &center,
        );
        assert!(mesh.cube_quads().is_empty());
        assert!(mesh.liquid_quads().is_empty());
        assert_eq!(mesh.model_refs().len(), 1);
        assert_eq!(mesh.model_refs()[0].words()[1], template_id);
        assert_eq!(mesh.model_lighting().len(), expected_quads);
        assert!(mesh.connectivity().is_all_connected());
    }
}

fn write_vine_render_pack(root: &Path, cube_name: &str) {
    fs::create_dir_all(root.join("textures/blocks")).expect("create vine render fixture tree");
    fs::write(
        root.join("blocks.json"),
        format!(r#"{{"vine":{{"textures":"vine"}},"{cube_name}":{{"textures":"cube"}}}}"#),
    )
    .expect("write vine render block routing");
    fs::write(
        root.join("textures/terrain_texture.json"),
        r#"{"texture_data":{"vine":{"textures":"textures/blocks/vine"},"cube":{"textures":"textures/blocks/cube"}}}"#,
    )
    .expect("write vine render terrain routing");
    fs::write(root.join("textures/flipbook_textures.json"), "[]")
        .expect("write empty vine flipbook inventory");

    for (index, name) in ["vine", "cube"].into_iter().enumerate() {
        let mut rgba = vec![0_u8; 16 * 16 * 4];
        for (pixel_index, pixel) in rgba.chunks_exact_mut(4).enumerate() {
            let x = (pixel_index % 16) as u8;
            let y = (pixel_index / 16) as u8;
            pixel.copy_from_slice(&[20 + index as u8 * 90 + x, 40 + y, 80, 255]);
        }
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&rgba, 16, 16, ExtendedColorType::Rgba8)
            .expect("encode vine render fixture PNG");
        fs::write(root.join(format!("textures/blocks/{name}.png")), png)
            .expect("write vine render fixture PNG");
    }
}

fn compiled_vine_fixture() -> &'static CompiledVineFixture {
    static FIXTURE: OnceLock<CompiledVineFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let generated = read_registry(include_bytes!("../../../assets/data/block-registry-v1001.bin"))
            .expect("decode committed vine registry");
        let air = generated
            .iter()
            .find(|record| record.name.as_ref() == "minecraft:air")
            .expect("committed registry air record")
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
        let mut vines = generated
            .iter()
            .filter(|record| record.model_family == ModelFamily::Vine)
            .cloned()
            .collect::<Vec<_>>();
        vines.sort_unstable_by_key(|record| {
            record
                .model_state
                .get(ModelStateField::Connections)
                .expect("vine direction bits")
        });
        assert_eq!(vines.len(), 16, "protocol-1001 vine state count");
        let by_mask = std::array::from_fn(|mask| {
            assert_eq!(
                vines[mask].model_state.get(ModelStateField::Connections),
                Some(mask as u32)
            );
            vines[mask].sequential_id
        });
        let cube_name = cube
            .name
            .strip_prefix("minecraft:")
            .expect("canonical cube namespace");
        let directory = tempfile::tempdir().expect("create compiled vine render fixture");
        write_vine_render_pack(directory.path(), cube_name);
        let mut records = Vec::with_capacity(18);
        records.push(air.clone());
        records.extend(vines);
        records.push(cube.clone());
        let compiled = compile_pack(directory.path(), &records)
            .expect("compile all vine states through assets compiler");
        let blob = encode_blob(&compiled).expect("encode compiled vine render fixture");
        CompiledVineFixture {
            assets: RuntimeAssets::decode(&blob).expect("decode compiled vine render fixture"),
            air: air.sequential_id,
            cube: cube.sequential_id,
            by_mask,
        }
    })
}

fn mesh_compiled_vine(
    runtime_id: u32,
    coordinates: &[[u8; 3]],
    neighbours: &Neighbourhood<'_>,
) -> ChunkMesh {
    let fixture = compiled_vine_fixture();
    let placements = coordinates
        .iter()
        .copied()
        .map(|coordinate| (coordinate, 1))
        .collect::<Vec<_>>();
    let center = sub_chunk(vec![packed_storage(
        1,
        &[fixture.air, runtime_id],
        &placements,
    )]);
    mesh_sub_chunk(
        &BlockClassifier::new(fixture.air),
        &fixture.assets,
        NetworkIdMode::Sequential,
        neighbours,
        &center,
    )
}

#[test]
fn compiled_vines_cover_all_masks_with_exact_cpu_model_streams_and_zero_mask_no_draw() {
    let fixture = compiled_vine_fixture();
    for (mask, &runtime_id) in fixture.by_mask.iter().enumerate() {
        let resolved = fixture
            .assets
            .resolve(NetworkIdMode::Sequential, runtime_id);
        assert_eq!(resolved.kind(), VisualKind::Model, "mask {mask}");
        assert!(
            !resolved
                .flags()
                .intersects(BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE)
        );
        let template_id = resolved.model_template().expect("compiled vine template");
        let quad_count = (mask as u32).count_ones();
        assert_eq!(
            fixture.assets.model_templates()[template_id as usize].quad_count,
            quad_count,
            "mask {mask}"
        );

        let mesh = mesh_compiled_vine(runtime_id, &[[7, 8, 9]], &Neighbourhood::empty());
        assert!(mesh.cube_quads().is_empty(), "mask {mask}");
        assert!(mesh.liquid_quads().is_empty(), "mask {mask}");
        if mask == 0 {
            assert!(mesh.model_refs().is_empty());
            assert!(mesh.model_lighting().is_empty());
            assert!(
                mesh.is_empty(),
                "zero-mask vine must allocate no draw stream"
            );
        } else {
            assert_eq!(mesh.model_refs().len(), 1, "mask {mask}");
            assert_eq!(
                mesh.model_refs()[0].words(),
                [
                    7 | (8 << 4) | (9 << 8),
                    template_id,
                    0,
                    (1_u32 << quad_count) - 1,
                ],
                "mask {mask}"
            );
            assert_eq!(
                mesh.model_lighting().len(),
                quad_count as usize,
                "mask {mask}"
            );
        }
        assert!(
            mesh.connectivity().is_all_connected(),
            "mask {mask}: vines must not close cave-connectivity faces"
        );
    }
}

#[test]
fn compiled_vines_remain_drawable_on_every_subchunk_boundary_next_to_full_occluders() {
    let fixture = compiled_vine_fixture();
    let coordinates = [
        [0, 8, 8],
        [15, 8, 8],
        [8, 0, 8],
        [8, 15, 8],
        [8, 8, 0],
        [8, 8, 15],
    ];
    let opaque = sub_chunk(vec![uniform_storage(fixture.cube)]);
    let neighbourhood = Neighbourhood::empty()
        .with_negative_x(&opaque)
        .with_positive_x(&opaque)
        .with_negative_y(&opaque)
        .with_positive_y(&opaque)
        .with_negative_z(&opaque)
        .with_positive_z(&opaque);
    let expected_origins = coordinates
        .iter()
        .map(|[x, y, z]| u32::from(*x) | (u32::from(*y) << 4) | (u32::from(*z) << 8))
        .collect::<HashSet<_>>();

    for (mask, &runtime_id) in fixture.by_mask.iter().enumerate() {
        let quad_count = (mask as u32).count_ones();
        let mesh = mesh_compiled_vine(runtime_id, &coordinates, &neighbourhood);
        assert!(mesh.cube_quads().is_empty(), "mask {mask}");
        if mask == 0 {
            assert!(mesh.model_refs().is_empty());
            assert!(mesh.model_lighting().is_empty());
            continue;
        }
        assert_eq!(mesh.model_refs().len(), coordinates.len(), "mask {mask}");
        assert_eq!(
            mesh.model_refs()
                .iter()
                .map(|reference| reference.words()[0])
                .collect::<HashSet<_>>(),
            expected_origins,
            "mask {mask}: boundary positions"
        );
        for (index, reference) in mesh.model_refs().iter().enumerate() {
            assert_eq!(
                reference.words()[2],
                index as u32 * quad_count,
                "mask {mask}"
            );
            assert_eq!(
                reference.words()[3],
                (1_u32 << quad_count) - 1,
                "mask {mask}: a full neighbour must not cull a two-sided attachment plane"
            );
        }
        assert_eq!(
            mesh.model_lighting().len(),
            coordinates.len() * quad_count as usize,
            "mask {mask}"
        );
        assert!(mesh.connectivity().is_all_connected(), "mask {mask}");
    }
}
