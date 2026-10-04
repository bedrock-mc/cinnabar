use super::*;
use crate::block_entity::{
    chest::{ChestPair, ChestVariant},
    mesh::{Facing, MAX_BLOCK_ENTITY_VERTICES},
    sign::{SignFace, SignMount},
};

/// Builds a lawful atlas with static chests, animated portals and beams, and crack textures.
fn assets_with_chest_offset(chest_x: u32) -> assets::RuntimeBlockEntityAssets {
    let mut placements = [
        ("textures/entity/chest/normal", chest_x, 64, 64),
        ("textures/environment/destroy_stage_0", 64, 16, 16),
        ("textures/entity/end_portal", 80, 256, 256),
        ("textures/entity/beacon_beam", 336, 16, 16),
    ]
    .map(|(name, x, width, height)| assets::BlockEntityPlacement {
        name: name.into(),
        x,
        y: 0,
        width,
        height,
    });
    placements.sort_by(|a, b| a.name.cmp(&b.name));
    let bytes = assets::encode_block_entity_catalog(
        b"{}",
        1024,
        256,
        &vec![255; 1024 * 256 * 4],
        &placements,
    )
    .unwrap();
    assets::RuntimeBlockEntityAssets::decode(&bytes).unwrap()
}

/// Installs the authored atlas without any local or proprietary asset dependency.
fn scene() -> BlockEntityScene {
    let mut scene = BlockEntityScene::default();
    scene.install_assets(&assets_with_chest_offset(0));
    scene
}

/// Creates one static chest with separately controllable position and light.
fn chest(index: i32, light: f32) -> BlockEntitySubmission {
    BlockEntitySubmission {
        block: [index % 20, 64, index / 20],
        light: light.into(),
        kind: BlockEntityKind::Chest(ChestModel {
            variant: ChestVariant::Normal,
            facing: Facing::North,
            pair: ChestPair::Single,
            lid: 0.0,
        }),
    }
}

/// Creates a portal that emits geometry into both solid and additive draw layers.
fn portal(index: i32) -> BlockEntitySubmission {
    BlockEntitySubmission {
        block: [index, 63, 0],
        light: 1.0.into(),
        kind: BlockEntityKind::EndPortal,
    }
}

/// Rebuilds every submission using the original emission loop, without mesh fragments.
fn reference_frame(
    scene: &BlockEntityScene,
    clock: SceneClock,
    cracks: &[CrackInstance],
    submissions: &[BlockEntitySubmission],
) -> (BlockEntityFrame, u64) {
    let atlas = scene.atlas.as_ref().unwrap();
    let text = scene.text.as_ref().unwrap();
    let mut builder = MeshBuilder::new(atlas.size());
    for submission in submissions {
        submission.light.apply(&mut builder);
        emit_submission(
            &mut builder,
            atlas,
            (&scene.heads, &scene.mobs),
            submission,
            clock,
        );
    }
    BlockEntityLight::Scalar(1.0).apply(&mut builder);
    for crack in cracks {
        emit_crack(&mut builder, atlas, crack);
    }
    let rejected = builder.rejected_quads;
    (
        BlockEntityFrame {
            revision: scene.frame.revision.wrapping_add(1),
            atlas: scene.image.clone(),
            dynamic_revision: text.revision(),
            dynamic_rgba8: if scene.frame.dynamic_revision != text.revision()
                || scene.frame.dynamic_rgba8.is_empty()
            {
                Arc::from(text.pixels())
            } else {
                Arc::clone(&scene.frame.dynamic_rgba8)
            },
            solid: builder.solid.into(),
            overlay: builder.overlay.into(),
            crack: builder.crack.into(),
            additive: builder.additive.into(),
        },
        rejected,
    )
}

/// Checks every published vertex and dynamic pixel against a complete rebuild.
fn assert_matches_reference(
    scene: &mut BlockEntityScene,
    ticks: f64,
    cracks: &[CrackInstance],
    submissions: &[BlockEntitySubmission],
) {
    let clock = SceneClock { ticks };
    let (expected, rejected) = reference_frame(scene, clock, cracks, submissions);
    let frame = scene.update(clock, cracks, submissions);
    assert_eq!(frame.solid, expected.solid);
    assert_eq!(frame.overlay, expected.overlay);
    assert_eq!(frame.crack, expected.crack);
    assert_eq!(frame.additive, expected.additive);
    assert_eq!(frame.dynamic_revision, expected.dynamic_revision);
    assert_eq!(frame.dynamic_rgba8, expected.dynamic_rgba8);
    assert_eq!(scene.rejected_quads(), rejected);
}

#[test]
fn mixed_scenes_build_static_models_once_and_preserve_every_draw_layer() {
    let mut scene = scene();
    let submissions = [
        chest(0, 1.0),
        portal(1),
        chest(2, 0.25),
        BlockEntitySubmission {
            block: [3, 64, 0],
            light: 0.5.into(),
            kind: BlockEntityKind::Beacon(BeaconModel {
                height: 24,
                tint: [0.25, 0.5, 1.0],
            }),
        },
    ];
    let cracks = [CrackInstance {
        block: [0, 64, 0],
        stage: 0,
        shape: CrackShape::Cube,
    }];
    for tick in 0..30 {
        assert_matches_reference(&mut scene, f64::from(tick), &cracks, &submissions);
    }
    assert_eq!(scene.static_rebuilds, 2);
    assert!(!scene.frame.solid.is_empty());
    assert!(!scene.frame.overlay.is_empty());
    assert!(!scene.frame.crack.is_empty());
    assert!(!scene.frame.additive.is_empty());
}

#[test]
fn reordered_removed_and_changed_submissions_rebuild_only_the_affected_slots() {
    let mut scene = scene();
    let mut submissions = vec![chest(0, 1.0), portal(1), chest(2, 0.5)];
    assert_matches_reference(&mut scene, 0.0, &[], &submissions);
    assert_eq!(scene.static_rebuilds, 2);
    submissions.swap(0, 2);
    assert_matches_reference(&mut scene, 1.0, &[], &submissions);
    assert_eq!(scene.static_rebuilds, 4);
    submissions[0].light = 0.75.into();
    assert_matches_reference(&mut scene, 2.0, &[], &submissions);
    assert_eq!(scene.static_rebuilds, 5);
    let BlockEntityKind::Chest(model) = &mut submissions[2].kind else {
        panic!("expected authored chest");
    };
    model.lid = 0.5;
    assert_matches_reference(&mut scene, 3.0, &[], &submissions);
    assert_eq!(scene.static_rebuilds, 6);
    submissions.remove(0);
    assert_matches_reference(&mut scene, 4.0, &[], &submissions);
    assert_eq!(scene.static_rebuilds, 7);
    assert_eq!(scene.cached_submissions.len(), 2);
    assert_matches_reference(&mut scene, 5.0, &[], &[]);
    assert!(scene.cached_submissions.is_empty());
}

#[test]
fn changed_prefix_vertex_counts_invalidate_later_static_fragments() {
    let mut scene = scene();
    let mut submissions = [portal(0), chest(1, 1.0)];
    assert_matches_reference(&mut scene, 0.0, &[], &submissions);
    assert_eq!(scene.static_rebuilds, 1);
    submissions[0].kind = BlockEntityKind::EndGateway;
    assert_matches_reference(&mut scene, 1.0, &[], &submissions);
    // The gateway gets its own static fragment, and the changed prefix rebuilds the chest.
    assert_eq!(scene.static_rebuilds, 3);
    assert_matches_reference(&mut scene, 2.0, &[], &submissions);
    assert_eq!(scene.static_rebuilds, 3);
    submissions[0].kind = BlockEntityKind::EndPortal;
    assert_matches_reference(&mut scene, 3.0, &[], &submissions);
    // Returning to the animated portal rebuilds only the chest's static fragment.
    assert_eq!(scene.static_rebuilds, 4);
}

#[test]
fn dynamic_atlas_updates_keep_static_meshes_and_asset_installs_invalidate_them() {
    let mut scene = scene();
    let sign = BlockEntitySubmission {
        block: [4, 64, 0],
        light: 0.5.into(),
        kind: BlockEntityKind::Sign(SignModel {
            mount: SignMount::Wall(Facing::North),
            front: Some(SignFace {
                rect: scene.text_rect(1, || vec![255; 96 * 48 * 4]).unwrap(),
                glowing: true,
            }),
            back: None,
        }),
    };
    let submissions = [chest(0, 1.0), portal(1), sign];
    assert_matches_reference(&mut scene, 0.0, &[], &submissions);
    let first = scene.frame.clone();
    scene.text_rect(2, || vec![128; 96 * 48 * 4]).unwrap();
    assert_matches_reference(&mut scene, 1.0, &[], &submissions);
    assert_eq!(scene.static_rebuilds, 2);
    assert_ne!(scene.frame.dynamic_revision, first.dynamic_revision);
    assert_ne!(scene.frame.dynamic_rgba8, first.dynamic_rgba8);
    scene.install_assets(&assets_with_chest_offset(384));
    assert!(scene.cached_submissions.is_empty());
    assert_matches_reference(&mut scene, 2.0, &[], &submissions);
    assert_eq!(scene.static_rebuilds, 4);
    assert_ne!(scene.frame.solid, first.solid);
}

#[test]
fn cached_fragments_preserve_vertex_limits_and_rejected_quad_counts() {
    let mut scene = scene();
    let mut submissions: Vec<_> = (0..3700).map(|index| chest(index, 1.0)).collect();
    submissions.insert(0, portal(0));
    for tick in 0..2 {
        assert_matches_reference(&mut scene, f64::from(tick), &[], &submissions);
    }
    assert_eq!(scene.static_rebuilds, 3700);
    assert_eq!(scene.frame.solid.len(), MAX_BLOCK_ENTITY_VERTICES);
    assert!(scene.rejected_quads() > 0);
    submissions.remove(0);
    submissions.push(portal(0));
    assert_matches_reference(&mut scene, 2.0, &[], &submissions);
    assert_eq!(scene.cached_submissions.len(), submissions.len());
    assert_eq!(scene.frame.solid.len(), MAX_BLOCK_ENTITY_VERTICES);
}

#[test]
#[ignore = "benchmark"]
fn frame_cost_bench_block_entity_mixed_scene_400_chests() {
    let submissions: Vec<_> = (0..400)
        .flat_map(|index| {
            let mut group = vec![chest(index, 0.75)];
            if index % 20 == 0 {
                group.push(portal(index));
            }
            group
        })
        .collect();
    let frames = 200;
    let mut old_scene = scene();
    old_scene.update(SceneClock::default(), &[], &submissions);
    let started = std::time::Instant::now();
    for tick in 0..frames {
        let (frame, rejected) = reference_frame(
            &old_scene,
            SceneClock {
                ticks: f64::from(tick),
            },
            &[],
            &submissions,
        );
        old_scene.frame = std::hint::black_box(frame);
        old_scene.rejected_quads = rejected;
    }
    let old = started.elapsed() / frames;
    let mut new_scene = scene();
    new_scene.update(SceneClock::default(), &[], &submissions);
    let started = std::time::Instant::now();
    for tick in 0..frames {
        std::hint::black_box(new_scene.update(
            SceneClock {
                ticks: f64::from(tick),
            },
            &[],
            &submissions,
        ));
    }
    let new = started.elapsed() / frames;
    assert_eq!(new_scene.static_rebuilds, 400);
    eprintln!(
        "FRAME_COST block_entity_mixed_scene_400_chests: old={:.3}ms new={:.3}ms static_builds={}",
        old.as_secs_f64() * 1e3,
        new.as_secs_f64() * 1e3,
        new_scene.static_rebuilds,
    );
}

#[test]
fn review_render_appended_atlas_pixels_change_identity() {
    let mut atlas = BlockEntityAtlas::from_assets(&assets_with_chest_offset(0));
    let original = atlas.identity();
    let texture = super::super::mob::MobTexture {
        name: "test/mob".into(),
        width: 1,
        height: 1,
        rgba8: Arc::from([7; 4]),
    };
    atlas.append_textures(std::slice::from_ref(&texture));
    assert_ne!(atlas.identity(), original);
    let appended = atlas.identity();
    atlas.append_textures(&[texture]);
    assert_eq!(atlas.identity(), appended);
}

#[test]
fn review_render_static_gateway_reuses_geometry_across_ticks() {
    let mut scene = scene();
    let gateway = BlockEntitySubmission {
        block: [0; 3],
        light: 1.0.into(),
        kind: BlockEntityKind::EndGateway,
    };
    scene.update(
        SceneClock { ticks: 1.0 },
        &[],
        std::slice::from_ref(&gateway),
    );
    let revision = scene.frame.revision;
    scene.update(SceneClock { ticks: 2.0 }, &[], &[gateway]);
    assert_eq!(scene.frame.revision, revision);
}

#[test]
fn review_render_atlas_snapshot_does_not_block_mob_installation() {
    let temporary = tempfile::tempdir().unwrap();
    for family in [
        "entity",
        "models/entity",
        "animations",
        "animation_controllers",
        "render_controllers",
        "textures/entity",
    ] {
        std::fs::create_dir_all(temporary.path().join(family)).unwrap();
    }
    std::fs::write(temporary.path().join("models/entity/test.geo.json"), br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.test","texture_width":16,"texture_height":16},"bones":[{"name":"body","cubes":[{"origin":[0,0,0],"size":[1,1,1],"uv":[0,0]}]}]}]}"#).unwrap();
    let compiled = pack_compiler::compile_entity_assets(
        temporary.path(),
        include_bytes!("../../../../../assets/vanilla-source.json"),
    )
    .unwrap();
    let bytes = assets::encode_entity_blob(&compiled).unwrap();
    let entities = assets::RuntimeEntityAssets::decode(&bytes).unwrap();
    let catalog = assets::RuntimeActorCatalog::decode(
        &assets::encode_actor_catalog(&bytes, &[], &[]).unwrap(),
        &bytes,
    )
    .unwrap();
    let mut scene = scene();
    scene.update(SceneClock::default(), &[], &[chest(0, 1.0)]);
    assert!(scene.reusable.is_some());
    let snapshot = Arc::clone(scene.atlas().unwrap());
    scene.install_mob_assets(&entities, &catalog);
    assert!(
        scene.reusable.is_none(),
        "installation must invalidate cached models even with a retained atlas"
    );
    assert!(!Arc::ptr_eq(&snapshot, scene.atlas().unwrap()));
}
