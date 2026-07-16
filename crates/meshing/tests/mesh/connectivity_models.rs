struct CompiledConnectedFixture {
    assets: RuntimeAssets,
    air: u32,
    pane: u32,
    other_pane: u32,
    wood_fence: u32,
    nether_fence: u32,
    gate_facing_z: u32,
    gate_facing_x: u32,
    wall: u32,
    cube: u32,
}

fn write_connected_render_pack(root: &Path, cube_name: &str) {
    fs::create_dir_all(root.join("textures/blocks")).expect("create connected fixture tree");
    fs::write(
        root.join("blocks.json"),
        format!(
            r#"{{
                "glass_pane":{{"textures":{{"west":"pane_body","east":"pane_edge","down":"pane_edge","up":"pane_edge","north":"pane_body","south":"pane_body"}}}},
                "white_stained_glass_pane":{{"textures":{{"west":"other_pane_body","east":"other_pane_edge","down":"other_pane_edge","up":"other_pane_edge","north":"other_pane_body","south":"other_pane_body"}}}},
                "oak_fence":{{"textures":"oak_fence"}},
                "nether_brick_fence":{{"textures":"nether_fence"}},
                "fence_gate":{{"textures":"oak_fence_gate"}},
                "cobblestone_wall":{{"textures":"wall"}},
                "{cube_name}":{{"textures":"cube"}}
            }}"#
        ),
    )
    .expect("write connected block routing");
    fs::write(
        root.join("textures/terrain_texture.json"),
        r#"{"texture_data":{
            "pane_body":{"textures":"textures/blocks/pane_body"},
            "pane_edge":{"textures":"textures/blocks/pane_edge"},
            "other_pane_body":{"textures":"textures/blocks/other_pane_body"},
            "other_pane_edge":{"textures":"textures/blocks/other_pane_edge"},
            "oak_fence":{"textures":"textures/blocks/oak_fence"},
            "nether_fence":{"textures":"textures/blocks/nether_fence"},
            "oak_fence_gate":{"textures":"textures/blocks/oak_fence_gate"},
            "wall":{"textures":"textures/blocks/wall"},
            "cube":{"textures":"textures/blocks/cube"}
        }}"#,
    )
    .expect("write connected terrain routing");
    fs::write(root.join("textures/flipbook_textures.json"), "[]")
        .expect("write connected empty flipbooks");
    for (index, name) in [
        "pane_body",
        "pane_edge",
        "other_pane_body",
        "other_pane_edge",
        "oak_fence",
        "nether_fence",
        "oak_fence_gate",
        "wall",
        "cube",
    ]
    .into_iter()
    .enumerate()
    {
        let mut rgba = vec![0_u8; 16 * 16 * 4];
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[20 + index as u8 * 25, 70, 110, 255]);
        }
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&rgba, 16, 16, ExtendedColorType::Rgba8)
            .expect("encode connected fixture PNG");
        fs::write(root.join(format!("textures/blocks/{name}.png")), png)
            .expect("write connected fixture PNG");
    }
}

fn compiled_connected_fixture() -> &'static CompiledConnectedFixture {
    static FIXTURE: OnceLock<CompiledConnectedFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let records = read_registry(include_bytes!("../../../assets/data/block-registry-v1001.bin"))
            .expect("decode connected registry");
        let named = |name: &str| {
            records
                .iter()
                .find(|record| record.name.as_ref() == name)
                .unwrap_or_else(|| panic!("missing {name}"))
                .clone()
        };
        let air = named("minecraft:air");
        let pane = named("minecraft:glass_pane");
        let other_pane = named("minecraft:white_stained_glass_pane");
        let wood_fence = named("minecraft:oak_fence");
        let nether_fence = named("minecraft:nether_brick_fence");
        let gate = |orientation| {
            records
                .iter()
                .find(|record| {
                    record.name.as_ref() == "minecraft:fence_gate"
                        && record.model_state.get(ModelStateField::Orientation) == Some(orientation)
                        && record.model_state.get(ModelStateField::Open) == Some(0)
                        && record.model_state.get(ModelStateField::Flags) == Some(0)
                })
                .unwrap_or_else(|| panic!("missing closed oak gate orientation {orientation}"))
                .clone()
        };
        let gate_facing_z = gate(0);
        let gate_facing_x = gate(1);
        let wall = named("minecraft:cobblestone_wall");
        let cube = records
            .iter()
            .find(|record| {
                record.model_family == ModelFamily::Cube
                    && record.flags.contains(BlockFlags::OCCLUDES_FULL_FACE)
            })
            .expect("full cube")
            .clone();
        let cube_name = cube.name.strip_prefix("minecraft:").unwrap();
        let directory = tempfile::tempdir().expect("connected fixture directory");
        write_connected_render_pack(directory.path(), cube_name);
        let ids = [
            air.sequential_id,
            pane.sequential_id,
            other_pane.sequential_id,
            wood_fence.sequential_id,
            nether_fence.sequential_id,
            gate_facing_z.sequential_id,
            gate_facing_x.sequential_id,
            wall.sequential_id,
            cube.sequential_id,
        ];
        let compiled = compile_pack(
            directory.path(),
            &[
                air,
                pane,
                other_pane,
                wood_fence,
                nether_fence,
                gate_facing_z,
                gate_facing_x,
                wall,
                cube,
            ],
        )
        .expect("compile connected fixture");
        let blob = encode_blob(&compiled).expect("encode connected fixture");
        let assets = RuntimeAssets::decode(&blob).expect("decode connected fixture");
        let pane_base = assets
            .resolve(NetworkIdMode::Sequential, ids[1])
            .model_template()
            .unwrap();
        let wood_base = assets
            .resolve(NetworkIdMode::Sequential, ids[3])
            .model_template()
            .unwrap();
        let nether_base = assets
            .resolve(NetworkIdMode::Sequential, ids[4])
            .model_template()
            .unwrap();
        assert_eq!(
            assets.model_templates()[pane_base as usize].flags,
            MODEL_TEMPLATE_FLAG_PANE
        );
        assert_eq!(
            assets.model_templates()[wood_base as usize].flags,
            MODEL_TEMPLATE_FLAG_FENCE_WOOD
        );
        assert_eq!(
            assets.model_templates()[nether_base as usize].flags,
            MODEL_TEMPLATE_FLAG_FENCE_NETHER
        );
        CompiledConnectedFixture {
            assets,
            air: ids[0],
            pane: ids[1],
            other_pane: ids[2],
            wood_fence: ids[3],
            nether_fence: ids[4],
            gate_facing_z: ids[5],
            gate_facing_x: ids[6],
            wall: ids[7],
            cube: ids[8],
        }
    })
}

const CONNECTED_OFFSETS: [([u8; 3], u32); 4] = [
    ([8, 8, 7], 1),
    ([9, 8, 8], 2),
    ([8, 8, 9], 4),
    ([7, 8, 8], 8),
];

fn mesh_connected(center_id: u32, neighbours: &[(usize, u32)]) -> ChunkMesh {
    let fixture = compiled_connected_fixture();
    let mut palette = vec![fixture.air, center_id];
    let mut placements = vec![([8, 8, 8], 1)];
    for &(direction, id) in neighbours {
        let palette_index = palette
            .iter()
            .position(|&value| value == id)
            .unwrap_or_else(|| {
                palette.push(id);
                palette.len() - 1
            });
        placements.push((CONNECTED_OFFSETS[direction].0, palette_index));
    }
    let sub = sub_chunk(vec![packed_storage(4, &palette, &placements)]);
    mesh_sub_chunk(
        &BlockClassifier::new(fixture.air),
        &fixture.assets,
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
    )
}

#[test]
fn panes_select_all_sixteen_palette_native_connection_masks() {
    let fixture = compiled_connected_fixture();
    let base = fixture
        .assets
        .resolve(NetworkIdMode::Sequential, fixture.pane)
        .model_template()
        .unwrap();
    for mask in 0_u32..16 {
        let neighbours = CONNECTED_OFFSETS
            .iter()
            .enumerate()
            .filter(|(_, (_, bit))| mask & bit != 0)
            .map(|(direction, _)| (direction, fixture.pane))
            .collect::<Vec<_>>();
        let mesh = mesh_connected(fixture.pane, &neighbours);
        let center = center_stair_ref(&mesh, [8, 8, 8]);
        assert_eq!(center.words()[1], base + mask, "mask={mask:#06b}");
        assert_eq!(mesh.model_refs().len(), 1 + neighbours.len());
    }
    let all_cubes = (0..4)
        .map(|direction| (direction, fixture.cube))
        .collect::<Vec<_>>();
    assert_eq!(
        center_stair_ref(&mesh_connected(fixture.pane, &all_cubes), [8, 8, 8]).words()[1],
        base + 15
    );
    assert_eq!(
        center_stair_ref(
            &mesh_connected(fixture.pane, &[(0, fixture.wall)]),
            [8, 8, 8]
        )
        .words()[1],
        base + 1,
        "thin panes connect to wall models"
    );
}

#[test]
fn equal_panes_suppress_only_internal_edge_caps_and_different_materials_retain_them() {
    let fixture = compiled_connected_fixture();
    let same = mesh_connected(fixture.pane, &[(0, fixture.pane)]);
    let different = mesh_connected(fixture.pane, &[(0, fixture.other_pane)]);
    let same_ref = center_stair_ref(&same, [8, 8, 8]);
    let different_ref = center_stair_ref(&different, [8, 8, 8]);
    assert_eq!(same_ref.words()[1], different_ref.words()[1]);
    assert_eq!(
        different_ref.words()[3].count_ones(),
        same_ref.words()[3].count_ones() + 1,
        "only the equal-material boundary cap is suppressed"
    );
    assert_eq!(
        different_ref.words()[3].count_ones(),
        fixture.assets.model_templates()[different_ref.words()[1] as usize].quad_count
    );
}

#[test]
fn fences_select_bounded_post_plus_arm_refs_and_preserve_connection_class() {
    let fixture = compiled_connected_fixture();
    for (fence, same, different, flag) in [
        (
            fixture.wood_fence,
            fixture.wood_fence,
            fixture.nether_fence,
            MODEL_TEMPLATE_FLAG_FENCE_WOOD,
        ),
        (
            fixture.nether_fence,
            fixture.nether_fence,
            fixture.wood_fence,
            MODEL_TEMPLATE_FLAG_FENCE_NETHER,
        ),
    ] {
        let base = fixture
            .assets
            .resolve(NetworkIdMode::Sequential, fence)
            .model_template()
            .unwrap();
        assert_eq!(fixture.assets.model_templates()[base as usize].flags, flag);
        for mask in 0_u32..16 {
            let neighbours = CONNECTED_OFFSETS
                .iter()
                .enumerate()
                .filter(|(_, (_, bit))| mask & bit != 0)
                .map(|(direction, _)| (direction, same))
                .collect::<Vec<_>>();
            let mesh = mesh_connected(fence, &neighbours);
            let center_refs = mesh
                .model_refs()
                .iter()
                .filter(|reference| reference.words()[0] & 0xfff == 8 | (8 << 4) | (8 << 8))
                .collect::<Vec<_>>();
            assert_eq!(center_refs.len(), if mask == 0 { 1 } else { 2 });
            assert_eq!(center_refs[0].words()[1], base);
            if mask != 0 {
                assert_eq!(center_refs[1].words()[1], base + 1 + mask);
            }
        }
        let different_mesh = mesh_connected(fence, &[(0, different)]);
        assert_eq!(
            different_mesh
                .model_refs()
                .iter()
                .filter(|reference| reference.words()[0] & 0xfff == 8 | (8 << 4) | (8 << 8))
                .count(),
            1,
            "wood and nether fences do not connect"
        );
        let cube_mesh = mesh_connected(fence, &[(0, fixture.cube)]);
        assert_eq!(
            cube_mesh
                .model_refs()
                .iter()
                .filter(|reference| reference.words()[0] & 0xfff == 8 | (8 << 4) | (8 << 8))
                .count(),
            2,
            "full support face connects"
        );
    }
}

#[test]
fn fences_connect_only_to_the_sides_of_axis_aligned_fence_gates() {
    let fixture = compiled_connected_fixture();
    for (gate, connecting_directions) in [
        (fixture.gate_facing_z, [1_usize, 3]),
        (fixture.gate_facing_x, [0_usize, 2]),
    ] {
        for direction in 0..4 {
            let mesh = mesh_connected(fixture.wood_fence, &[(direction, gate)]);
            let center_ref_count = mesh
                .model_refs()
                .iter()
                .filter(|reference| reference.words()[0] & 0xfff == 8 | (8 << 4) | (8 << 8))
                .count();
            assert_eq!(
                center_ref_count,
                if connecting_directions.contains(&direction) {
                    2
                } else {
                    1
                },
                "gate={gate} direction={direction}"
            );
        }
    }
}

#[test]
fn connected_models_cross_all_four_horizontal_subchunk_boundaries() {
    let fixture = compiled_connected_fixture();
    let pane_base = fixture
        .assets
        .resolve(NetworkIdMode::Sequential, fixture.pane)
        .model_template()
        .unwrap();
    let fence_base = fixture
        .assets
        .resolve(NetworkIdMode::Sequential, fixture.wood_fence)
        .model_template()
        .unwrap();
    for (center, remote, face, bit) in [
        ([8, 8, 0], [8, 8, 15], Face::NegativeZ, 1_u32),
        ([15, 8, 8], [0, 8, 8], Face::PositiveX, 2),
        ([8, 8, 15], [8, 8, 0], Face::PositiveZ, 4),
        ([0, 8, 8], [15, 8, 8], Face::NegativeX, 8),
    ] {
        for (block, base, expected_refs) in [
            (fixture.pane, pane_base, 1_usize),
            (fixture.wood_fence, fence_base, 2),
        ] {
            let center_sub = sub_chunk(vec![packed_storage(
                1,
                &[fixture.air, block],
                &[(center, 1)],
            )]);
            let remote_sub = sub_chunk(vec![packed_storage(
                1,
                &[fixture.air, block],
                &[(remote, 1)],
            )]);
            let mesh = mesh_sub_chunk(
                &BlockClassifier::new(fixture.air),
                &fixture.assets,
                NetworkIdMode::Sequential,
                &neighbourhood_for(face, &remote_sub),
                &center_sub,
            );
            let center_word =
                u32::from(center[0]) | (u32::from(center[1]) << 4) | (u32::from(center[2]) << 8);
            let refs = mesh
                .model_refs()
                .iter()
                .filter(|reference| reference.words()[0] & 0xfff == center_word)
                .collect::<Vec<_>>();
            assert_eq!(refs.len(), expected_refs, "face={face:?} block={block}");
            if block == fixture.pane {
                assert_eq!(refs[0].words()[1], base + bit, "face={face:?}");
            } else {
                assert_eq!(refs[0].words()[1], base, "face={face:?}");
                assert_eq!(refs[1].words()[1], base + 1 + bit, "face={face:?}");
            }
            let missing = mesh_sub_chunk(
                &BlockClassifier::new(fixture.air),
                &fixture.assets,
                NetworkIdMode::Sequential,
                &Neighbourhood::empty(),
                &center_sub,
            );
            let missing_refs = missing
                .model_refs()
                .iter()
                .filter(|reference| reference.words()[0] & 0xfff == center_word)
                .count();
            assert_eq!(missing_refs, 1, "missing face={face:?} block={block}");
        }
    }
}

#[test]
fn fence_gate_axis_connections_cross_all_four_horizontal_subchunk_boundaries() {
    let fixture = compiled_connected_fixture();
    let fence_base = fixture
        .assets
        .resolve(NetworkIdMode::Sequential, fixture.wood_fence)
        .model_template()
        .unwrap();
    for (center, remote, face, bit, gate) in [
        (
            [8, 8, 0],
            [8, 8, 15],
            Face::NegativeZ,
            1_u32,
            fixture.gate_facing_x,
        ),
        (
            [15, 8, 8],
            [0, 8, 8],
            Face::PositiveX,
            2,
            fixture.gate_facing_z,
        ),
        (
            [8, 8, 15],
            [8, 8, 0],
            Face::PositiveZ,
            4,
            fixture.gate_facing_x,
        ),
        (
            [0, 8, 8],
            [15, 8, 8],
            Face::NegativeX,
            8,
            fixture.gate_facing_z,
        ),
    ] {
        let center_sub = sub_chunk(vec![packed_storage(
            1,
            &[fixture.air, fixture.wood_fence],
            &[(center, 1)],
        )]);
        let remote_sub = sub_chunk(vec![packed_storage(
            1,
            &[fixture.air, gate],
            &[(remote, 1)],
        )]);
        let mesh = mesh_sub_chunk(
            &BlockClassifier::new(fixture.air),
            &fixture.assets,
            NetworkIdMode::Sequential,
            &neighbourhood_for(face, &remote_sub),
            &center_sub,
        );
        let center_word =
            u32::from(center[0]) | (u32::from(center[1]) << 4) | (u32::from(center[2]) << 8);
        let refs = mesh
            .model_refs()
            .iter()
            .filter(|reference| reference.words()[0] & 0xfff == center_word)
            .collect::<Vec<_>>();
        assert_eq!(refs.len(), 2, "face={face:?}");
        assert_eq!(refs[0].words()[1], fence_base, "face={face:?}");
        assert_eq!(refs[1].words()[1], fence_base + 1 + bit, "face={face:?}");
    }
}
