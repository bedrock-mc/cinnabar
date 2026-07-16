#[derive(Clone, Copy)]
struct NetworkValues {
    sequential: u32,
    hashed: u32,
}

impl NetworkValues {
    const fn for_mode(self, mode: NetworkIdMode) -> u32 {
        match mode {
            NetworkIdMode::Sequential => self.sequential,
            NetworkIdMode::Hashed => self.hashed,
        }
    }
}

struct CompiledCopperGrateFixture {
    assets: RuntimeAssets,
    air: NetworkValues,
    copper: NetworkValues,
    exposed: NetworkValues,
    waxed_copper: NetworkValues,
    cube: NetworkValues,
}

fn write_copper_grate_render_pack(root: &Path, cube_name: &str) {
    fs::create_dir_all(root.join("textures/blocks")).expect("create copper-grate fixture tree");
    fs::write(
        root.join("blocks.json"),
        format!(
            r#"{{
                "copper_grate":{{"textures":"copper_grate"}},
                "exposed_copper_grate":{{"textures":"exposed_copper_grate"}},
                "waxed_copper_grate":{{"textures":"copper_grate"}},
                "{cube_name}":{{"textures":"cube"}}
            }}"#
        ),
    )
    .expect("write copper-grate block routing");
    fs::write(
        root.join("textures/terrain_texture.json"),
        r#"{"texture_data":{
            "copper_grate":{"textures":"textures/blocks/copper_grate"},
            "exposed_copper_grate":{"textures":"textures/blocks/exposed_copper_grate"},
            "cube":{"textures":"textures/blocks/cube"}
        }}"#,
    )
    .expect("write copper-grate terrain routing");
    fs::write(root.join("textures/flipbook_textures.json"), "[]")
        .expect("write copper-grate empty flipbooks");
    for (name, pixel) in [
        ("copper_grate", [180, 90, 45, 80]),
        ("exposed_copper_grate", [90, 145, 105, 80]),
        ("cube", [90, 100, 110, 255]),
    ] {
        let rgba = pixel.repeat(16 * 16);
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&rgba, 16, 16, ExtendedColorType::Rgba8)
            .expect("encode copper-grate fixture PNG");
        fs::write(root.join(format!("textures/blocks/{name}.png")), png)
            .expect("write copper-grate fixture PNG");
    }
}

fn compiled_copper_grate_fixture() -> &'static CompiledCopperGrateFixture {
    static FIXTURE: OnceLock<CompiledCopperGrateFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let records = read_registry(include_bytes!("../../../assets/data/block-registry-v1001.bin"))
            .expect("decode copper-grate registry");
        let named = |name: &str| {
            records
                .iter()
                .find(|record| record.name.as_ref() == name)
                .unwrap_or_else(|| panic!("missing {name}"))
                .clone()
        };
        let air = named("minecraft:air");
        let copper = named("minecraft:copper_grate");
        let exposed = named("minecraft:exposed_copper_grate");
        let waxed_copper = named("minecraft:waxed_copper_grate");
        let cube = records
            .iter()
            .find(|record| {
                record.model_family == ModelFamily::Cube
                    && record.flags.contains(BlockFlags::OCCLUDES_FULL_FACE)
            })
            .expect("full cube")
            .clone();
        let values = |record: &assets::RegistryRecord| NetworkValues {
            sequential: record.sequential_id,
            hashed: record.network_hash,
        };
        let cube_name = cube.name.strip_prefix("minecraft:").unwrap();
        let directory = tempfile::tempdir().expect("copper-grate fixture directory");
        write_copper_grate_render_pack(directory.path(), cube_name);
        let fixture_values = [
            values(&air),
            values(&copper),
            values(&exposed),
            values(&waxed_copper),
            values(&cube),
        ];
        let compiled = compile_pack(
            directory.path(),
            &[air, copper, exposed, waxed_copper, cube],
        )
        .expect("compile copper-grate fixture");
        let blob = encode_blob(&compiled).expect("encode copper-grate fixture");
        let assets = RuntimeAssets::decode(&blob).expect("decode copper-grate fixture");
        for id in fixture_values[1..4].iter().map(|values| values.sequential) {
            let visual = assets.resolve(NetworkIdMode::Sequential, id);
            let template = visual.model_template().expect("copper-grate template");
            assert_eq!(
                assets.model_templates()[template as usize].flags,
                MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE
            );
            assert!(BlockFace::ALL.into_iter().all(|face| {
                let flags = assets.material(visual.face(face).material_id()).flags;
                flags & MATERIAL_FLAG_ALPHA_CUTOUT != 0 && flags & MATERIAL_FLAG_ALPHA_BLEND == 0
            }));
        }
        let face_materials = |network_value| {
            let visual = assets.resolve(NetworkIdMode::Sequential, network_value);
            BlockFace::ALL.map(|face| visual.face(face).material_id())
        };
        assert_eq!(
            face_materials(fixture_values[1].sequential),
            face_materials(fixture_values[3].sequential)
        );
        CompiledCopperGrateFixture {
            assets,
            air: fixture_values[0],
            copper: fixture_values[1],
            exposed: fixture_values[2],
            waxed_copper: fixture_values[3],
            cube: fixture_values[4],
        }
    })
}

fn mesh_copper_grates(
    mode: NetworkIdMode,
    placements: &[([u8; 3], NetworkValues)],
    neighbours: &Neighbourhood<'_>,
) -> ChunkMesh {
    let fixture = compiled_copper_grate_fixture();
    let air = fixture.air.for_mode(mode);
    let mut palette = vec![air];
    let placements = placements
        .iter()
        .map(|&(coordinate, values)| {
            let id = values.for_mode(mode);
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
        &BlockClassifier::new(air),
        &fixture.assets,
        mode,
        neighbours,
        &center,
    )
}

#[test]
fn identical_copper_grate_states_cull_to_ten_cutout_draws_in_both_network_modes() {
    let fixture = compiled_copper_grate_fixture();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let mesh = mesh_copper_grates(
            mode,
            &[([7, 8, 8], fixture.copper), ([8, 8, 8], fixture.copper)],
            &Neighbourhood::empty(),
        );
        assert_eq!(mesh.model_draw_refs().len(), 10, "mode={mode:?}");
        assert!(
            mesh.transparent_model_draw_refs().is_empty(),
            "mode={mode:?}"
        );
    }
}

#[test]
fn different_oxidation_copper_grates_retain_twelve_cutout_draws_in_both_network_modes() {
    let fixture = compiled_copper_grate_fixture();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let mesh = mesh_copper_grates(
            mode,
            &[([7, 8, 8], fixture.copper), ([8, 8, 8], fixture.exposed)],
            &Neighbourhood::empty(),
        );
        assert_eq!(mesh.model_draw_refs().len(), 12, "mode={mode:?}");
        assert!(
            mesh.transparent_model_draw_refs().is_empty(),
            "mode={mode:?}"
        );
    }
}

#[test]
fn waxed_and_unwaxed_copper_grates_retain_twelve_draws_despite_equal_materials() {
    let fixture = compiled_copper_grate_fixture();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let mesh = mesh_copper_grates(
            mode,
            &[
                ([7, 8, 8], fixture.copper),
                ([8, 8, 8], fixture.waxed_copper),
            ],
            &Neighbourhood::empty(),
        );
        assert_eq!(mesh.model_draw_refs().len(), 12, "mode={mode:?}");
        assert!(
            mesh.transparent_model_draw_refs().is_empty(),
            "mode={mode:?}"
        );
    }
}

#[test]
fn copper_grate_hides_behind_opaque_cube_and_wall_remains_cave_open() {
    let fixture = compiled_copper_grate_fixture();
    let asymmetric = mesh_copper_grates(
        NetworkIdMode::Sequential,
        &[([7, 8, 8], fixture.copper), ([8, 8, 8], fixture.cube)],
        &Neighbourhood::empty(),
    );
    assert_eq!(asymmetric.model_draw_refs().len(), 5);
    assert!(asymmetric.transparent_model_draw_refs().is_empty());
    assert_eq!(asymmetric.quad_count(), 6);
    assert!(has_face(&asymmetric, [8, 8, 8], Face::NegativeX));

    let wall = (0..16)
        .flat_map(|y| (0..16).map(move |z| ([8, y, z], fixture.copper)))
        .collect::<Vec<_>>();
    let wall = mesh_copper_grates(NetworkIdMode::Sequential, &wall, &Neighbourhood::empty());
    assert!(wall.connectivity().is_all_connected());
    assert!(wall.transparent_model_draw_refs().is_empty());
}

#[test]
fn identical_copper_grate_state_culls_across_all_six_subchunk_boundaries_and_modes() {
    let fixture = compiled_copper_grate_fixture();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let air = fixture.air.for_mode(mode);
        let copper = fixture.copper.for_mode(mode);
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
                &[air, copper],
                &[(neighbour_coordinate, 1)],
            )]);
            let mesh = mesh_copper_grates(
                mode,
                &[(center_coordinate, fixture.copper)],
                &neighbourhood_for(face, &neighbour),
            );
            assert_eq!(
                mesh.model_draw_refs().len(),
                5,
                "mode={mode:?} face={face:?}"
            );
            assert!(mesh.transparent_model_draw_refs().is_empty());
        }
    }
}
