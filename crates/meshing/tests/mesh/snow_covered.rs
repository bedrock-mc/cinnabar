/// TopSnow renders its cuboid on the snow pass and the block it covers on that
/// block's own pass (tessellateTopSnowInWorld). A crossed
/// plant is not a conflicting solid and must not become a diagnostic cube.
#[test]
fn covered_snow_keeps_all_five_observed_plant_families_on_every_height() {
    let fixture = compiled_snow_fixture();
    for (height, &snow) in fixture.covered_layers.iter().enumerate() {
        for &plant in &fixture.plants {
            for reverse in [false, true] {
                let mut layers = vec![
                    packed_storage(1, &[fixture.air, snow], &[([8, 8, 8], 1)]),
                    packed_storage(1, &[fixture.air, plant], &[([8, 8, 8], 1)]),
                ];
                if reverse {
                    layers.reverse();
                }
                let chunk = sub_chunk(layers);
                for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
                    let classifier = BlockClassifier::new(fixture.air);
                    let resolved =
                        ContributorResolver::new(classifier, &fixture.assets, mode, &chunk)
                            .resolve([8, 8, 8]);
                    let direct = ContributorResolver::resolve_direct(
                        classifier,
                        &fixture.assets,
                        mode,
                        &chunk,
                        [8, 8, 8],
                    );
                    assert_eq!(resolved.primary_network_value(), Some(snow));
                    assert_eq!(direct.primary_network_value(), Some(snow));
                    assert_eq!(resolved.diagnostic_network_value(), None);
                    assert_eq!(direct.diagnostic_network_value(), None);
                    let mesh = mesh_sub_chunk(
                        &classifier,
                        &fixture.assets,
                        mode,
                        &Neighbourhood::empty(),
                        &chunk,
                    );
                    assert!(mesh.diagnostic_geometry().entries().is_empty());
                    let expected_models = if height + 1 == fixture.layers.len() {
                        1
                    } else {
                        2
                    };
                    assert_eq!(mesh.model_refs().len(), expected_models);
                    let plant_template = fixture
                        .assets
                        .resolve(mode, plant)
                        .model_template()
                        .unwrap();
                    let plant_ref = mesh
                        .model_refs()
                        .iter()
                        .find(|reference| reference.words()[1] == plant_template)
                        .expect("covered plant survives as its own crossed model");
                    assert_eq!(plant_ref.words()[3], 0b11);
                    assert_eq!(plant_ref.words()[0], 8 | (8 << 4) | (8 << 8));
                    assert_eq!(
                        mesh.cube_quads().len(),
                        if expected_models == 1 { 6 } else { 0 }
                    );
                    // The existing bound covers a compound/connection model,
                    // which is larger than the two simple covered-snow models.
                    assert!(
                        meshing::mesh_output_byte_len(
                            &mesh,
                            &meshing::PackedBiomeRecord::fallback()
                        ) <= meshing::MeshOutputBounds::new(&fixture.assets).for_sub_chunk(
                            &chunk,
                            &fixture.assets,
                            mode
                        )
                    );
                }
            }
        }
    }
}

#[test]
fn uniform_full_height_snow_still_visits_the_covered_plant_models() {
    let fixture = compiled_snow_fixture();
    for &snow in [
        fixture.covered_layers[0],
        *fixture.covered_layers.last().unwrap(),
    ]
    .iter()
    {
        let chunk = sub_chunk(vec![
            uniform_storage(snow),
            uniform_storage(fixture.plants[0]),
        ]);
        let mesh = mesh_snow(&chunk, &Neighbourhood::empty());
        assert!(mesh.diagnostic_geometry().entries().is_empty());
        let expected = if snow == *fixture.covered_layers.last().unwrap() {
            1
        } else {
            2
        };
        assert_eq!(
            mesh.model_refs().len(),
            world::BLOCKS_PER_SUB_CHUNK * expected
        );
        assert!(
            meshing::mesh_output_byte_len(&mesh, &meshing::PackedBiomeRecord::fallback())
                <= meshing::MeshOutputBounds::new(&fixture.assets).for_sub_chunk(
                    &chunk,
                    &fixture.assets,
                    NetworkIdMode::Sequential
                )
        );
    }
}

#[test]
fn covered_snow_is_not_generic_permission_for_multiple_primary_solids() {
    let fixture = compiled_snow_fixture();
    let snow = fixture.covered_layers[0];
    let plant = fixture.plants[0];
    for ids in [vec![snow, fixture.cube], vec![plant, fixture.plants[1]]] {
        let chunk = sub_chunk(
            ids.iter()
                .map(|&id| packed_storage(1, &[fixture.air, id], &[([8, 8, 8], 1)]))
                .collect(),
        );
        let resolved = ContributorResolver::new(
            BlockClassifier::new(fixture.air),
            &fixture.assets,
            NetworkIdMode::Sequential,
            &chunk,
        )
        .resolve([8, 8, 8]);
        assert_eq!(resolved.primary_network_value(), None);
        assert_eq!(resolved.diagnostic_network_value(), ids.last().copied());
        let mesh = mesh_snow(&chunk, &Neighbourhood::empty());
        assert!(mesh.model_refs().is_empty());
        assert_eq!(mesh.cube_quads().len(), 6);
    }
}

#[test]
fn covered_snow_surface_still_culls_a_touching_shorter_snow_side() {
    let fixture = compiled_snow_fixture();
    let chunk = sub_chunk(vec![
        packed_storage(
            2,
            &[fixture.air, fixture.covered_layers[3], fixture.layers[0]],
            &[([8, 8, 8], 1), ([7, 8, 8], 2)],
        ),
        packed_storage(1, &[fixture.air, fixture.plants[0]], &[([8, 8, 8], 1)]),
    ]);
    let mesh = mesh_snow(&chunk, &Neighbourhood::empty());
    assert!(mesh.diagnostic_geometry().entries().is_empty());
    assert_eq!(snow_model_mask(&mesh, [7, 8, 8]) & (1 << 1), 0);
    assert_eq!(snow_model_mask(&mesh, [8, 8, 8]) & (1 << 0), 1 << 0);
}

#[test]
fn covered_mushroom_light_does_not_select_emitting_snow_shading() {
    let fixture = compiled_snow_fixture();
    let mushroom = fixture.plants.iter().copied().find(|&id| {
        fixture.assets.resolve(NetworkIdMode::Sequential, id).light_properties().emission() > 0
    }).expect("fixture retains brown mushroom's native emission");
    let sample = MeshLightSample::try_new(1, 13).unwrap();
    let classifier = BlockClassifier::new(fixture.air);
    for &snow in &fixture.covered_layers {
        let snow_storage = packed_storage(1, &[fixture.air, snow], &[([8, 8, 8], 1)]);
        let snow_only = sub_chunk(vec![snow_storage.clone()]);
        for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
            let baseline = mesh_sub_chunk_with_lighting(
                &classifier, &fixture.assets, mode, &Neighbourhood::empty(), &snow_only, &|_| sample,
            );
            for reverse in [false, true] {
                let mut storages = vec![
                    snow_storage.clone(),
                    packed_storage(1, &[fixture.air, mushroom], &[([8, 8, 8], 1)]),
                ];
                if reverse { storages.reverse(); }
                let covered = sub_chunk(storages);
                let actual = mesh_sub_chunk_with_lighting(
                    &classifier, &fixture.assets, mode, &Neighbourhood::empty(), &covered, &|_| sample,
                );
                let direct = meshing::bake_quad_lighting_with_sampler(
                    &classifier, &fixture.assets, mode, &MeshNeighbourhood::new(&covered),
                    &|_| sample, [8, 8, 8], Face::NegativeX,
                    [[0, 0, 0], [0, 0, 256], [0, 256, 256], [0, 256, 0]],
                );
                assert!(direct.samples().iter().all(|value| value & (1 << 11) == 0));
                assert_eq!(actual.cube_lighting(), baseline.cube_lighting(), "full-height snow keeps its own material shading");
                if let Some(template) = fixture.assets.resolve(mode, snow).model_template() {
                    let reference = actual.model_refs().iter().find(|reference| reference.words()[1] == template).unwrap();
                    let start = reference.words()[2] as usize;
                    let count = fixture.assets.model_templates()[template as usize].quad_count as usize;
                    assert_eq!(&actual.model_lighting()[start..start + count], baseline.model_lighting(), "inset snow keeps its own material shading");
                    let direct = meshing::bake_template_lighting_with_sampler(
                        &classifier, &fixture.assets, mode, &MeshNeighbourhood::new(&covered),
                        &|_| sample, [8, 8, 8], template, 0,
                    ).unwrap();
                    assert_eq!(direct, baseline.model_lighting());
                }
                for lighting in actual.cube_lighting().iter().chain(actual.model_lighting()) {
                    assert!(lighting.samples().iter().all(|sample| sample & 0xf == 1), "propagated mushroom light is preserved");
                }
                assert!(actual.diagnostic_geometry().entries().is_empty());
            }
        }
    }
}
