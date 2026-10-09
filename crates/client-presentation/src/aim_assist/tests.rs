mod hitboxes;

use std::sync::Arc;

use bevy::prelude::Vec3;
use protocol::*;

use super::*;

/// Creates the authoritative command shared by ranking fixtures.
fn settings(mode: CameraAimAssistTargetMode) -> CameraAimAssistSettings {
    CameraAimAssistSettings {
        preset_id: Arc::from(""),
        view_angle: [60.0, 60.0],
        distance: 12.0,
        target_mode: mode,
        action: CameraAimAssistAction::Set,
        show_debug_render: false,
    }
}

/// Creates named item/category rules with independently observable fallbacks.
fn registry(replace: bool) -> CameraAimAssistRegistry {
    CameraAimAssistRegistry {
        categories: ["hand", "default", "tool"]
            .map(|name| CameraAimAssistCategory {
                name: Arc::from(name),
                priorities: CameraAimAssistPriorities::default(),
            })
            .into(),
        presets: vec![CameraAimAssistPreset {
            identifier: Arc::from("test:preset"),
            hand_settings: Some(Arc::from("hand")),
            default_item_settings: Some(Arc::from("default")),
            item_settings: vec![CameraAimAssistItemSetting {
                item: Arc::from("minecraft:pickaxe"),
                category: Arc::from("tool"),
            }]
            .into(),
            liquid_targeting_list: vec![Arc::from("minecraft:bucket")].into(),
            ..Default::default()
        }]
        .into(),
        replace,
    }
}

#[test]
fn unknown_preset_preserves_previous_command_and_clear_disables() {
    let mut state = ServerAimAssist::default();
    let mut command = settings(CameraAimAssistTargetMode::Angle);
    state.apply(1, &CameraEvent::AimAssist(command.clone()));
    command.preset_id = Arc::from("missing");
    state.apply(2, &CameraEvent::AimAssist(command.clone()));
    assert_eq!(state.settings().unwrap().preset_id.as_ref(), "");
    assert_eq!(state.semantic_skips(), 1);
    command.action = CameraAimAssistAction::Clear;
    state.apply(3, &CameraEvent::AimAssist(command));
    assert!(state.settings().is_none());
}

#[test]
fn held_item_category_falls_back_independently_from_empty_hand() {
    let mut state = ServerAimAssist::default();
    state.apply(1, &CameraEvent::AimAssistPresets(registry(true)));
    let mut command = settings(CameraAimAssistTargetMode::Angle);
    command.preset_id = Arc::from("test:preset");
    state.apply(2, &CameraEvent::AimAssist(command));
    for (held, expected) in [
        (None, "hand"),
        (Some("minecraft:stick"), "default"),
        (Some("minecraft:pickaxe"), "tool"),
    ] {
        assert_eq!(
            state.category(held).category.unwrap().name.as_ref(),
            expected
        );
    }
    assert!(state.category(Some("minecraft:bucket")).target_liquids);
    assert!(!state.category(None).target_liquids);
}

#[test]
fn add_preserves_unrelated_registry_names_and_set_replaces_them() {
    let mut state = ServerAimAssist::default();
    state.apply(1, &CameraEvent::AimAssistPresets(registry(true)));
    state.apply(
        2,
        &CameraEvent::AimAssistPresets(CameraAimAssistRegistry::default()),
    );
    assert!(state.has_preset("test:preset"));
    state.apply(
        3,
        &CameraEvent::AimAssistPresets(CameraAimAssistRegistry {
            replace: true,
            ..Default::default()
        }),
    );
    assert!(!state.has_preset("test:preset"));
}

#[test]
fn priorities_use_metadata_indices_and_category_zero_exclusion_sentinel() {
    let mut state = ServerAimAssist::default();
    let entries = [
        CameraAimAssistActorPriority {
            preset_index: 7,
            category_index: 4,
            actor_index: 9,
            priority: 81,
        },
        CameraAimAssistActorPriority {
            preset_index: 7,
            category_index: 0,
            actor_index: 10,
            priority: -2,
        },
    ];
    state.apply(1, &CameraEvent::AimAssistActorPriority(Arc::from(entries)));
    assert_eq!(state.actor_priority(7, 4, 9), Some(81));
    assert_eq!(state.actor_priority(7, 5, 9), Some(0));
    assert_eq!(state.actor_priority(7, 4, 10), None);
    assert_eq!(state.player_priority(7, 4, 10), 0);
    assert_eq!(state.player_priority(7, 0, 10), -2);
    state.apply(
        2,
        &CameraEvent::AimAssistActorPriority(Arc::from([CameraAimAssistActorPriority {
            priority: -1,
            ..entries[0]
        }])),
    );
    assert_eq!(state.actor_priority(7, 4, 9), Some(-1));
    assert_eq!(state.actor_priority(7, 4, 10), None);
}

#[test]
fn event_dedup_and_world_replacement_drop_server_policy() {
    let mut state = ServerAimAssist::default();
    state.observe_identity(Some((1, 0)));
    state.apply(
        3,
        &CameraEvent::AimAssist(settings(CameraAimAssistTargetMode::Angle)),
    );
    let mut clear = settings(CameraAimAssistTargetMode::Angle);
    clear.action = CameraAimAssistAction::Clear;
    state.apply(3, &CameraEvent::AimAssist(clear));
    assert!(state.settings().is_some());
    let revision = state.revision();
    state.observe_identity(Some((1, 1)));
    assert!(state.settings().is_some());
    assert_ne!(state.revision(), revision);
    state.observe_identity(Some((2, 0)));
    assert!(state.settings().is_none());
    state.apply(
        1,
        &CameraEvent::AimAssist(settings(CameraAimAssistTargetMode::Distance)),
    );
    assert!(state.settings().is_some());
}

#[test]
fn explicit_blocks_tags_and_exclusions_resolve_without_allocating() {
    let mut source = registry(true);
    let category = Arc::make_mut(&mut source.categories).first_mut().unwrap();
    category.priorities.blocks = vec![CameraAimAssistPriority {
        identifier: Arc::from("stone"),
        priority: 12,
    }]
    .into();
    category.priorities.block_tags = vec![CameraAimAssistPriority {
        identifier: Arc::from("wood"),
        priority: 20,
    }]
    .into();
    category.priorities.block_default = Some(3);
    Arc::make_mut(&mut source.presets)[0].exclusions.blocks = vec![Arc::from("air")].into();
    let category = AimAssistCategory {
        preset: Some(&source.presets[0]),
        category: Some(&source.categories[0]),
        target_liquids: false,
    };
    let tags: [Arc<str>; 1] = [Arc::from("wood")];
    let before = crate::test_allocations::count();
    assert_eq!(category.block_priority("stone", &tags), Some(20));
    assert_eq!(category.block_priority("log", &tags), Some(20));
    assert_eq!(category.block_priority("unknown", &[]), Some(3));
    assert_eq!(category.block_priority("air", &[]), None);
    assert_eq!(crate::test_allocations::count() - before, 0);
}

#[test]
fn weighted_score_golden_values_cover_modes_and_priority_clamps() {
    let direction = Vec3::Z;
    assert_eq!(
        target_score(CameraAimAssistTargetMode::Angle, direction, direction, 0),
        1.311_302_2e-7
    );
    assert_eq!(
        target_score(CameraAimAssistTargetMode::Angle, Vec3::X, direction, 10),
        std::f32::consts::FRAC_PI_2
    );
    assert!(
        (target_score(
            CameraAimAssistTargetMode::Distance,
            direction * 3.0,
            direction,
            0
        ) - 10.89)
            .abs()
            < 1.0e-5
    );
    assert_eq!(
        target_score(
            CameraAimAssistTargetMode::Distance,
            direction * 3.0,
            direction,
            -100
        ),
        target_score(
            CameraAimAssistTargetMode::Distance,
            direction * 3.0,
            direction,
            0
        )
    );
    assert_eq!(
        target_score(
            CameraAimAssistTargetMode::Distance,
            direction * 3.0,
            direction,
            1000
        ),
        target_score(
            CameraAimAssistTargetMode::Distance,
            direction * 3.0,
            direction,
            1001
        )
    );
}

/// Creates an admitted cube with an independently selected center and identity.
fn candidate(id: u64, center: Vec3, priority: i32) -> AimAssistCandidate {
    AimAssistCandidate {
        kind: TargetKind::Actor(id),
        minimum: center - Vec3::splat(0.2),
        maximum: center + Vec3::splat(0.2),
        point: center,
        priority,
        obstructed: false,
    }
}

#[test]
fn nearest_box_point_controls_view_and_distance_admission() {
    let frustum = AimAssistFrustum::new(Vec3::ZERO, Vec3::Z, [30.0, 60.0], 5.0).unwrap();
    assert!(frustum.contains_box(Vec3::new(-0.5, -0.5, 4.9), Vec3::new(0.5, 0.5, 7.0)));
    assert!(!frustum.contains_box(Vec3::new(-0.5, -0.5, 5.0), Vec3::new(0.5, 0.5, 7.0)));
    assert!(!frustum.contains_box(Vec3::new(0.0, 1.0, 2.0), Vec3::new(0.1, 1.1, 2.1)));
    assert!(!frustum.contains_box(Vec3::new(0.0, 0.0, -2.0), Vec3::new(0.1, 0.1, -1.0)));
}

#[test]
fn priority_changes_winner_and_equal_scores_preserve_candidate_order() {
    let frustum = AimAssistFrustum::new(Vec3::ZERO, Vec3::Z, [60.0; 2], 12.0).unwrap();
    let near = candidate(1, Vec3::Z * 2.0, 0);
    let far = candidate(2, Vec3::Z * 5.0, 100);
    assert_eq!(
        frustum
            .select(CameraAimAssistTargetMode::Distance, [near, far])
            .unwrap()
            .kind,
        far.kind
    );
    assert_eq!(
        frustum
            .select(CameraAimAssistTargetMode::Angle, [near, far])
            .unwrap()
            .kind,
        far.kind
    );
    let tie = AimAssistCandidate {
        kind: TargetKind::Actor(3),
        ..near
    };
    assert_eq!(
        frustum
            .select(CameraAimAssistTargetMode::Distance, [near, tie])
            .unwrap()
            .kind,
        near.kind
    );
    let blocked = AimAssistCandidate {
        obstructed: true,
        ..near
    };
    assert_eq!(
        frustum
            .select(CameraAimAssistTargetMode::Distance, [blocked, far])
            .unwrap()
            .kind,
        far.kind
    );
}

#[test]
fn invalid_query_values_are_skipped_without_a_target() {
    for angles in [[f32::NAN, 30.0], [0.0, 30.0], [180.0, 30.0]] {
        assert!(AimAssistFrustum::new(Vec3::ZERO, Vec3::Z, angles, 5.0).is_none());
    }
    assert!(AimAssistFrustum::new(Vec3::ZERO, Vec3::Z, [60.0; 2], -1.0).is_none());
}

#[test]
fn projectile_rotation_depends_on_camera_and_control_scheme() {
    use AimAssistControlScheme::*;
    for scheme in [
        LockedPlayerRelativeStrafe,
        CameraRelative,
        CameraRelativeStrafe,
        PlayerRelative,
        PlayerRelativeStrafe,
    ] {
        assert!(!rotates_player_on_projectile(
            "minecraft:first_person",
            scheme
        ));
        assert!(!rotates_player_on_projectile(
            "minecraft:third_person",
            scheme
        ));
        assert!(rotates_player_on_projectile("minecraft:free", scheme));
        assert!(rotates_player_on_projectile("minecraft:fixed_boom", scheme));
        assert_eq!(
            rotates_player_on_projectile("minecraft:follow_orbit", scheme),
            matches!(scheme, CameraRelative | PlayerRelative)
        );
    }
}

/// Creates a loaded empty region with one visible block and one authoritative actor.
fn world_fixture() -> (
    client_world::WorldAuthority,
    world::ChunkStore,
    sim::CollisionRegistry,
) {
    let mut authority = client_world::WorldAuthority::new(
        WorldBootstrap {
            local_player_unique_id: 1,
            local_player_runtime_id: 1,
            dimension: 0,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(assets::RuntimeAssets::diagnostic()),
        None,
        [0.0; 3],
        None,
    );
    authority
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: 2,
                runtime_id: 2,
                kind: ActorKind::Entity {
                    identifier: Arc::from("minecraft:pig"),
                },
                position: [0.0, 0.0, 3.0],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                body_yaw: 0.0,
                held_item: NetworkItemStack::empty(),
                metadata: Arc::from([]),
                attributes: Arc::from([]),
                properties: Arc::from([]),
                links: Arc::from([]),
            })),
            Some(1),
        )
        .unwrap();
    let mut store = world::ChunkStore::new();
    for x in -2..=2 {
        for z in -2..=2 {
            store
                .mark_chunk_loaded(world::ChunkKey::new(0, x, z))
                .unwrap();
        }
    }
    store
        .update_block(
            world::SubChunkKey::new(0, 0, 0, 0),
            world::BlockUpdate::new(1, 0, 5, 0, 1),
            0,
        )
        .unwrap();
    let mut registry = sim::CollisionRegistry::new();
    registry.register(0, []).unwrap();
    registry
        .register(1, [sim::Aabb::new(sim::Vec3::ZERO, sim::Vec3::ONE)])
        .unwrap();
    (authority, store, registry)
}

#[test]
fn full_evaluation_is_allocation_free_and_publishes_every_second_tick() {
    let (world, store, registry) = world_fixture();
    let blocks = sim::PaletteWorld::new(&store, &registry, 0);
    let mut state = ServerAimAssist::default();
    state.apply(
        1,
        &CameraEvent::AimAssist(settings(CameraAimAssistTargetMode::Distance)),
    );
    let mut frame = AimAssistFrame::default();
    let origin = Vec3::new(0.0, 0.5, 0.0);
    frame.evaluate(
        &state,
        &world,
        &blocks,
        1,
        origin,
        Vec3::Z,
        None,
        None,
        |id| (id == 1).then_some("stone"),
        |_| &[],
    );
    assert!(frame.target.is_none());
    let queries = frame.visibility_queries;
    let skips = frame.skipped_queries;
    frame.evaluate(
        &state,
        &world,
        &blocks,
        1,
        origin,
        Vec3::Z,
        None,
        None,
        |id| (id == 1).then_some("stone"),
        |_| &[],
    );
    assert_eq!(frame.visibility_queries, queries);
    assert_eq!(frame.skipped_queries, skips);
    let before = crate::test_allocations::count();
    for tick in 2..20 {
        frame.evaluate(
            &state,
            &world,
            &blocks,
            tick,
            origin,
            Vec3::Z,
            None,
            None,
            |id| (id == 1).then_some("stone"),
            |_| &[],
        );
    }
    assert_eq!(crate::test_allocations::count() - before, 0);
    assert_eq!(frame.target.unwrap().kind, TargetKind::Actor(2));
    let mut clear = settings(CameraAimAssistTargetMode::Distance);
    clear.action = CameraAimAssistAction::Clear;
    state.apply(2, &CameraEvent::AimAssist(clear));
    frame.synchronize(&state);
    assert!(frame.target.is_none());
}

#[test]
fn interaction_direction_uses_odd_tick_eye_for_retained_target_and_action_rotation() {
    let (world, store, registry) = world_fixture();
    let blocks = sim::PaletteWorld::new(&store, &registry, 0);
    let mut state = ServerAimAssist::default();
    state.apply(
        1,
        &CameraEvent::AimAssist(settings(CameraAimAssistTargetMode::Distance)),
    );
    let mut frame = AimAssistFrame::default();
    let eye = Vec3::new(0.0, 0.5, 0.0);
    for (tick, offset) in [(1, 0.0), (2, 0.25)] {
        frame.evaluate(
            &state,
            &world,
            &blocks,
            tick,
            eye + Vec3::X * offset,
            Vec3::Z,
            None,
            None,
            |id| (id == 1).then_some("stone"),
            |_| &[],
        );
    }
    let target = frame.target.unwrap();
    assert_eq!(
        frame.interaction_direction(),
        (target.point - eye).try_normalize()
    );
    frame.evaluate(
        &state,
        &world,
        &blocks,
        3,
        eye + Vec3::X,
        Vec3::Z,
        None,
        None,
        |id| (id == 1).then_some("stone"),
        |_| &[],
    );
    assert_eq!(frame.target, Some(target));
    let direction = frame.interaction_direction();
    assert_eq!(direction, (target.point - eye - Vec3::X).try_normalize());
    frame.evaluate(
        &state,
        &world,
        &blocks,
        3,
        eye + Vec3::X * 2.0,
        Vec3::Z,
        None,
        None,
        |id| (id == 1).then_some("stone"),
        |_| &[],
    );
    assert_eq!(frame.interaction_direction(), direction);
}

#[test]
fn registry_and_priority_packets_preserve_search_cycle_and_even_tick_uses_latest_weight() {
    let (world, store, registry) = world_fixture();
    let blocks = sim::PaletteWorld::new(&store, &registry, 0);
    let mut state = ServerAimAssist::default();
    state.apply(
        1,
        &CameraEvent::AimAssist(settings(CameraAimAssistTargetMode::Distance)),
    );
    let mut frame = AimAssistFrame::default();
    let eye = Vec3::new(0.0, 0.5, 0.0);
    frame.evaluate(
        &state,
        &world,
        &blocks,
        1,
        eye,
        Vec3::Z,
        None,
        None,
        |id| (id == 1).then_some("stone"),
        |_| &[],
    );
    let revision = state.revision();
    for tick in 2..=6 {
        state.apply(
            tick * 2,
            &CameraEvent::AimAssistActorPriority(Arc::from([CameraAimAssistActorPriority {
                preset_index: 0,
                category_index: 0,
                actor_index: 0,
                priority: 100,
            }])),
        );
        state.apply(
            tick * 2 + 1,
            &CameraEvent::AimAssistPresets(CameraAimAssistRegistry {
                replace: tick % 2 == 0,
                ..Default::default()
            }),
        );
        assert_eq!(state.revision(), revision);
        frame.evaluate(
            &state,
            &world,
            &blocks,
            tick,
            eye,
            Vec3::Z,
            None,
            None,
            |id| (id == 1).then_some("stone"),
            |_| &[],
        );
        let target = frame.target.unwrap();
        assert_eq!(target.kind, TargetKind::Actor(2));
        assert_eq!(
            target.score,
            target_score(
                CameraAimAssistTargetMode::Distance,
                target.point - eye,
                Vec3::Z,
                100
            )
        );
    }
}

#[test]
fn removed_actor_winner_clears_even_phase_result_before_next_candidate_capture() {
    let (mut world, store, registry) = world_fixture();
    let blocks = sim::PaletteWorld::new(&store, &registry, 0);
    let mut state = ServerAimAssist::default();
    state.apply(
        1,
        &CameraEvent::AimAssist(settings(CameraAimAssistTargetMode::Distance)),
    );
    let mut frame = AimAssistFrame::default();
    let eye = Vec3::new(0.0, 0.5, 0.0);
    frame.evaluate(
        &state,
        &world,
        &blocks,
        1,
        eye,
        Vec3::Z,
        None,
        None,
        |id| (id == 1).then_some("stone"),
        |_| &[],
    );
    world
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Remove(ActorRemoveEvent {
                dimension: 0,
                unique_id: 2,
            })),
            Some(2),
        )
        .unwrap();
    frame.evaluate(
        &state,
        &world,
        &blocks,
        2,
        eye,
        Vec3::Z,
        None,
        None,
        |id| (id == 1).then_some("stone"),
        |_| &[],
    );
    assert!(frame.target.is_none());
    assert!(frame.interaction_direction().is_none());
    for tick in 3..=4 {
        frame.evaluate(
            &state,
            &world,
            &blocks,
            tick,
            eye,
            Vec3::Z,
            None,
            None,
            |id| (id == 1).then_some("stone"),
            |_| &[],
        );
    }
    assert!(matches!(
        frame.target.unwrap().kind,
        TargetKind::Block { .. }
    ));
}
