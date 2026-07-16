struct CompiledStainedGlassFixture {
    assets: RuntimeAssets,
    air: u32,
    red: u32,
    blue: u32,
    cube: u32,
}

fn write_stained_glass_render_pack(root: &Path, cube_name: &str) {
    fs::create_dir_all(root.join("textures/blocks"))
        .expect("create stained-glass render fixture tree");
    fs::write(
        root.join("blocks.json"),
        format!(
            r#"{{
                "red_stained_glass":{{"textures":"red_stained_glass"}},
                "blue_stained_glass":{{"textures":"blue_stained_glass"}},
                "{cube_name}":{{"textures":"cube"}}
            }}"#
        ),
    )
    .expect("write stained-glass block routing");
    fs::write(
        root.join("textures/terrain_texture.json"),
        r#"{"texture_data":{
            "red_stained_glass":{"textures":"textures/blocks/red_stained_glass"},
            "blue_stained_glass":{"textures":"textures/blocks/blue_stained_glass"},
            "cube":{"textures":"textures/blocks/cube"}
        }}"#,
    )
    .expect("write stained-glass terrain routing");
    fs::write(root.join("textures/flipbook_textures.json"), "[]")
        .expect("write stained-glass empty flipbooks");
    for (name, pixel) in [
        ("red_stained_glass", [180, 25, 35, 96]),
        ("blue_stained_glass", [25, 55, 180, 96]),
        ("cube", [90, 100, 110, 255]),
    ] {
        let rgba = pixel.repeat(16 * 16);
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&rgba, 16, 16, ExtendedColorType::Rgba8)
            .expect("encode stained-glass fixture PNG");
        fs::write(root.join(format!("textures/blocks/{name}.png")), png)
            .expect("write stained-glass fixture PNG");
    }
}

fn compiled_stained_glass_fixture() -> &'static CompiledStainedGlassFixture {
    static FIXTURE: OnceLock<CompiledStainedGlassFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let records = read_registry(include_bytes!("../../../assets/data/block-registry-v1001.bin"))
            .expect("decode stained-glass registry");
        let named = |name: &str| {
            records
                .iter()
                .find(|record| record.name.as_ref() == name)
                .unwrap_or_else(|| panic!("missing {name}"))
                .clone()
        };
        let air = named("minecraft:air");
        let red = named("minecraft:red_stained_glass");
        let blue = named("minecraft:blue_stained_glass");
        let cube = records
            .iter()
            .find(|record| {
                record.model_family == ModelFamily::Cube
                    && record.flags.contains(BlockFlags::OCCLUDES_FULL_FACE)
            })
            .expect("full cube")
            .clone();
        let cube_name = cube.name.strip_prefix("minecraft:").unwrap();
        let directory = tempfile::tempdir().expect("stained-glass fixture directory");
        write_stained_glass_render_pack(directory.path(), cube_name);
        let ids = [
            air.sequential_id,
            red.sequential_id,
            blue.sequential_id,
            cube.sequential_id,
        ];
        let compiled = compile_pack(directory.path(), &[air, red, blue, cube])
            .expect("compile stained-glass fixture");
        let blob = encode_blob(&compiled).expect("encode stained-glass fixture");
        let assets = RuntimeAssets::decode(&blob).expect("decode stained-glass fixture");
        for id in [ids[1], ids[2]] {
            let visual = assets.resolve(NetworkIdMode::Sequential, id);
            let template = visual.model_template().expect("stained-glass template");
            assert_eq!(
                assets.model_templates()[template as usize].flags,
                MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE
            );
        }
        CompiledStainedGlassFixture {
            assets,
            air: ids[0],
            red: ids[1],
            blue: ids[2],
            cube: ids[3],
        }
    })
}

fn mesh_stained_glass(placements: &[([u8; 3], u32)], neighbours: &Neighbourhood<'_>) -> ChunkMesh {
    let fixture = compiled_stained_glass_fixture();
    let mut palette = vec![fixture.air];
    let placements = placements
        .iter()
        .map(|&(coordinate, id)| {
            let palette_index = palette
                .iter()
                .position(|&value| value == id)
                .unwrap_or_else(|| {
                    palette.push(id);
                    palette.len() - 1
                });
            (coordinate, palette_index)
        })
        .collect::<Vec<_>>();
    let center = sub_chunk(vec![packed_storage(2, &palette, &placements)]);
    mesh_sub_chunk(
        &BlockClassifier::new(fixture.air),
        &fixture.assets,
        NetworkIdMode::Sequential,
        neighbours,
        &center,
    )
}

#[test]
fn equal_colour_stained_glass_suppresses_both_internal_faces() {
    let fixture = compiled_stained_glass_fixture();
    let mesh = mesh_stained_glass(
        &[([7, 8, 8], fixture.red), ([8, 8, 8], fixture.red)],
        &Neighbourhood::empty(),
    );

    assert_eq!(mesh.transparent_model_draw_refs().len(), 10);
    assert!(mesh.model_draw_refs().is_empty());
}

#[test]
fn different_colour_stained_glass_retains_both_boundary_faces() {
    let fixture = compiled_stained_glass_fixture();
    let mesh = mesh_stained_glass(
        &[([7, 8, 8], fixture.red), ([8, 8, 8], fixture.blue)],
        &Neighbourhood::empty(),
    );

    assert_eq!(mesh.transparent_model_draw_refs().len(), 12);
    assert!(mesh.model_draw_refs().is_empty());
}

#[test]
fn stained_glass_hides_behind_opaque_cube_without_hiding_opaque_face() {
    let fixture = compiled_stained_glass_fixture();
    let mesh = mesh_stained_glass(
        &[([7, 8, 8], fixture.red), ([8, 8, 8], fixture.cube)],
        &Neighbourhood::empty(),
    );

    assert_eq!(mesh.transparent_model_draw_refs().len(), 5);
    assert!(mesh.model_draw_refs().is_empty());
    assert_eq!(mesh.quad_count(), 6);
    assert!(has_face(&mesh, [8, 8, 8], Face::NegativeX));
}

#[test]
fn stained_glass_wall_remains_cave_open() {
    let fixture = compiled_stained_glass_fixture();
    let wall = (0..16)
        .flat_map(|y| (0..16).map(move |z| ([8, y, z], fixture.red)))
        .collect::<Vec<_>>();
    let mesh = mesh_stained_glass(&wall, &Neighbourhood::empty());

    assert!(mesh.connectivity().is_all_connected());
}

#[test]
fn equal_colour_stained_glass_culls_across_all_six_subchunk_boundaries() {
    let fixture = compiled_stained_glass_fixture();
    for (face, center_coordinate, neighbour_coordinate) in [
        (Face::NegativeX, [0, 5, 6], [15, 5, 6]),
        (Face::PositiveX, [15, 5, 6], [0, 5, 6]),
        (Face::NegativeY, [5, 0, 6], [5, 15, 6]),
        (Face::PositiveY, [5, 15, 6], [5, 0, 6]),
        (Face::NegativeZ, [5, 6, 0], [5, 6, 15]),
        (Face::PositiveZ, [5, 6, 15], [5, 6, 0]),
    ] {
        let neighbour = sub_chunk(vec![packed_storage(
            1,
            &[fixture.air, fixture.red],
            &[(neighbour_coordinate, 1)],
        )]);
        let mesh = mesh_stained_glass(
            &[(center_coordinate, fixture.red)],
            &neighbourhood_for(face, &neighbour),
        );

        assert_eq!(
            mesh.transparent_model_draw_refs().len(),
            5,
            "failed to cull {face:?}"
        );
        assert!(mesh.model_draw_refs().is_empty(), "face={face:?}");
    }
}
