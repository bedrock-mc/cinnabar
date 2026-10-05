struct PortalFixture {
    assets: RuntimeAssets,
    air: u32,
    unknown: u32,
    axis_x: u32,
    axis_z: u32,
    stone: u32,
}

fn portal_fixture() -> &'static PortalFixture {
    static FIXTURE: OnceLock<PortalFixture> = OnceLock::new();
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
        let air = add(
            "minecraft:air",
            "{}".into(),
            BlockFlags::AIR,
            ModelFamily::Air,
        );
        let [unknown, axis_x, axis_z] = ["unknown", "x", "z"].map(|axis| {
            add(
                assets::NETHER_PORTAL_IDENTIFIER,
                format!(r#"{{"portal_axis":{{"type":"string","value":"{axis}"}}}}"#),
                BlockFlags::empty(),
                ModelFamily::Unknown,
            )
        });
        let stone = add(
            "minecraft:stone",
            "{}".into(),
            BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
            ModelFamily::Cube,
        );
        let directory = tempfile::tempdir().unwrap();
        write_slab_render_pack(directory.path(), "portal", "unused", "stone");
        let mut lights = vec![assets::LightProperties::default(); records.len()];
        for id in [unknown, axis_x, axis_z] {
            lights[id as usize] = assets::LightProperties::new(7, 0).unwrap();
        }
        let compiled = compile_pack_with_lights(directory.path(), &records, &lights).unwrap();
        PortalFixture {
            assets: RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap(),
            air,
            unknown,
            axis_x,
            axis_z,
            stone,
        }
    })
}

fn portal_chunk(placements: &[([u8; 3], u32)]) -> SubChunk {
    let fixture = portal_fixture();
    let mut palette = vec![fixture.air];
    let indexed = placements
        .iter()
        .map(|&(position, id)| {
            let index = palette
                .iter()
                .position(|&value| value == id)
                .unwrap_or_else(|| {
                    palette.push(id);
                    palette.len() - 1
                });
            (position, index)
        })
        .collect::<Vec<_>>();
    let mut bytes = vec![9, 1, 0];
    bytes.extend(packed_storage(3, &palette, &indexed));
    SubChunk::decode(&bytes, &RawBlockIds { air: fixture.air })
}

fn mesh_portals(sub_chunk: &SubChunk, neighbours: &Neighbourhood<'_>) -> ChunkMesh {
    let fixture = portal_fixture();
    mesh_sub_chunk(
        &BlockClassifier::new(fixture.air),
        &fixture.assets,
        NetworkIdMode::Sequential,
        neighbours,
        sub_chunk,
    )
}

#[test]
fn nether_portals_publish_blended_faces_and_cull_only_touching_boundaries() {
    let fixture = portal_fixture();
    let mesh = mesh_portals(
        &portal_chunk(&[([7, 8, 8], fixture.axis_x)]),
        &Neighbourhood::empty(),
    );
    assert!(mesh.quads().is_empty());
    assert!(mesh.model_draw_refs().is_empty());
    assert_eq!(mesh.transparent_model_draw_refs().len(), 6);

    // Touching same-family portal faces disappear, including mixed state axes.
    let mesh = mesh_portals(
        &portal_chunk(&[([7, 8, 8], fixture.axis_x), ([8, 8, 8], fixture.axis_x)]),
        &Neighbourhood::empty(),
    );
    assert_eq!(mesh.transparent_model_draw_refs().len(), 10);
    let mesh = mesh_portals(
        &portal_chunk(&[([7, 8, 8], fixture.axis_x), ([7, 8, 9], fixture.axis_z)]),
        &Neighbourhood::empty(),
    );
    assert_eq!(
        mesh.transparent_model_draw_refs().len(),
        10,
        "portal property culls across axes even on inset faces"
    );
    // Stone outside a broad inset face cannot hide the visible portal surface.
    let mesh = mesh_portals(
        &portal_chunk(&[([7, 8, 8], fixture.axis_x), ([7, 8, 9], fixture.stone)]),
        &Neighbourhood::empty(),
    );
    assert_eq!(mesh.transparent_model_draw_refs().len(), 6);
    let mesh = mesh_portals(
        &portal_chunk(&[([7, 8, 8], fixture.axis_x), ([8, 8, 8], fixture.stone)]),
        &Neighbourhood::empty(),
    );
    assert_eq!(mesh.transparent_model_draw_refs().len(), 5);
}

#[test]
fn portal_flat_lighting_samples_inset_and_boundary_cells_with_registered_emission_minimum() {
    let fixture = portal_fixture();
    let origin = [7, 8, 8];
    let chunk = portal_chunk(&[(origin, fixture.axis_x), ([8, 9, 8], fixture.stone)]);
    let sampler = |coordinate| {
        if coordinate == origin.map(i32::from) {
            MeshLightSample::try_new(15, 4).unwrap()
        } else {
            MeshLightSample::try_new(1, 2).unwrap()
        }
    };
    let mesh = mesh_sub_chunk_with_lighting(
        &BlockClassifier::new(fixture.air),
        &fixture.assets,
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &chunk,
        &sampler,
    );
    let emission = fixture
        .assets
        .resolve(NetworkIdMode::Sequential, fixture.axis_x)
        .light_properties()
        .emission();
    for (index, lighting) in mesh.model_lighting().iter().enumerate() {
        let [block, sky] = if index >= 4 { [15, 4] } else { [emission, 2] };
        assert_eq!(
            lighting.samples(),
            [u16::from(block) | u16::from(sky) << 4 | 1 << 11; 4],
            "flat portal face {index} has no corner AO"
        );
    }
}

#[test]
fn legacy_portal_axis_selects_x_from_portal_neighbors_including_chunk_boundaries() {
    let fixture = portal_fixture();
    let template_z = fixture
        .assets
        .resolve(NetworkIdMode::Sequential, fixture.axis_z)
        .model_template()
        .unwrap();
    let template_x = fixture
        .assets
        .resolve(NetworkIdMode::Sequential, fixture.axis_x)
        .model_template()
        .unwrap();
    let mesh = mesh_portals(
        &portal_chunk(&[([7, 8, 8], fixture.unknown)]),
        &Neighbourhood::empty(),
    );
    assert_eq!(mesh.model_refs()[0].words()[1], template_z);
    let mesh = mesh_portals(
        &portal_chunk(&[([7, 8, 8], fixture.unknown), ([8, 8, 8], fixture.axis_z)]),
        &Neighbourhood::empty(),
    );
    assert_eq!(mesh.model_refs()[0].words()[1], template_x);
    let neighbor = portal_chunk(&[([0, 8, 8], fixture.axis_x)]);
    let chunk = portal_chunk(&[([15, 8, 8], fixture.unknown)]);
    let mesh = mesh_portals(&chunk, &Neighbourhood::empty().with_positive_x(&neighbor));
    assert_eq!(mesh.model_refs()[0].words()[1], template_x);
}
