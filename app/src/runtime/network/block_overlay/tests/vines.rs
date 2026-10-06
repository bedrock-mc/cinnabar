use super::*;

#[test]
fn custom_block_property_alias_applies_each_facing_permutation() {
    for spelling in ["q.block_property", "query.block_property"] {
        let block = block(
            "test:vine",
            4,
            CustomBlockVisuals {
                state_axes: Box::new([CustomStateAxis {
                    name: "custom:facing_direction".into(),
                    values: (0..4).map(CustomStateValue::Int).collect(),
                }]),
                permutations: (1..4)
                    .map(|facing| CustomPermutation {
                        condition: format!("{spelling}('custom:facing_direction') == {facing}")
                            .into(),
                        components: turn(facing as i32),
                    })
                    .collect(),
                ..CustomBlockVisuals::default()
            },
        );
        let expressions = super::super::condition::BlockExpressions::new(&block);
        for state in 0..4 {
            let mut gaps = OverlayGaps::default();
            let values = block.state_values(state).unwrap();
            let resolved = super::super::condition::state_visual(
                &block,
                &expressions,
                Some(&values),
                &mut gaps,
            );
            assert_eq!(
                resolved
                    .components
                    .transformation
                    .map_or(0, |transform| transform.rotation[1]),
                state as i32,
                "{spelling}, facing {state}"
            );
            assert_eq!(gaps.unevaluated_permutations, 0);
        }
    }
}

#[test]
fn block_property_permutations_rotate_each_runtime_vine_state() {
    let view = view_with_geometry(
        br#"{"minecraft:geometry":[{
        "description":{"identifier":"geometry.gen","texture_width":32,"texture_height":32},
        "bones":[{"name":"root","cubes":[{
            "origin":[-8,0,-8],"size":[16,16,0],
            "uv":{"north":{"uv":[0,0],"uv_size":[16,16]}}
        }]}]
    }]}"#,
    );
    let vine = block(
        "test:vine",
        4,
        CustomBlockVisuals {
            base: CustomVisualComponents {
                geometry: Some("geometry.gen".into()),
                materials: materials("gen"),
                ..CustomVisualComponents::default()
            },
            state_axes: Box::new([CustomStateAxis {
                name: "custom:facing_direction".into(),
                values: (0..4).map(CustomStateValue::Int).collect(),
            }]),
            permutations: [(1, 2), (2, 1), (3, 3)]
                .map(|(facing, quarters)| CustomPermutation {
                    condition: format!(
                        "query.block_property('custom:facing_direction') == {facing}"
                    )
                    .into(),
                    components: turn(quarters),
                })
                .into(),
            ..CustomBlockVisuals::default()
        },
    );
    let states = vine.hashed_states();
    let blocks = CustomBlocks {
        blocks: vec![vine].into(),
        ..CustomBlocks::default()
    };
    let expected = [
        ([0, 0, 0], [256, 256, 0]),
        ([0, 0, 256], [256, 256, 256]),
        ([0, 0, 0], [0, 256, 256]),
        ([256, 0, 0], [256, 256, 256]),
    ];
    for hashed in [false, true] {
        let compiled = compile_block_overlay(&view, &blocks, hashed, None).unwrap();
        assert_eq!(compiled.gaps.unevaluated_permutations, 0);
        let session = RuntimeAssets::diagnostic()
            .with_block_overlay(1, &compiled.overlay)
            .unwrap();
        for (state, &(expected_min, expected_max)) in expected.iter().enumerate() {
            let (mode, id) = if hashed {
                (NetworkIdMode::Hashed, states[state].hash)
            } else {
                (NetworkIdMode::Sequential, 1 + state as u32)
            };
            let template = session.resolve(mode, id).model_template().unwrap();
            let template = session.model_templates()[template as usize];
            assert_eq!(template.quad_count, 1);
            let quad = &session.model_quads()[template.quad_start as usize];
            let min = std::array::from_fn::<_, 3, _>(|axis| {
                quad.positions
                    .iter()
                    .map(|point| point[axis])
                    .min()
                    .unwrap()
            });
            let max = std::array::from_fn::<_, 3, _>(|axis| {
                quad.positions
                    .iter()
                    .map(|point| point[axis])
                    .max()
                    .unwrap()
            });
            assert_eq!(
                (min, max),
                (expected_min, expected_max),
                "{mode:?}, facing {state}"
            );
        }
    }
}

#[test]
fn custom_model_preserves_faces_beyond_two_visibility_masks() {
    let cubes = (0..15)
        .map(|index| {
            serde_json::json!({
                "origin":[index,0,0], "size":[1,1,1], "uv":[0,0]
            })
        })
        .collect::<Vec<_>>();
    let geometry = serde_json::json!({"minecraft:geometry":[{
        "description":{"identifier":"geometry.gen","texture_width":32,"texture_height":32},
        "bones":[{"name":"root","cubes":cubes}]
    }]});
    let view = view_with_geometry(&serde_json::to_vec(&geometry).unwrap());
    let blocks = CustomBlocks {
        blocks: vec![block(
            "test:vine",
            1,
            CustomBlockVisuals {
                base: CustomVisualComponents {
                    geometry: Some("geometry.gen".into()),
                    materials: materials("gen"),
                    ..CustomVisualComponents::default()
                },
                ..CustomBlockVisuals::default()
            },
        )]
        .into(),
        ..CustomBlocks::default()
    };
    let compiled = compile_block_overlay(&view, &blocks, false, None).unwrap();
    assert_eq!(compiled.overlay.model_quads.len(), cubes.len() * 6);
    assert_eq!(compiled.gaps.truncated_models, 0);
    let session = RuntimeAssets::diagnostic()
        .with_block_overlay(1, &compiled.overlay)
        .expect("all model parts publish");
    let mut template = session
        .resolve(NetworkIdMode::Sequential, 1)
        .model_template()
        .unwrap();
    let mut faces = 0;
    loop {
        let part = session.model_templates()[template as usize];
        faces += part.quad_count;
        assert!(part.quad_count <= u32::BITS);
        if part.flags & assets::MODEL_TEMPLATE_FLAG_COMPOUND_NEXT == 0 {
            break;
        }
        template += 1;
    }
    assert_eq!(faces as usize, cubes.len() * 6);
}

#[test]
fn custom_model_fixture_retains_lamp_vine_and_plant_geometry() {
    let Some(dir) = std::env::var_os("CINNABAR_PACKCACHE_DIR") else {
        eprintln!(
            "skipping custom_model_fixture_retains_lamp_vine_and_plant_geometry: fixture unavailable; requires CINNABAR_PACKCACHE_DIR containing offline cached packs"
        );
        return;
    };
    let targets = [
        (
            "hive:climbing_rose_vine",
            "geometry.hive.block.climbing.rose.vine",
            33,
        ),
        ("hive:large_lamp", "geometry.hive.block.large.lamp", 40),
        ("hive:red_snapdragons", "geometry.hive.block.plant", 2),
    ];
    let wanted = targets.map(|(_, geometry, _)| geometry).into();
    let mut checked = std::collections::HashSet::new();
    for entry in std::fs::read_dir(dir).expect("packcache dir").flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "zip") {
            continue;
        }
        let Some(view) = super::super::super::local_pack::local_pack_view_at(&path) else {
            continue;
        };
        let geometries = super::super::geometry::geometry_catalog(&view, &wanted);
        for (name, identifier, expected_faces) in targets {
            if !geometries.contains_key(identifier) {
                continue;
            }
            let mut instances = materials(name).unwrap();
            instances[0].render_method = Some("alpha_test".into());
            instances[0].ambient_occlusion = Some(0.0);
            instances[0].face_dimming = Some(false);
            let blocks = CustomBlocks {
                blocks: vec![block(
                    name,
                    1,
                    CustomBlockVisuals {
                        base: CustomVisualComponents {
                            geometry: Some(identifier.into()),
                            materials: Some(instances),
                            light_dampening: Some(0),
                            light_emission: Some(if name == "hive:large_lamp" { 15 } else { 0 }),
                            ..CustomVisualComponents::default()
                        },
                        ..CustomBlockVisuals::default()
                    },
                )]
                .into(),
                ..CustomBlocks::default()
            };
            let compiled = compile_block_overlay(&view, &blocks, false, None).unwrap();
            assert_eq!(compiled.gaps.missing_textures, 0, "{name}");
            assert_eq!(compiled.gaps.truncated_models, 0, "{name}");
            assert_eq!(compiled.overlay.model_quads.len(), expected_faces, "{name}");
            let session = RuntimeAssets::diagnostic()
                .with_block_overlay(1, &compiled.overlay)
                .unwrap();
            let visual = session.resolve(NetworkIdMode::Sequential, 1);
            assert_eq!(visual.light_properties().filter(), 0, "{name}");
            assert_eq!(
                visual.light_properties().emission(),
                blocks.blocks[0].visual.base.light_emission.unwrap()
            );
            for quad in session.model_quads() {
                let material = session.material(quad.material);
                assert_ne!(
                    material.flags & assets::MATERIAL_FLAG_DISABLE_AO,
                    0,
                    "{name}"
                );
                assert_ne!(
                    material.flags & assets::MATERIAL_FLAG_DISABLE_FACE_DIMMING,
                    0,
                    "{name}"
                );
            }
            checked.insert(name);
        }
    }
    if checked.is_empty() {
        eprintln!(
            "skipping custom_model_fixture_retains_lamp_vine_and_plant_geometry: fixture unavailable; no admitted model geometries in cached packs"
        );
        return;
    }
    assert_eq!(checked.len(), targets.len());
}
