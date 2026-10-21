use super::*;

#[test]
fn animated_replacements_use_the_resolved_default_state() {
    let base = base();
    let keys = keys();
    let view = timeline_pack(
        json!({"texture_data":{"state":{"textures":"textures/replacement"}}}),
        json!([{"atlas_tile":"state","flipbook_texture":"textures/replacement","frames":[1,0,1],"ticks_per_frame":3}]),
        &[(
            "textures/replacement",
            16,
            32,
            [200, 10, 20, 255],
            [20, 30, 200, 255],
        )],
    );
    let compiled = compile(&view, &base, &keys);
    let runtime = base.with_block_overlay(2, &compiled.overlay).unwrap();
    for id in [1, 2] {
        let material = runtime.material(id);
        assert_ne!(material.animation, assets::NO_ANIMATION);
        let animation = runtime.animations()[material.animation as usize];
        assert_eq!(animation.frame_count, 3);
        assert_eq!(animation.ticks_per_frame, 3);
        assert_eq!(material_color(&runtime, id), [20, 30, 200, 255]);
    }
    assert_eq!(runtime.material(1).animation, runtime.material(2).animation);
    assert_eq!(runtime.material(1).texture, runtime.material(2).texture);
    assert_eq!(compiled.overlay.textures.first().unwrap().layers, 3);
    assert_eq!(compiled.overlay.materials.len(), 2);
}

#[test]
fn weighted_selectors_reserve_the_base_material_table_and_allow_later_static_routes() {
    let base = carrier(
        &[[100, 120, 140, 255]],
        [1; 6],
        &vec![0; assets::MAX_MATERIALS - 3],
    );
    let keys = MaterialKeys::from_entries([(1, "weighted"), (2, "static")]);
    let view = timeline_pack(
        json!({"texture_data":{
            "weighted":{"textures":{"variations":["textures/shared","textures/shared"]}},
            "static":{"textures":"textures/shared"}
        }}),
        json!([]),
        &[(
            "textures/shared",
            16,
            16,
            [20, 30, 200, 255],
            [20, 30, 200, 255],
        )],
    );
    let compiled = compile(&view, &base, &keys);
    assert_eq!(
        base.materials().len() + compiled.overlay.materials.len(),
        assets::MAX_MATERIALS
    );
    let runtime = base.with_block_overlay(2, &compiled.overlay).unwrap();
    assert_eq!(runtime.material(1).variation_count, 0);
    assert_eq!(runtime.material(1), base.material(1));
    assert_eq!(material_color(&runtime, 2), [20, 30, 200, 255]);
    assert_eq!(compiled.overlay.material_overrides.len(), 1);
    assert_eq!(compiled.overlay.textures.first().unwrap().layers, 2);
}

#[test]
fn repeated_raster_paths_in_real_later_states_do_not_inherit_the_default_flipbook() {
    let base = base();
    let keys = MaterialKeys::from_entries([(1, "state"), (2, "state")]).with_sources(
        [(
            "state".into(),
            json!({"textures":["textures/shared","textures/shared"]}),
        )],
        [0, 1].map(|variant| {
            (
                variant + 1,
                MaterialTextureSource {
                    key: "state".into(),
                    variant,
                    path: "textures/shared".into(),
                    raster: "textures/shared.png".into(),
                    baked_tint: None,
                },
            )
        }),
        [],
    );
    let view = timeline_pack(
        json!({}),
        json!([{"atlas_tile":"state","flipbook_texture":"textures/shared","frames":[1,0,1],"ticks_per_frame":3}]),
        &[(
            "textures/shared",
            16,
            32,
            [200, 10, 20, 255],
            [20, 30, 200, 255],
        )],
    );
    let compiled = compile(&view, &base, &keys);
    let runtime = base.with_block_overlay(2, &compiled.overlay).unwrap();
    assert_ne!(runtime.material(1).animation, assets::NO_ANIMATION);
    assert_eq!(material_color(&runtime, 1), [20, 30, 200, 255]);
    assert_eq!(runtime.material(2).animation, assets::NO_ANIMATION);
    assert_eq!(material_color(&runtime, 2), [200, 10, 20, 255]);
}

#[test]
fn failed_carried_faces_and_change_probes_release_unpublished_admissions() {
    for weighted_probe in [false, true] {
        let base = base();
        let original = keys();
        let mut terrain = original
            .terrain_entries()
            .map(|(key, entry)| (key.into(), entry.clone()))
            .collect::<Vec<_>>();
        let mut blocks = original.block_sources().to_vec();
        blocks[1].carried_keys = std::array::from_fn(|_| Some("good".into()));
        if weighted_probe {
            terrain.push((
                "carry".into(),
                json!({"textures":{"variations":["textures/first","textures/missing"]}}),
            ));
            blocks[0].carried_keys = std::array::from_fn(|_| Some("carry".into()));
        } else {
            blocks[0].carried_keys = std::array::from_fn(|index| {
                Some(if index == 0 { "first" } else { "missing" }.into())
            });
        }
        let keys = MaterialKeys::from_entries([(1, "state"), (2, "state")]).with_sources(
            terrain,
            original
                .material_sources()
                .map(|(id, source)| (id, source.clone())),
            blocks,
        );
        let good = [30, 40, 180, 255];
        let world = [140, 20, 30, 255];
        let view = stack(&[(
            if weighted_probe {
                json!({"texture_data":{"good":{"textures":"textures/good"}}})
            } else {
                json!({"texture_data":{
                    "world":{"textures":"textures/world"},
                    "first":{"textures":"textures/first"},
                    "missing":{"textures":"textures/missing"},
                    "good":{"textures":"textures/good"}
                }})
            },
            if weighted_probe {
                json!({})
            } else {
                json!({"fixture":{"textures":"world"}})
            },
            vec![
                ("textures/world", world),
                ("textures/first", [200, 50, 80, 255]),
                ("textures/good", good),
            ],
        )]);
        let compiled = super::super::super::super::compile_block_overlay(
            &view,
            &CustomBlocks::default(),
            false,
            Some(&keys),
            if weighted_probe { 2 } else { 3 },
            Some(&base),
        )
        .unwrap();
        assert_eq!(compiled.vanilla_carried.len(), 1);
        assert_eq!(compiled.vanilla_carried[0].visual, 1);
        let sheet = render_model::equipment::blocks::overlay_face_sheet(
            &compiled.overlay,
            compiled.vanilla_carried[0].faces,
            None,
        )
        .unwrap();
        assert!(
            sheet
                .rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == good)
        );
        assert_eq!(
            compiled.overlay.textures.first().unwrap().layers,
            if weighted_probe { 2 } else { 3 }
        );
        assert_eq!(
            compiled.overlay.materials.len(),
            if weighted_probe { 2 } else { 3 }
        );
        let runtime = base.with_block_overlay(2, &compiled.overlay).unwrap();
        if !weighted_probe {
            let material = runtime
                .resolve(NetworkIdMode::Sequential, 0)
                .face(BlockFace::Up)
                .material_id();
            assert_eq!(material_color(&runtime, material), world);
        }
    }
}

#[test]
fn custom_materials_share_the_combined_base_and_overlay_budget() {
    let base = carrier(
        &[[100, 120, 140, 255]],
        [1; 6],
        &vec![0; assets::MAX_MATERIALS - 3],
    );
    let view = timeline_pack(
        json!({"texture_data":{"first":{"textures":"textures/first"},"later":{"textures":"textures/later"}}}),
        json!([]),
        &[
            (
                "textures/first",
                16,
                16,
                [20, 30, 200, 255],
                [20, 30, 200, 255],
            ),
            (
                "textures/later",
                16,
                16,
                [200, 10, 20, 255],
                [200, 10, 20, 255],
            ),
        ],
    );
    let blocks = CustomBlocks {
        blocks: ["first", "later"]
            .map(|key| {
                block(
                    &format!("test:{key}"),
                    1,
                    CustomBlockVisuals {
                        base: CustomVisualComponents {
                            materials: materials(key),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                )
            })
            .into(),
        ..Default::default()
    };
    let compiled = super::super::super::super::compile_block_overlay(
        &view,
        &blocks,
        false,
        None,
        3,
        Some(&base),
    )
    .unwrap();
    assert_eq!(compiled.overlay.textures.first().unwrap().layers, 2);
    assert_eq!(
        base.materials().len() + compiled.overlay.materials.len(),
        assets::MAX_MATERIALS
    );
    let runtime = base.with_block_overlay(2, &compiled.overlay).unwrap();
    assert_eq!(
        runtime.resolve(NetworkIdMode::Sequential, 2).kind(),
        VisualKind::Cube
    );
    assert_eq!(
        runtime.resolve(NetworkIdMode::Sequential, 3).kind(),
        VisualKind::Cube,
        "budget-refused faces degrade per face instead of sinking the cube"
    );
    assert!(
        compiled.overlay.visuals[1]
            .faces
            .iter()
            .all(|&face| face == assets::DIAGNOSTIC_MATERIAL)
    );
}

#[test]
fn failed_faces_degrade_per_face_without_leaking_materials() {
    let base = base();
    let view = timeline_pack(
        json!({"texture_data":{
            "first":{"textures":"textures/first"}, "missing":{"textures":"textures/missing"},
            "good":{"textures":"textures/good"}
        }}),
        json!([]),
        &[
            (
                "textures/first",
                16,
                16,
                [200, 10, 20, 255],
                [200, 10, 20, 255],
            ),
            (
                "textures/good",
                16,
                16,
                [20, 30, 200, 255],
                [20, 30, 200, 255],
            ),
        ],
    );
    let mut broken = materials("first").unwrap().into_vec();
    let mut missing = broken[0].clone();
    missing.name = "east".into();
    missing.texture = "missing".into();
    broken.push(missing);
    let blocks = CustomBlocks {
        blocks: vec![
            block(
                "test:broken",
                1,
                CustomBlockVisuals {
                    base: CustomVisualComponents {
                        materials: Some(broken.into()),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            ),
            block(
                "test:good",
                1,
                CustomBlockVisuals {
                    base: CustomVisualComponents {
                        materials: materials("good"),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            ),
        ]
        .into(),
        ..Default::default()
    };
    let compiled = super::super::super::super::compile_block_overlay(
        &view,
        &blocks,
        false,
        None,
        2,
        Some(&base),
    )
    .unwrap();
    assert_eq!(compiled.overlay.visuals[0].kind, VisualKind::Cube);
    assert_eq!(
        compiled.overlay.visuals[0].faces[BlockFace::East as usize],
        assets::DIAGNOSTIC_MATERIAL,
        "only the failed east face degrades"
    );
    assert_eq!(compiled.overlay.visuals[1].kind, VisualKind::Cube);
    assert_eq!(
        compiled
            .overlay
            .textures
            .iter()
            .map(|shard| shard.layers)
            .collect::<Vec<_>>(),
        [2, 1],
        "the kept first-face raster and the good raster live on their own shards"
    );
    assert_eq!(compiled.overlay.materials.len(), 3);
    let runtime = base.with_block_overlay(2, &compiled.overlay).unwrap();
    let material = runtime
        .resolve(NetworkIdMode::Sequential, 3)
        .face(BlockFace::Up)
        .material_id();
    assert_eq!(material_color(&runtime, material), [20, 30, 200, 255]);
}
