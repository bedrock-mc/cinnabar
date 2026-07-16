struct CompiledChiseledBookshelfFixture {
    assets: RuntimeAssets,
    air: NetworkValues,
    shelves: Vec<NetworkValues>,
    cube: NetworkValues,
}

impl CompiledChiseledBookshelfFixture {
    fn shelf(&self, books: u32, direction: u32) -> NetworkValues {
        self.shelves[(books * 4 + direction) as usize]
    }
}

fn write_chiseled_bookshelf_render_pack(root: &Path, cube_name: &str) {
    fs::create_dir_all(root.join("textures/blocks")).expect("create bookshelf fixture tree");
    fs::write(
        root.join("blocks.json"),
        format!(
            r#"{{
                "chiseled_bookshelf":{{"textures":{{"west":"chiseled_bookshelf_side","east":"chiseled_bookshelf_side","down":"chiseled_bookshelf_top","up":"chiseled_bookshelf_top","north":"chiseled_bookshelf_front","south":"chiseled_bookshelf_side"}}}},
                "{cube_name}":{{"textures":"cube"}}
            }}"#
        ),
    )
    .expect("write bookshelf block routing");
    fs::write(
        root.join("textures/terrain_texture.json"),
        r#"{"texture_data":{
            "chiseled_bookshelf_front":{"textures":["textures/blocks/chiseled_bookshelf_empty","textures/blocks/chiseled_bookshelf_occupied"]},
            "chiseled_bookshelf_side":{"textures":"textures/blocks/chiseled_bookshelf_side"},
            "chiseled_bookshelf_top":{"textures":"textures/blocks/chiseled_bookshelf_top"},
            "cube":{"textures":"textures/blocks/cube"}
        }}"#,
    )
    .expect("write bookshelf terrain routing");
    fs::write(root.join("textures/flipbook_textures.json"), "[]")
        .expect("write bookshelf empty flipbooks");
    for (index, name) in [
        "chiseled_bookshelf_empty",
        "chiseled_bookshelf_occupied",
        "chiseled_bookshelf_side",
        "chiseled_bookshelf_top",
        "cube",
    ]
    .into_iter()
    .enumerate()
    {
        let rgba = [20 + index as u8 * 35, 70, 110, 255].repeat(16 * 16);
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&rgba, 16, 16, ExtendedColorType::Rgba8)
            .expect("encode bookshelf fixture PNG");
        fs::write(root.join(format!("textures/blocks/{name}.png")), png)
            .expect("write bookshelf fixture PNG");
    }
}

fn compiled_chiseled_bookshelf_fixture() -> &'static CompiledChiseledBookshelfFixture {
    static FIXTURE: OnceLock<CompiledChiseledBookshelfFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let records = read_registry(include_bytes!("../../../assets/data/block-registry-v1001.bin"))
            .expect("decode bookshelf registry");
        let air = records
            .iter()
            .find(|record| record.name.as_ref() == "minecraft:air")
            .expect("air record")
            .clone();
        let shelves = records
            .iter()
            .filter(|record| record.name.as_ref() == "minecraft:chiseled_bookshelf")
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(shelves.len(), 256);
        let cube = records
            .iter()
            .find(|record| {
                record.name.as_ref() == "minecraft:stone"
                    && record.model_family == ModelFamily::Cube
                    && record.flags.contains(BlockFlags::OCCLUDES_FULL_FACE)
            })
            .expect("stone full cube")
            .clone();
        let values = |record: &assets::RegistryRecord| NetworkValues {
            sequential: record.sequential_id,
            hashed: record.network_hash,
        };
        let shelf_values = shelves.iter().map(values).collect::<Vec<_>>();
        let air_values = values(&air);
        let cube_values = values(&cube);
        let cube_name = cube.name.strip_prefix("minecraft:").unwrap();
        let directory = tempfile::tempdir().expect("bookshelf fixture directory");
        write_chiseled_bookshelf_render_pack(directory.path(), cube_name);
        let mut selected = Vec::with_capacity(258);
        selected.push(air);
        selected.extend(shelves);
        selected.push(cube);
        let compiled =
            compile_pack(directory.path(), &selected).expect("compile bookshelf fixture");
        let blob = encode_blob(&compiled).expect("encode bookshelf fixture");
        let assets = RuntimeAssets::decode(&blob).expect("decode bookshelf fixture");
        let cube_visual = assets.resolve(NetworkIdMode::Sequential, cube_values.sequential);
        assert!(cube_visual.is_known());
        assert_eq!(cube_visual.kind(), VisualKind::Cube);
        assert!(cube_visual.flags().contains(BlockFlags::OCCLUDES_FULL_FACE));
        let shelf_visual = assets.resolve(NetworkIdMode::Sequential, shelf_values[0].sequential);
        assert_eq!(shelf_visual.kind(), VisualKind::Model);
        assert_eq!(shelf_visual.flags(), BlockFlags::OCCLUDES_FULL_FACE);
        let template = assets.model_templates()[shelf_visual.model_template().unwrap() as usize];
        assert!(
            assets.model_quads()[template.quad_start as usize
                ..(template.quad_start + template.quad_count) as usize]
                .iter()
                .all(|quad| {
                    quad.flags & MODEL_QUAD_FLAG_FACE_MASK != 0
                        && quad.flags & MODEL_QUAD_FLAG_FACE_MASK
                            == (quad.flags & MODEL_QUAD_FLAG_CULL_FACE_MASK) >> 4
                })
        );
        CompiledChiseledBookshelfFixture {
            assets,
            air: air_values,
            shelves: shelf_values,
            cube: cube_values,
        }
    })
}

fn mesh_chiseled_bookshelves(
    mode: NetworkIdMode,
    placements: &[([u8; 3], NetworkValues)],
    neighbours: &Neighbourhood<'_>,
) -> ChunkMesh {
    let fixture = compiled_chiseled_bookshelf_fixture();
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
    let bits = if palette.len() <= 2 { 1 } else { 2 };
    let center = sub_chunk(vec![packed_storage(bits, &palette, &placements)]);
    mesh_sub_chunk(
        &BlockClassifier::new(air),
        &fixture.assets,
        mode,
        neighbours,
        &center,
    )
}

#[test]
fn chiseled_bookshelf_rotations_and_front_slots_cull_as_full_faces() {
    let fixture = compiled_chiseled_bookshelf_fixture();
    let local = mesh_chiseled_bookshelves(
        NetworkIdMode::Sequential,
        &[
            ([8, 8, 8], fixture.shelf(5, 2)),
            ([8, 8, 7], fixture.shelf(63, 2)),
        ],
        &Neighbourhood::empty(),
    );
    assert_eq!(local.model_draw_refs().len(), 15);
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        for (direction, front, center, neighbour) in [
            (0, Face::PositiveZ, [5, 6, 15], [5, 6, 0]),
            (1, Face::NegativeX, [0, 5, 6], [15, 5, 6]),
            (2, Face::NegativeZ, [5, 6, 0], [5, 6, 15]),
            (3, Face::PositiveX, [15, 5, 6], [0, 5, 6]),
        ] {
            assert_eq!(
                fixture
                    .assets
                    .resolve(mode, fixture.shelf(5, direction).for_mode(mode),)
                    .variant(),
                (direction + 2) & 3
            );
            let selected = fixture
                .assets
                .resolve(mode, fixture.shelf(5, direction).for_mode(mode));
            let selected_template =
                fixture.assets.model_templates()[selected.model_template().unwrap() as usize];
            assert_eq!(
                fixture.assets.model_quads()[selected_template.quad_start as usize
                    ..(selected_template.quad_start + selected_template.quad_count) as usize]
                    .iter()
                    .map(|quad| quad.flags)
                    .collect::<Vec<_>>(),
                [
                    0x33, 0x44, 0x11, 0x22, 0x66, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55
                ]
            );
            let occluder = fixture.shelf(63, direction).for_mode(mode);
            let neighbour_chunk = sub_chunk(vec![packed_storage(
                1,
                &[fixture.air.for_mode(mode), occluder],
                &[(neighbour, 1)],
            )]);
            assert_eq!(
                ContributorResolver::new(
                    BlockClassifier::new(fixture.air.for_mode(mode)),
                    &fixture.assets,
                    mode,
                    &neighbour_chunk,
                )
                .resolve(neighbour)
                .primary_network_value(),
                Some(occluder)
            );
            assert_eq!(
                fixture.assets.resolve(mode, occluder).flags(),
                BlockFlags::OCCLUDES_FULL_FACE
            );
            let cube_center = sub_chunk(vec![packed_storage(
                1,
                &[fixture.air.for_mode(mode), fixture.cube.for_mode(mode)],
                &[(center, 1)],
            )]);
            let cube_mesh = mesh_sub_chunk(
                &BlockClassifier::new(fixture.air.for_mode(mode)),
                &fixture.assets,
                mode,
                &neighbourhood_for(front, &neighbour_chunk),
                &cube_center,
            );
            assert_eq!(cube_mesh.quad_count(), 5);
            let mesh = mesh_chiseled_bookshelves(
                mode,
                &[(center, fixture.shelf(5, direction))],
                &neighbourhood_for(front, &neighbour_chunk),
            );
            let transform = mesh.model_refs()[0].words()[0];
            assert_eq!(
                [
                    (transform & 0xf) as u8,
                    ((transform >> 4) & 0xf) as u8,
                    ((transform >> 8) & 0xf) as u8,
                ],
                center
            );
            assert_eq!(
                mesh.model_draw_refs().len(),
                5,
                "mode={mode:?} direction={direction}"
            );
            assert_eq!(mesh.model_lighting().len(), 11);
            assert_eq!(mesh.model_refs().len(), 1);
        }
    }
}

#[test]
fn chiseled_bookshelf_cull_faces_cross_every_subchunk_boundary() {
    let fixture = compiled_chiseled_bookshelf_fixture();
    for (face, center, neighbour, expected_draws) in [
        (Face::NegativeX, [0, 5, 6], [15, 5, 6], 10),
        (Face::PositiveX, [15, 5, 6], [0, 5, 6], 10),
        (Face::NegativeY, [5, 0, 6], [5, 15, 6], 10),
        (Face::PositiveY, [5, 15, 6], [5, 0, 6], 10),
        (Face::NegativeZ, [5, 6, 0], [5, 6, 15], 5),
        (Face::PositiveZ, [5, 6, 15], [5, 6, 0], 10),
    ] {
        let neighbour_chunk = sub_chunk(vec![packed_storage(
            1,
            &[fixture.air.sequential, fixture.shelf(0, 2).sequential],
            &[(neighbour, 1)],
        )]);
        let mesh = mesh_chiseled_bookshelves(
            NetworkIdMode::Sequential,
            &[(center, fixture.shelf(63, 2))],
            &neighbourhood_for(face, &neighbour_chunk),
        );
        assert_eq!(
            mesh.model_draw_refs().len(),
            expected_draws,
            "face={face:?}"
        );
        assert_eq!(mesh.model_lighting().len(), 11);
    }
}

#[test]
fn chiseled_bookshelf_dense_subchunk_has_bounded_exact_model_streams_and_closes_caves() {
    let fixture = compiled_chiseled_bookshelf_fixture();
    let shelf = fixture.shelf(63, 2).sequential;
    let center = sub_chunk(vec![uniform_storage(shelf)]);
    let mesh = mesh_sub_chunk(
        &BlockClassifier::new(fixture.air.sequential),
        &fixture.assets,
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &center,
    );

    assert_eq!(mesh.model_refs().len(), 4096 - 14 * 14 * 14);
    assert_eq!(mesh.model_draw_refs().len(), 2_816);
    assert_eq!(mesh.model_lighting().len(), (4096 - 14 * 14 * 14) * 11);
    assert!(
        mesh.model_refs()
            .iter()
            .enumerate()
            .all(|(index, reference)| reference.words()[2] == (index * 11) as u32)
    );
    assert!(mesh.transparent_model_draw_refs().is_empty());
    assert!(mesh.connectivity().is_empty());
}
