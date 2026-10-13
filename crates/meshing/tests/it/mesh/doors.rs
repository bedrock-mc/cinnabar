struct DoorRenderFixture {
    assets: RuntimeAssets,
    air: u32,
    air_hash: u32,
    doors: Vec<RegistryRecord>,
}

/// Compiles every registered door family with original, distinct half artwork.
fn door_render_fixture() -> &'static DoorRenderFixture {
    static FIXTURE: OnceLock<DoorRenderFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let target: serde_json::Value =
            serde_json::from_slice(include_bytes!("../../../../../assets/bedrock-target.json"))
                .unwrap();
        let registry = assets::read_registry_for_protocol(
            include_bytes!("../../../../assets/data/block-registry-v2193.bin"),
            target["wire_protocol"].as_u64().unwrap() as u32,
        )
        .expect("decode door registry");
        let air = registry
            .iter()
            .find(|r| r.flags.contains(BlockFlags::AIR))
            .unwrap()
            .clone();
        let doors = registry
            .into_iter()
            .filter(|r| r.model_family == ModelFamily::Door)
            .collect::<Vec<_>>();
        let root = tempfile::tempdir().unwrap();
        let blocks = doors
            .iter()
            .map(|r| {
                (
                    r.name.strip_prefix("minecraft:").unwrap().to_owned(),
                    serde_json::json!({"textures":{"down":"lower","up":"lower","side":"upper"}}),
                )
            })
            .collect::<serde_json::Map<_, _>>();
        fs::create_dir_all(root.path().join("textures/blocks")).unwrap();
        fs::write(root.path().join("textures/flipbook_textures.json"), "[]").unwrap();
        fs::write(
            root.path().join("blocks.json"),
            serde_json::to_vec(&blocks).unwrap(),
        )
        .unwrap();
        fs::write(
            root.path().join("textures/terrain_texture.json"),
            serde_json::to_vec(&serde_json::json!({"texture_data": {
                "lower":{"textures":"textures/blocks/lower"},
                "upper":{"textures":"textures/blocks/upper"}
            }}))
            .unwrap(),
        )
        .unwrap();
        for (name, color) in [("lower", [200, 10, 20, 127]), ("upper", [10, 200, 20, 127])] {
            let bytes = [color; 256].concat();
            let file =
                fs::File::create(root.path().join(format!("textures/blocks/{name}.png"))).unwrap();
            PngEncoder::new(file)
                .write_image(&bytes, 16, 16, ExtendedColorType::Rgba8)
                .unwrap();
        }
        let records = std::iter::once(air.clone())
            .chain(doors.iter().cloned())
            .collect::<Vec<_>>();
        let compiled = compile_pack(root.path(), &records).expect("compile door fixture");
        DoorRenderFixture {
            assets: RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap(),
            air: air.sequential_id,
            air_hash: air.network_hash,
            doors,
        }
    })
}

/// Finds a registered half by the independent fields supplied by a server.
fn door_render_id(name: &str, facing: u32, open: u32, hinge: u32, upper: bool) -> u32 {
    door_render_fixture()
        .doors
        .iter()
        .find(|r| {
            r.name.as_ref() == name
                && r.model_state.get(ModelStateField::Orientation) == Some(facing)
                && r.model_state.get(ModelStateField::Open) == Some(open)
                && r.model_state.get(ModelStateField::Hinge) == Some(hinge)
                && r.model_state.get(ModelStateField::Flags) == Some(u32::from(upper) << 7)
        })
        .unwrap()
        .sequential_id
}

/// Meshes an isolated door pair through the ordinary palette and lighting path.
fn mesh_door_cells(cells: &[([u8; 3], u32)], neighbours: &Neighbourhood<'_>) -> ChunkMesh {
    mesh_door_cells_mode(cells, neighbours, NetworkIdMode::Sequential)
}

/// Resolves the same authored states through either wire identity mode.
fn mesh_door_cells_mode(
    cells: &[([u8; 3], u32)],
    neighbours: &Neighbourhood<'_>,
    mode: NetworkIdMode,
) -> ChunkMesh {
    let fixture = door_render_fixture();
    let id = |id| {
        if mode == NetworkIdMode::Sequential {
            id
        } else {
            if id == fixture.air {
                fixture.air_hash
            } else {
                fixture
                    .doors
                    .iter()
                    .find(|r| r.sequential_id == id)
                    .unwrap()
                    .network_hash
            }
        }
    };
    let palette = std::iter::once(id(fixture.air))
        .chain(cells.iter().map(|(_, value)| id(*value)))
        .collect::<Vec<_>>();
    let placements = cells
        .iter()
        .enumerate()
        .map(|(i, (p, _))| (*p, i + 1))
        .collect::<Vec<_>>();
    mesh_sub_chunk(
        &BlockClassifier::new(id(fixture.air)),
        &fixture.assets,
        mode,
        neighbours,
        &sub_chunk(vec![packed_storage(4, &palette, &placements)]),
    )
}

/// Encodes an adjacent chunk without resolving away its raw stored door fields.
fn door_neighbor_chunk(cell: [u8; 3], id: u32) -> SubChunk {
    let fixture = door_render_fixture();
    sub_chunk(vec![packed_storage(1, &[fixture.air, id], &[(cell, 1)])])
}

#[test]
fn door_pairs_require_the_same_class_even_when_textures_match() {
    let fixture = door_render_fixture();
    let names = fixture
        .doors
        .iter()
        .map(|r| r.name.as_ref())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .take(2)
        .collect::<Vec<_>>();
    let lower = door_render_id(names[0], 0, 1, 0, false);
    let upper = door_render_id(names[1], 0, 1, 0, true);
    assert!(
        mesh_door_cells(
            &[([8, 8, 8], lower), ([8, 9, 8], upper)],
            &Neighbourhood::empty()
        )
        .model_refs()
        .is_empty()
    );
    for id in [lower, upper] {
        assert!(
            mesh_door_cells(&[([8, 8, 8], id)], &Neighbourhood::empty())
                .model_refs()
                .is_empty()
        );
    }
}

#[test]
fn door_half_tracks_loaded_vertical_neighbor_and_changed_hinge() {
    let fixture = door_render_fixture();
    let name = fixture.doors[0].name.as_ref();
    for x in [0, 15] {
        for z in [0, 15] {
            let lower = door_render_id(name, 0, 1, 1, false);
            let cell = [x, 15, z];
            assert!(
                mesh_door_cells(&[(cell, lower)], &Neighbourhood::empty())
                    .model_refs()
                    .is_empty()
            );
            let mut previous = None;
            for hinge in 0..2 {
                let upper = door_render_id(name, 2, 0, hinge, true);
                let above = door_neighbor_chunk([x, 0, z], upper);
                let mesh = mesh_door_cells(
                    &[(cell, lower)],
                    &Neighbourhood::empty().with_positive_y(&above),
                );
                let upper_mesh = mesh_door_cells(
                    &[([x, 0, z], upper)],
                    &Neighbourhood::empty().with_negative_y(&door_neighbor_chunk(cell, lower)),
                );
                let bounds = door_mesh_bounds(&mesh, cell);
                assert_eq!(bounds, door_mesh_bounds(&upper_mesh, [x, 0, z]));
                if let Some(previous) = previous {
                    assert_ne!(previous, bounds);
                }
                previous = Some(bounds);
            }
            assert!(
                mesh_door_cells(&[(cell, lower)], &Neighbourhood::empty())
                    .model_refs()
                    .is_empty()
            );
        }
    }
}

/// Returns the selected template's block-local envelope for one cell.
fn door_mesh_bounds(mesh: &ChunkMesh, cell: [u8; 3]) -> ([i16; 3], [i16; 3]) {
    let key = u32::from(cell[0]) | (u32::from(cell[1]) << 4) | (u32::from(cell[2]) << 8);
    let reference = mesh
        .model_refs()
        .iter()
        .find(|r| r.words()[0] & 0xfff == key)
        .expect("door half rendered");
    let fixture = door_render_fixture();
    let template = fixture.assets.model_templates()[reference.words()[1] as usize];
    let quads = &fixture.assets.model_quads()
        [template.quad_start as usize..(template.quad_start + template.quad_count) as usize];
    let mut min = [i16::MAX; 3];
    let mut max = [i16::MIN; 3];
    for position in quads.iter().flat_map(|q| q.positions) {
        for axis in 0..3 {
            min[axis] = min[axis].min(position[axis]);
            max[axis] = max[axis].max(position[axis]);
        }
    }
    (min, max)
}

#[test]
fn door_halves_share_lower_open_direction_and_upper_hinge() {
    let fixture = door_render_fixture();
    let names = fixture
        .doors
        .iter()
        .map(|r| r.name.as_ref())
        .collect::<std::collections::BTreeSet<_>>();
    for name in names {
        for direction in 0..4 {
            for open in 0..2 {
                for hinge in 0..2 {
                    let lower = door_render_id(name, direction, open, hinge ^ 1, false);
                    let upper = door_render_id(name, (direction + 1) & 3, open ^ 1, hinge, true);
                    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
                        let mesh = mesh_door_cells_mode(
                            &[([8, 8, 8], lower), ([8, 9, 8], upper)],
                            &Neighbourhood::empty(),
                            mode,
                        );
                        assert_eq!(
                            door_mesh_bounds(&mesh, [8, 8, 8]),
                            door_mesh_bounds(&mesh, [8, 9, 8]),
                            "{name}/{direction}/{open}/{hinge}"
                        );
                        let effective_lower = door_render_id(name, direction, open, hinge, false);
                        let effective_upper = door_render_id(name, direction, open, hinge, true);
                        let expected = mesh_door_cells(
                            &[([8, 8, 8], effective_lower), ([8, 9, 8], effective_upper)],
                            &Neighbourhood::empty(),
                        );
                        assert_eq!(
                            door_mesh_bounds(&mesh, [8, 8, 8]),
                            door_mesh_bounds(&expected, [8, 8, 8])
                        );
                    }
                }
            }
        }
    }
}
