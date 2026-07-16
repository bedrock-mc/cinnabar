struct CompiledStairFixture {
    assets: RuntimeAssets,
    air: u32,
    ids: [[u32; 4]; 2],
    cube: u32,
}

fn compiled_stair_fixture() -> &'static CompiledStairFixture {
    static FIXTURE: OnceLock<CompiledStairFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let records = read_registry(include_bytes!("../../../assets/data/block-registry-v1001.bin"))
            .expect("decode stair registry");
        let air = records
            .iter()
            .find(|record| record.name.as_ref() == "minecraft:air")
            .unwrap()
            .clone();
        let stairs = records
            .iter()
            .filter(|record| record.name.as_ref() == "minecraft:oak_stairs")
            .cloned()
            .collect::<Vec<_>>();
        let cube = records
            .iter()
            .find(|record| {
                record.model_family == ModelFamily::Cube
                    && record.flags.contains(BlockFlags::OCCLUDES_FULL_FACE)
            })
            .unwrap()
            .clone();
        assert_eq!(stairs.len(), 8);
        let mut ids = [[0; 4]; 2];
        for record in &stairs {
            ids[record.model_state.get(ModelStateField::Half).unwrap() as usize][record
                .model_state
                .get(ModelStateField::Orientation)
                .unwrap()
                as usize] = record.sequential_id;
        }
        let directory = tempfile::tempdir().expect("create stair render fixture");
        write_slab_render_pack(
            directory.path(),
            "oak_stairs",
            "unused_double",
            "unused_cube",
        );
        let compiled = compile_pack(
            directory.path(),
            &std::iter::once(air.clone())
                .chain(stairs)
                .chain(std::iter::once(cube.clone()))
                .collect::<Vec<_>>(),
        )
        .expect("compile stair render fixture");
        let blob = encode_blob(&compiled).expect("encode stair render fixture");
        CompiledStairFixture {
            assets: RuntimeAssets::decode(&blob).expect("decode stair render fixture"),
            air: air.sequential_id,
            ids,
            cube: cube.sequential_id,
        }
    })
}

fn mesh_stair_placements(
    placements: &[([u8; 3], u32)],
    neighbours: &Neighbourhood<'_>,
) -> ChunkMesh {
    let fixture = compiled_stair_fixture();
    let mut palette = vec![fixture.air];
    let indexed = placements
        .iter()
        .map(|&(coordinate, id)| {
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
    let storage = packed_storage(4, &palette, &indexed);
    let sub = sub_chunk(vec![storage]);
    mesh_sub_chunk(
        &BlockClassifier::new(fixture.air),
        &fixture.assets,
        NetworkIdMode::Sequential,
        neighbours,
        &sub,
    )
}

#[test]
fn compiled_stairs_select_all_dragonfly_neighbor_shapes_before_lighting() {
    let fixture = compiled_stair_fixture();
    let north = fixture.ids[0][2];
    let east = fixture.ids[0][3];
    let west = fixture.ids[0][1];
    let center = [8, 8, 8];
    let base = fixture
        .assets
        .resolve(NetworkIdMode::Sequential, north)
        .model_template()
        .unwrap();
    assert_eq!(
        fixture.assets.model_templates()[base as usize].flags,
        MODEL_TEMPLATE_FLAG_STAIR
    );
    for (name, neighbour, expected_shape) in [
        ("straight", None, 0),
        ("right inner", Some(([8, 8, 9], east)), 1),
        ("left inner", Some(([8, 8, 9], west)), 2),
        ("right outer", Some(([8, 8, 7], west)), 3),
        ("left outer", Some(([8, 8, 7], east)), 4),
    ] {
        let mut placements = vec![(center, north)];
        if let Some(neighbour) = neighbour {
            placements.push(neighbour);
        }
        let mesh = mesh_stair_placements(&placements, &Neighbourhood::empty());
        let reference = mesh
            .model_refs()
            .iter()
            .find(|reference| reference.words()[0] & 0xfff == 8 | (8 << 4) | (8 << 8))
            .unwrap();
        assert_eq!(reference.words()[1], base + expected_shape, "{name}");
        let quad_count =
            fixture.assets.model_templates()[(base + expected_shape) as usize].quad_count as usize;
        let lighting_start = reference.words()[2] as usize;
        let lighting_end = mesh
            .model_refs()
            .iter()
            .map(|reference| reference.words()[2] as usize)
            .filter(|&start| start > lighting_start)
            .min()
            .unwrap_or(mesh.model_lighting().len());
        assert_eq!(
            lighting_end - lighting_start,
            quad_count,
            "{name} selected lighting span"
        );
        assert!(
            mesh.cube_quads().is_empty(),
            "{name} never enters cube stream"
        );
    }
}

const fn stair_offset(facing: usize) -> [i8; 3] {
    match facing {
        0 => [0, 0, 1],
        1 => [-1, 0, 0],
        2 => [0, 0, -1],
        _ => [1, 0, 0],
    }
}

fn offset_coordinate([x, y, z]: [u8; 3], [dx, dy, dz]: [i8; 3]) -> [u8; 3] {
    [
        (x as i16 + dx as i16) as u8,
        (y as i16 + dy as i16) as u8,
        (z as i16 + dz as i16) as u8,
    ]
}

fn center_stair_ref(mesh: &ChunkMesh, center: [u8; 3]) -> PackedModelRef {
    *mesh
        .model_refs()
        .iter()
        .find(|reference| {
            reference.words()[0] & 0xfff
                == u32::from(center[0]) | (u32::from(center[1]) << 4) | (u32::from(center[2]) << 8)
        })
        .unwrap()
}

#[test]
fn stair_all_orientations_upside_states_shapes_and_dragonfly_suppression() {
    let fixture = compiled_stair_fixture();
    let center = [8, 8, 8];
    for half in 0..2 {
        for facing in 0..4 {
            let current = fixture.ids[half][facing];
            let right = (facing + 1) & 3;
            let left = (facing + 3) & 3;
            let front = offset_coordinate(center, stair_offset(facing));
            let back = offset_coordinate(center, stair_offset((facing + 2) & 3));
            let right_side = offset_coordinate(center, stair_offset(right));
            let base = fixture
                .assets
                .resolve(NetworkIdMode::Sequential, current)
                .model_template()
                .unwrap();
            for (shape, neighbour) in [
                (0, None),
                (1, Some((back, fixture.ids[half][right]))),
                (2, Some((back, fixture.ids[half][left]))),
                (3, Some((front, fixture.ids[half][left]))),
                (4, Some((front, fixture.ids[half][right]))),
            ] {
                let mut placements = vec![(center, current)];
                if let Some(neighbour) = neighbour {
                    placements.push(neighbour);
                }
                let mesh = mesh_stair_placements(&placements, &Neighbourhood::empty());
                let reference = center_stair_ref(&mesh, center);
                assert_eq!(
                    reference.words()[1],
                    base + shape,
                    "half={half} facing={facing} shape={shape}"
                );
                let count =
                    fixture.assets.model_templates()[(base + shape) as usize].quad_count as usize;
                let start = reference.words()[2] as usize;
                let end = mesh
                    .model_refs()
                    .iter()
                    .map(|reference| reference.words()[2] as usize)
                    .filter(|&next| next > start)
                    .min()
                    .unwrap_or(mesh.model_lighting().len());
                assert_eq!(
                    end - start,
                    count,
                    "half={half} facing={facing} shape={shape}"
                );
            }
            for placements in [
                vec![
                    (center, current),
                    (back, fixture.ids[half][right]),
                    (right_side, current),
                ],
                vec![
                    (center, current),
                    (front, fixture.ids[half][left]),
                    (right_side, current),
                ],
                vec![(center, current), (front, fixture.ids[1 - half][right])],
            ] {
                let mesh = mesh_stair_placements(&placements, &Neighbourhood::empty());
                assert_eq!(
                    center_stair_ref(&mesh, center).words()[1],
                    base,
                    "suppression/half mismatch half={half} facing={facing}"
                );
            }
        }
    }
}

#[test]
fn stair_topology_crosses_all_horizontal_subchunk_boundaries_for_both_halves() {
    let fixture = compiled_stair_fixture();
    for half in 0..2 {
        for (facing, center, remote, boundary) in [
            (0, [8, 8, 15], [8, 8, 0], Face::PositiveZ),
            (1, [0, 8, 8], [15, 8, 8], Face::NegativeX),
            (2, [8, 8, 0], [8, 8, 15], Face::NegativeZ),
            (3, [15, 8, 8], [0, 8, 8], Face::PositiveX),
        ] {
            let current = fixture.ids[half][facing];
            let right = fixture.ids[half][(facing + 1) & 3];
            let base = fixture
                .assets
                .resolve(NetworkIdMode::Sequential, current)
                .model_template()
                .unwrap();
            let neighbour = sub_chunk(vec![packed_storage(
                1,
                &[fixture.air, right],
                &[(remote, 1)],
            )]);
            let mesh = mesh_stair_placements(
                &[(center, current)],
                &neighbourhood_for(boundary, &neighbour),
            );
            assert_eq!(
                center_stair_ref(&mesh, center).words()[1],
                base + 4,
                "half={half} facing={facing}"
            );
        }
    }
}

#[test]
fn stair_rotated_boundary_cull_faces_preserve_lighting_addresses() {
    let fixture = compiled_stair_fixture();
    let center = [8, 8, 8];
    for half in 0..2 {
        for facing in 0..4 {
            let current = fixture.ids[half][facing];
            let neighbour = offset_coordinate(center, stair_offset(facing));
            let open = mesh_stair_placements(&[(center, current)], &Neighbourhood::empty());
            let culled = mesh_stair_placements(
                &[(center, current), (neighbour, fixture.cube)],
                &Neighbourhood::empty(),
            );
            let open_ref = center_stair_ref(&open, center);
            let culled_ref = center_stair_ref(&culled, center);
            assert_eq!(culled_ref.words()[1], open_ref.words()[1]);
            assert_eq!(culled_ref.words()[2], 0);
            assert_ne!(
                culled_ref.words()[3],
                open_ref.words()[3],
                "half={half} facing={facing} transformed cull face"
            );
            assert_eq!(
                open_ref.words()[3].count_ones() - culled_ref.words()[3].count_ones(),
                4,
                "half={half} facing={facing} full stair side must map all four canonical half-cell faces"
            );
            let count = fixture.assets.model_templates()[culled_ref.words()[1] as usize].quad_count
                as usize;
            assert_eq!(
                culled.model_lighting().len(),
                count,
                "half={half} facing={facing} lighting address span"
            );
        }
    }
}

#[test]
fn stair_topology_crosses_horizontal_subchunk_boundaries_and_missing_is_straight() {
    let fixture = compiled_stair_fixture();
    let north = fixture.ids[1][2];
    let east = fixture.ids[1][3];
    let base = fixture
        .assets
        .resolve(NetworkIdMode::Sequential, north)
        .model_template()
        .unwrap();
    let remote = sub_chunk(vec![packed_storage(
        1,
        &[fixture.air, east],
        &[([8, 8, 15], 1)],
    )]);
    let center = [8, 8, 0];
    let crossed = mesh_stair_placements(
        &[(center, north)],
        &Neighbourhood::empty().with_negative_z(&remote),
    );
    assert_eq!(
        crossed.model_refs()[0].words()[1],
        base + 4,
        "upside-down left outer across -Z"
    );
    let missing = mesh_stair_placements(&[(center, north)], &Neighbourhood::empty());
    assert_eq!(
        missing.model_refs()[0].words()[1],
        base,
        "missing neighbour remains conservative straight"
    );
    assert!(
        missing.connectivity().is_all_connected(),
        "stairs remain conservative partial connectivity"
    );
}
