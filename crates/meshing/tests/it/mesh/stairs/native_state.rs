struct NativeStairFixture {
    assets: RuntimeAssets,
    air: RegistryRecord,
    stairs: Vec<RegistryRecord>,
}

fn native_stair_fixture() -> &'static NativeStairFixture {
    static FIXTURE: OnceLock<NativeStairFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let data = include_bytes!("../../../../../assets/data/block-registry-v2193.bin");
        let records = assets::read_registry_for_protocol(
            data,
            assets::registry_header_protocol(data).unwrap(),
        )
        .unwrap();
        let air = records
            .iter()
            .find(|record| record.name.as_ref() == "minecraft:air")
            .unwrap()
            .clone();
        let stairs = records
            .into_iter()
            .filter(|record| record.name.as_ref() == "minecraft:birch_stairs")
            .collect::<Vec<_>>();
        let directory = tempfile::tempdir().unwrap();
        write_slab_render_pack(
            directory.path(),
            "birch_stairs",
            "unused_double",
            "unused_cube",
        );
        let compiled = compile_pack(
            directory.path(),
            &std::iter::once(air.clone())
                .chain(stairs.iter().cloned())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let assets = RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap();
        NativeStairFixture {
            assets,
            air,
            stairs,
        }
    })
}

fn native_stair_id(record: &RegistryRecord, mode: NetworkIdMode) -> u32 {
    match mode {
        NetworkIdMode::Sequential => record.sequential_id,
        NetworkIdMode::Hashed => record.network_hash,
    }
}

fn mesh_native_stair(
    record: &RegistryRecord,
    mode: NetworkIdMode,
    center: [u8; 3],
    neighbor: Option<([u8; 3], &RegistryRecord)>,
    neighbourhood: &Neighbourhood<'_>,
) -> ChunkMesh {
    let fixture = native_stair_fixture();
    let mut palette = vec![
        native_stair_id(&fixture.air, mode),
        native_stair_id(record, mode),
    ];
    let mut placements = vec![(center, 1)];
    if let Some((position, record)) = neighbor {
        palette.push(native_stair_id(record, mode));
        placements.push((position, 2));
    }
    let sub = sub_chunk(vec![packed_storage(2, &palette, &placements)]);
    let mesh = mesh_sub_chunk(
        &BlockClassifier::new(native_stair_id(&fixture.air, mode)),
        &fixture.assets,
        mode,
        neighbourhood,
        &sub,
    );
    assert!(
        meshing::mesh_output_byte_len(&mesh, &meshing::PackedBiomeRecord::fallback())
            <= meshing::MeshOutputBounds::new(&fixture.assets).for_sub_chunk(
                &sub,
                &fixture.assets,
                mode
            )
    );
    mesh
}

#[test]
fn native_stair_corners_do_not_change_with_neighbors_and_bake_the_selected_template() {
    let fixture = native_stair_fixture();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        for record in &fixture.stairs {
            let visual = fixture.assets.resolve(mode, native_stair_id(record, mode));
            let template_id = visual.model_template().unwrap();
            let template = fixture.assets.model_templates()[template_id as usize];
            assert_eq!(template.flags, 0);
            for neighbor in [
                None,
                Some(([8, 8, 7], &fixture.stairs[0])),
                Some(([9, 8, 8], &fixture.stairs[1])),
            ] {
                let mesh =
                    mesh_native_stair(record, mode, [8, 8, 8], neighbor, &Neighbourhood::empty());
                let reference = center_stair_ref(&mesh, [8, 8, 8]);
                assert_eq!(
                    reference.words()[1],
                    template_id,
                    "{} {mode:?}",
                    record.canonical_state
                );
                assert_eq!((reference.words()[0] >> 12) & 3, visual.variant() & 3);
                let lighting_start = reference.words()[2] as usize;
                let lighting_end = mesh
                    .model_refs()
                    .iter()
                    .map(|reference| reference.words()[2] as usize)
                    .filter(|&start| start > lighting_start)
                    .min()
                    .unwrap_or(mesh.model_lighting().len());
                assert_eq!(lighting_end - lighting_start, template.quad_count as usize);
                assert!(mesh.cube_quads().is_empty());
            }
        }
    }
}

#[test]
fn native_stair_corners_remain_authoritative_at_every_horizontal_chunk_boundary() {
    let fixture = native_stair_fixture();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let remote = sub_chunk(vec![uniform_storage(native_stair_id(
            &fixture.stairs[1],
            mode,
        ))]);
        for (center, boundary) in [
            ([0, 8, 8], Face::NegativeX),
            ([15, 8, 8], Face::PositiveX),
            ([8, 8, 0], Face::NegativeZ),
            ([8, 8, 15], Face::PositiveZ),
        ] {
            let neighbours = neighbourhood_for(boundary, &remote);
            for record in &fixture.stairs {
                let mesh = mesh_native_stair(record, mode, center, None, &neighbours);
                let expected = fixture
                    .assets
                    .resolve(mode, native_stair_id(record, mode))
                    .model_template()
                    .unwrap();
                assert_eq!(
                    center_stair_ref(&mesh, center).words()[1],
                    expected,
                    "{} {mode:?} {boundary:?}",
                    record.canonical_state
                );
            }
        }
    }
}

#[test]
fn native_stair_corner_block_update_changes_geometry_even_without_neighbor_changes() {
    let fixture = native_stair_fixture();
    let record = |corner| {
        fixture
            .stairs
            .iter()
            .find(|record| {
                record.model_state.get(ModelStateField::Orientation) == Some(0)
                    && record.model_state.get(ModelStateField::Half) == Some(0)
                    && record
                        .canonical_state
                        .contains(&format!("\"value\":\"{corner}\""))
            })
            .unwrap()
    };
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let straight = mesh_native_stair(
            record("none"),
            mode,
            [8, 8, 8],
            None,
            &Neighbourhood::empty(),
        );
        let corner = mesh_native_stair(
            record("inner_left"),
            mode,
            [8, 8, 8],
            None,
            &Neighbourhood::empty(),
        );
        assert_ne!(
            center_stair_ref(&straight, [8, 8, 8]).words()[1],
            center_stair_ref(&corner, [8, 8, 8]).words()[1]
        );
    }
}
