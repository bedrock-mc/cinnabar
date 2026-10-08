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
        ("textures/environment/end_portal_colors", 352, 4, 4),
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

/// Creates a portal that emits encoded geometry into its dedicated draw layer.
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
            (&scene.heads, &scene.mobs, scene.bed.as_ref()),
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
            portal: builder.portal.into(),
            additive: builder.additive.into(),
            portal_star_rect: super::super::portal::star_rect(atlas),
            portal_time_seconds: (clock.ticks / f64::from(world::TICKS_PER_SECOND)) as f32,
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
    assert_eq!(frame.portal, expected.portal);
    assert_eq!(frame.additive, expected.additive);
    assert_eq!(frame.portal_star_rect, expected.portal_star_rect);
    assert_eq!(frame.portal_time_seconds, expected.portal_time_seconds);
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
    assert_eq!(scene.static_rebuilds, 3);
    assert!(!scene.frame.solid.is_empty());
    assert!(!scene.frame.overlay.is_empty());
    assert!(!scene.frame.crack.is_empty());
    assert!(!scene.frame.portal.is_empty());
}

#[test]
fn reordered_removed_and_changed_submissions_rebuild_only_the_changed_models() {
    let mut scene = scene();
    let mut submissions = vec![chest(0, 1.0), portal(1), chest(2, 0.5)];
    assert_matches_reference(&mut scene, 0.0, &[], &submissions);
    assert_eq!(scene.static_rebuilds, 3);
    // Walking reorders the scan; moved models replay their geometry.
    submissions.swap(0, 2);
    assert_matches_reference(&mut scene, 1.0, &[], &submissions);
    assert_eq!(scene.static_rebuilds, 3);
    submissions[0].light = 0.75.into();
    assert_matches_reference(&mut scene, 2.0, &[], &submissions);
    assert_eq!(scene.static_rebuilds, 4);
    let BlockEntityKind::Chest(model) = &mut submissions[2].kind else {
        panic!("expected authored chest");
    };
    model.lid = 0.5;
    assert_matches_reference(&mut scene, 3.0, &[], &submissions);
    assert_eq!(scene.static_rebuilds, 5);
    submissions.remove(0);
    assert_matches_reference(&mut scene, 4.0, &[], &submissions);
    assert_eq!(scene.static_rebuilds, 5);
    assert_eq!(scene.cached_submissions.len(), 2);
    assert_matches_reference(&mut scene, 5.0, &[], &[]);
    assert!(scene.cached_submissions.is_empty());
}

#[test]
fn changed_prefix_vertex_counts_keep_later_static_fragments_that_still_fit() {
    let mut scene = scene();
    let mut submissions = [portal(0), chest(1, 1.0)];
    assert_matches_reference(&mut scene, 0.0, &[], &submissions);
    assert_eq!(scene.static_rebuilds, 2);
    submissions[0].kind = BlockEntityKind::EndGateway;
    assert_matches_reference(&mut scene, 1.0, &[], &submissions);
    // The gateway builds a new fragment; the chest replays at its new offsets.
    assert_eq!(scene.static_rebuilds, 3);
    assert_matches_reference(&mut scene, 2.0, &[], &submissions);
    assert_eq!(scene.static_rebuilds, 3);
    submissions[0].kind = BlockEntityKind::EndPortal;
    assert_matches_reference(&mut scene, 3.0, &[], &submissions);
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
    assert_eq!(scene.static_rebuilds, 3);
    assert_ne!(scene.frame.dynamic_revision, first.dynamic_revision);
    assert_ne!(scene.frame.dynamic_rgba8, first.dynamic_rgba8);
    scene.install_assets(&assets_with_chest_offset(384));
    assert!(scene.cached_submissions.is_empty());
    assert_matches_reference(&mut scene, 2.0, &[], &submissions);
    assert_eq!(scene.static_rebuilds, 6);
    assert_ne!(scene.frame.solid, first.solid);
}

/// Vanilla shows every sign's text: more distinct texts than one page grow the canvas strip,
/// and an unchanged frame rasterizes and uploads nothing.
#[test]
fn every_distinct_sign_text_keeps_its_own_canvas_and_an_unchanged_frame_rasterizes_nothing() {
    const SIGNS: u64 = 160;
    let canvas = |key: u64| {
        [key as u8, (key >> 8) as u8, 7, 255]
            .into_iter()
            .cycle()
            .take(96 * 48 * 4)
            .collect::<Vec<u8>>()
    };
    let sign = |key: u64, rect: AtlasRect| BlockEntitySubmission {
        block: [key as i32 % 40, 64, key as i32 / 40],
        light: 1.0.into(),
        kind: BlockEntityKind::Sign(SignModel {
            mount: SignMount::Wall(Facing::North),
            front: Some(SignFace {
                rect,
                glowing: false,
            }),
            back: None,
        }),
    };
    let mut scene = scene();
    let rects: Vec<_> = (0..SIGNS)
        .map(|key| {
            scene
                .text_rect(key, || canvas(key))
                .expect("every sign gets a canvas")
        })
        .collect();
    let submissions: Vec<_> = (0..SIGNS)
        .map(|key| sign(key, rects[key as usize]))
        .collect();
    assert_matches_reference(&mut scene, 0.0, &[], &submissions);
    let first = scene.frame.clone();
    let image = first.atlas.as_ref().unwrap();
    assert!(image.size[1] > scene.atlas.as_ref().unwrap().static_height());
    for (key, rect) in rects.iter().enumerate() {
        let row = rect.y as usize - image.static_height as usize;
        let start = (row * image.size[0] as usize + rect.x as usize) * 4;
        assert_eq!(
            &first.dynamic_rgba8[start..start + 4],
            &canvas(key as u64)[..4],
            "sign {key} shows another sign's text"
        );
    }
    let again: Vec<_> = (0..SIGNS)
        .map(|key| scene.text_rect(key, || panic!("an unchanged sign must not rasterize")))
        .collect();
    assert_eq!(
        again.into_iter().map(Option::unwrap).collect::<Vec<_>>(),
        rects
    );
    let second = scene.update(SceneClock { ticks: 1.0 }, &[], &submissions);
    assert_eq!(second.revision, first.revision);
    assert_eq!(second.dynamic_revision, first.dynamic_revision);
}

#[test]
fn cached_fragments_preserve_vertex_limits_and_rejected_quad_counts() {
    let mut scene = scene();
    let mut submissions: Vec<_> = (0..3700).map(|index| chest(index, 1.0)).collect();
    submissions.insert(0, portal(0));
    for tick in 0..2 {
        assert_matches_reference(&mut scene, f64::from(tick), &[], &submissions);
    }
    assert_eq!(scene.static_rebuilds, submissions.len());
    assert_eq!(scene.frame.solid.len(), MAX_BLOCK_ENTITY_VERTICES);
    assert!(scene.rejected_quads() > 0);
    submissions.remove(0);
    submissions.push(portal(0));
    assert_matches_reference(&mut scene, 2.0, &[], &submissions);
    // Portal animation changes uniforms, so its geometry stays cached with the chests.
    assert_eq!(scene.cached_submissions.len(), submissions.len());
    assert_eq!(scene.frame.solid.len(), MAX_BLOCK_ENTITY_VERTICES);
}

#[test]
fn cached_portal_fragment_respects_capacity_after_its_prefix_moves() {
    assert_cached_fragment_capacity(portal(0), |builder| &mut builder.portal, 6);
}

#[test]
fn cached_additive_fragment_respects_capacity_after_its_prefix_moves() {
    assert_cached_fragment_capacity(
        BlockEntitySubmission {
            block: [0; 3],
            light: 1.0.into(),
            kind: BlockEntityKind::DragonDeath(DragonDeathModel {
                center: [0.0; 3],
                death_ticks: world::TICKS_PER_SECOND,
                partial_tick: 0.0,
                seed: 1,
                duration_ticks: (world::TICKS_PER_SECOND * 2) as f32,
            }),
        },
        |builder| &mut builder.additive,
        3,
    );
}

fn assert_cached_fragment_capacity(
    submission: BlockEntitySubmission,
    layer: fn(&mut MeshBuilder) -> &mut Vec<BlockEntityVertex>,
    vertices_per_rejected_quad: u64,
) {
    let scene = scene();
    let atlas = scene.atlas.as_ref().unwrap();
    let emit = |builder: &mut MeshBuilder| {
        emit_submission(
            builder,
            atlas,
            (&scene.heads, &scene.mobs, scene.bed.as_ref()),
            &submission,
            SceneClock::default(),
        );
    };
    let mut original = MeshBuilder::new(atlas.size());
    emit(&mut original);
    let fragment = CachedSubmission::capture(&submission, [0; 5], 0, &original);
    let emitted = layer(&mut original).len();
    assert_ne!(emitted, 0);
    let mut fitting_prefix = MeshBuilder::new(atlas.size());
    layer(&mut fitting_prefix).resize(
        MAX_BLOCK_ENTITY_VERTICES - emitted,
        BlockEntityVertex::default(),
    );
    assert!(fragment.matches(&submission, &fitting_prefix));
    fragment.append_to(&mut fitting_prefix);
    assert_eq!(layer(&mut fitting_prefix).len(), MAX_BLOCK_ENTITY_VERTICES);

    assert!(
        !fragment.matches(&submission, &fitting_prefix),
        "a moved cached fragment cannot bypass its saturated draw-layer budget"
    );
    emit(&mut fitting_prefix);
    assert_eq!(layer(&mut fitting_prefix).len(), MAX_BLOCK_ENTITY_VERTICES);
    assert_eq!(
        fitting_prefix.rejected_quads,
        emitted as u64 / vertices_per_rejected_quad
    );
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
    assert_eq!(new_scene.static_rebuilds, 420);
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
    let first = Arc::clone(&scene.frame.portal);
    let time = scene.frame.portal_time_seconds;
    scene.update(SceneClock { ticks: 2.0 }, &[], &[gateway]);
    assert_eq!(scene.frame.revision, revision);
    assert!(Arc::ptr_eq(&first, &scene.frame.portal));
    assert!(scene.frame.portal_time_seconds > time);
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
        &entities,
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

/// A busy lobby's per-frame scene cost through the installed carrier; prints with
/// `CINNABAR_LOBBY_BENCH=1`.
#[test]
fn lobby_block_entity_frame_cost() {
    use crate::block_entity::{
        banner::{BannerLayer, BannerModel, BannerMount},
        skull::{SkullKind, SkullModel, SkullMount},
    };
    if std::env::var_os("CINNABAR_LOBBY_BENCH").is_none() {
        eprintln!("LOBBY_BLOCK_ENTITIES skipped: set CINNABAR_LOBBY_BENCH=1");
        return;
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.local/assets/compiled/vanilla-v1.mcbeben");
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!("LOBBY_BLOCK_ENTITIES skipped: missing {}", path.display());
        return;
    };
    let assets = assets::RuntimeBlockEntityAssets::decode(&bytes).unwrap();
    for (signs, banners, walking) in [
        (80, 0, false),
        (80, 24, false),
        (160, 24, false),
        (80, 0, true),
    ] {
        let mut scene = BlockEntityScene::default();
        scene.install_assets(&assets);
        let mut samples = Vec::new();
        let mut uploads = 0usize;
        let mut last_revision = 0;
        for frame in 0..=300u32 {
            let started = std::time::Instant::now();
            let mut submissions = Vec::new();
            for index in 0..signs {
                let rect = scene.text_rect(index as u64, || vec![255; 96 * 48 * 4]);
                submissions.push(BlockEntitySubmission {
                    block: [index % 40, 65, index / 40],
                    light: 1.0.into(),
                    kind: BlockEntityKind::Sign(SignModel {
                        mount: SignMount::Wall(Facing::South),
                        front: rect.map(|rect| SignFace {
                            rect,
                            glowing: false,
                        }),
                        back: None,
                    }),
                });
            }
            for index in 0..60 {
                submissions.push(BlockEntitySubmission {
                    block: [index % 30, 66, 10 + index / 30],
                    light: 1.0.into(),
                    kind: BlockEntityKind::Skull(SkullModel {
                        kind: if index % 3 == 0 {
                            SkullKind::Skeleton
                        } else {
                            SkullKind::Player
                        },
                        mount: SkullMount::Floor {
                            rotation_degrees: 0.0,
                        },
                    }),
                });
            }
            for index in 0..40 {
                submissions.push(chest(index + 400, 1.0));
            }
            for index in 0..banners {
                submissions.push(BlockEntitySubmission {
                    block: [index, 67, 20],
                    light: 1.0.into(),
                    kind: BlockEntityKind::Banner(BannerModel {
                        mount: BannerMount::Wall(Facing::North),
                        base: [0.2, 0.3, 0.8],
                        layers: vec![
                            BannerLayer {
                                pattern: "border",
                                color: [1.0; 3],
                            },
                            BannerLayer {
                                pattern: "stripe_bottom",
                                color: [0.9, 0.1, 0.1],
                            },
                        ],
                    }),
                });
            }
            if walking {
                // Walking brings one entity into range at the front of the scan every few frames.
                let entering = (frame / 4) as usize % submissions.len();
                submissions.rotate_left(entering);
            }
            let clock = SceneClock {
                ticks: f64::from(frame) / 3.0,
            };
            let revision = scene.update(clock, &[], &submissions).revision;
            if frame > 0 {
                samples.push(started.elapsed());
                uploads += usize::from(revision != last_revision);
            }
            last_revision = revision;
        }
        samples.sort_unstable();
        let mean = samples.iter().sum::<std::time::Duration>() / samples.len() as u32;
        eprintln!(
            "LOBBY_BLOCK_ENTITIES signs={signs} banners={banners} walking={walking} solid_vertices={} mean_ms={:.3} p99_ms={:.3} rebuilt_frames={uploads}/{}",
            scene.frame.solid.len(),
            mean.as_secs_f64() * 1e3,
            samples[(samples.len() - 1) * 99 / 100].as_secs_f64() * 1e3,
            samples.len()
        );
    }
}
