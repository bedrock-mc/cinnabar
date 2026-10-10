use std::{sync::Arc, time::Duration};

use chunk_pipeline::WorldStream;
use client_presentation::{
    camera::{AutoFly, PITCH_LIMIT},
    local_player::LocalViewPose,
};
use protocol::{
    ActorEffectAction, ActorEffectEvent, ActorEvent, ActorKind, ActorSpawnEvent, WorldBootstrap,
    WorldEvent,
};

use super::*;

#[test]
fn production_camera_and_extension_schedule_is_acyclic_without_carriers() {
    let path = std::env::temp_dir().join(format!(
        "cinnabar-gameplay-schedule-{}.wat",
        std::process::id()
    ));
    std::fs::write(
        &path,
        r#"(component
        (core module $m (func (export "init")) (func (export "frame")))
        (core instance $i (instantiate $m))
        (func (export "init") (canon lift (core func $i "init")))
        (func (export "frame") (canon lift (core func $i "frame"))))"#,
    )
    .unwrap();
    let mut app = App::new();
    app.add_plugins(crate::camera::FlyCameraPlugin::default());
    super::super::configure_with_grants(
        &mut app,
        Some(&path),
        ModGrants {
            players: true,
            camera: true,
            ..Default::default()
        },
    );
    std::fs::remove_file(path).unwrap();
    assert!(app.world().contains_resource::<super::super::ModRuntime>());
    let mut schedule = app
        .world_mut()
        .resource_mut::<Schedules>()
        .remove(Update)
        .unwrap();
    schedule.initialize(app.world_mut()).unwrap();
}

#[derive(Resource)]
struct Request {
    allowed: bool,
    grants: ModGrants,
}

#[derive(Resource, Default)]
struct ResultSnapshot(Option<GameplaySnapshot>);

fn capture(context: GameplayContext, request: Res<Request>, mut result: ResMut<ResultSnapshot>) {
    result.0 = context.snapshot(request.allowed, &request.grants);
}

fn stream() -> WorldStream {
    WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        // No block events are submitted; this fixture uses only actor authority.
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    })
}

fn spawn(id: u64, kind: ActorKind, position: [f32; 3]) -> WorldEvent {
    WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
        dimension: 0,
        unique_id: i64::try_from(id).unwrap(),
        runtime_id: id,
        kind,
        position,
        velocity: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
        body_yaw: 0.0,
        held_item: Default::default(),
        metadata: Arc::from([]),
        attributes: Arc::from([]),
        properties: Arc::from([]),
        links: Arc::from([]),
    }))
}

fn player(id: u64, position: [f32; 3]) -> WorldEvent {
    spawn(
        id,
        ActorKind::Player {
            uuid: [0; 16],
            username: format!("fixture-{id}").into(),
        },
        position,
    )
}

fn app() -> App {
    let mut app = App::new();
    app.insert_resource(Request {
        allowed: true,
        grants: ModGrants {
            players: true,
            camera: true,
            ..Default::default()
        },
    })
    .init_resource::<ResultSnapshot>()
    .insert_resource(finalized_input(false))
    .insert_resource(LocalViewPose::new(Vec3::ZERO, Quat::IDENTITY))
    .insert_resource(ClientWorld {
        stream: Some(stream()),
        ..Default::default()
    })
    .add_systems(Update, capture);
    app
}

fn finalized_input(attack_held: bool) -> SemanticInputSnapshot {
    let mut runtime = crate::semantic_controls::SemanticInputRuntime::default();
    let snapshot = runtime
        .route_and_finalize(semantic_input::DeviceFrame {
            keyboard_mouse: Some(semantic_input::KeyboardMouseFrame {
                mouse_buttons: if attack_held { vec![1] } else { Vec::new() },
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    SemanticInputSnapshot::from_finalized(snapshot)
}

#[test]
fn player_state_effects_follow_real_time_while_gameplay_keeps_virtual_delta() {
    #[derive(Resource)]
    struct PlayerFacts {
        player: player_state::PlayerState,
        ui: client_ui::ui_runtime::UiRuntime,
    }
    #[derive(Resource, Default)]
    struct PlayerSnapshot(Option<mod_host::PlayerStateSnapshot>);

    fn capture_player_state(
        context: GameplayContext,
        facts: Res<PlayerFacts>,
        mut result: ResMut<PlayerSnapshot>,
    ) {
        result.0 = context.player_state(true, &facts.player, &facts.ui);
    }

    let session = 7;
    let mut ui = client_ui::ui_runtime::UiRuntime::new(session);
    for (sequence, effect_id, duration_ticks) in [(1, 1, 80), (2, 19, -1)] {
        ui.apply_local_effect(
            session,
            sequence,
            ActorEffectEvent {
                dimension: 0,
                actor_runtime_id: 1,
                action: ActorEffectAction::Add,
                effect_id,
                amplifier: 0,
                particles: true,
                ambient: false,
                duration_ticks,
                tick: 40,
            },
            1_000,
        )
        .unwrap();
    }
    let mut clock = crate::environment::WorldClock::default();
    crate::environment::bind_session_generation(
        &mut clock,
        &mut crate::environment::WeatherState::default(),
        session,
    );
    let mut real_time = Time::<Real>::default();
    real_time.advance_by(Duration::from_secs(2));
    let mut virtual_time: Time = Time::default();
    virtual_time.advance_by(Duration::from_millis(250));
    let mut app = app();
    app.insert_resource(clock)
        .insert_resource(real_time)
        .insert_resource(virtual_time)
        .insert_resource(PlayerFacts {
            player: player_state::PlayerState::new(session),
            ui,
        })
        .init_resource::<PlayerSnapshot>()
        .add_systems(Update, capture_player_state);

    // Long frames advance the effect clock fully even when virtual time stays below its anchor.
    for (real_delta, remaining) in [(0, Some(60)), (2, Some(20)), (1, None)] {
        if real_delta != 0 {
            app.world_mut()
                .resource_mut::<Time<Real>>()
                .advance_by(Duration::from_secs(real_delta));
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(Duration::from_millis(250));
        }
        app.update();
        let snapshot = app.world().resource::<PlayerSnapshot>().0.as_ref().unwrap();
        let finite = snapshot.effects.iter().find(|effect| effect.effect_id == 1);
        assert_eq!(
            finite.map(|effect| effect.remaining_ticks.unwrap()),
            remaining
        );
        let infinite = snapshot
            .effects
            .iter()
            .find(|effect| effect.effect_id == 19);
        assert_eq!(infinite.unwrap().remaining_ticks, None);
        assert_eq!(
            app.world()
                .resource::<ResultSnapshot>()
                .0
                .as_ref()
                .unwrap()
                .frame_seconds,
            0.25
        );
    }
}

#[test]
fn missing_finalized_input_blocks_gameplay_even_with_world_and_camera_authority() {
    let mut app = app();
    app.update();
    assert!(app.world().resource::<ResultSnapshot>().0.is_some());

    app.insert_resource(SemanticInputSnapshot::default());
    app.update();
    assert!(app.world().resource::<ResultSnapshot>().0.is_none());
    app.world_mut().remove_resource::<SemanticInputSnapshot>();
    app.update();
    assert!(app.world().resource::<ResultSnapshot>().0.is_none());

    app.insert_resource(finalized_input(true));
    app.update();
    assert!(
        app.world()
            .resource::<ResultSnapshot>()
            .0
            .as_ref()
            .is_some_and(|snapshot| snapshot.attack_held)
    );
}

#[test]
fn missing_authority_or_gameplay_context_clears_the_snapshot() {
    let mut app = app();
    app.update();
    assert!(app.world().resource::<ResultSnapshot>().0.is_some());

    // This is the adapter's captured-gameplay flag, supplied by focus/menu gating.
    app.world_mut().resource_mut::<Request>().allowed = false;
    app.update();
    assert!(app.world().resource::<ResultSnapshot>().0.is_none());
    app.world_mut().resource_mut::<Request>().allowed = true;
    app.world_mut().resource_mut::<Request>().grants = ModGrants::default();
    app.update();
    assert!(app.world().resource::<ResultSnapshot>().0.is_none());

    app.world_mut().resource_mut::<Request>().grants.players = true;
    app.world_mut().resource_mut::<ClientWorld>().stream = None;
    app.update();
    assert!(app.world().resource::<ResultSnapshot>().0.is_none());
    app.world_mut().resource_mut::<ClientWorld>().stream = Some(stream());
    app.world_mut().remove_resource::<LocalViewPose>();
    app.update();
    assert!(app.world().resource::<ResultSnapshot>().0.is_none());
}

#[test]
fn acceptance_camera_blocks_snapshot_even_while_presentation_is_paused() {
    let mut app = app();
    app.insert_resource(AutoFly::new(true));
    app.update();
    assert!(app.world().resource::<ResultSnapshot>().0.is_none());
    app.world_mut()
        .resource_mut::<AutoFly>()
        .pause_for_stable_presentation();
    assert!(!app.world().resource::<AutoFly>().enabled());
    app.update();
    assert!(app.world().resource::<ResultSnapshot>().0.is_none());
    app.insert_resource(AutoFly::new(false));
    app.update();
    assert!(app.world().resource::<ResultSnapshot>().0.is_some());
}

#[test]
fn protocol_players_are_remote_only_and_grants_do_not_disclose_players() {
    let mut app = app();
    {
        let mut world = app.world_mut().resource_mut::<ClientWorld>();
        let stream = world.stream.as_mut().unwrap();
        stream.submit(1, player(1, [0.0; 3])).unwrap();
        stream.submit(2, player(2, [2.0, 0.0, 0.0])).unwrap();
        stream
            .submit(
                3,
                spawn(
                    3,
                    ActorKind::Entity {
                        identifier: "minecraft:player".into(),
                    },
                    [0.0; 3],
                ),
            )
            .unwrap();
    }
    app.update();
    let snapshot = app.world().resource::<ResultSnapshot>().0.as_ref().unwrap();
    assert_eq!(snapshot.players.len(), 1);
    assert_eq!(snapshot.players[0].runtime_id, 2);
    assert_eq!(snapshot.dimension, 0);
    assert!(!snapshot.attack_held);
    assert_eq!(snapshot.frame_seconds, 0.0);
    app.world_mut().resource_mut::<Request>().grants.players = false;
    app.update();
    assert!(
        app.world()
            .resource::<ResultSnapshot>()
            .0
            .as_ref()
            .unwrap()
            .players
            .is_empty()
    );
}

#[test]
fn nearest_players_are_bounded_and_ties_use_runtime_identity() {
    let mut stream = stream();
    let count = mod_api::MAX_GAMEPLAY_PLAYERS + 7;
    // Reverse the admission order to make ordering independent of the actor map.
    for (index, id) in (2..=count as u64 + 1).rev().enumerate() {
        stream
            .submit(index as u64 + 1, player(id, [1.0, 0.0, 0.0]))
            .unwrap();
    }
    let players = nearest_players(stream.authority().remote_actors(), Vec3::ZERO);
    assert_eq!(players.len(), mod_api::MAX_GAMEPLAY_PLAYERS);
    assert_eq!(players.first().unwrap().runtime_id, 2);
    assert_eq!(
        players.last().unwrap().runtime_id,
        mod_api::MAX_GAMEPLAY_PLAYERS as u64 + 1
    );

    // A newly spawned nearer player must replace the furthest retained tie.
    let near_id = count as u64 + 2;
    stream
        .submit(count as u64 + 1, player(near_id, [0.0; 3]))
        .unwrap();
    let players = nearest_players(stream.authority().remote_actors(), Vec3::ZERO);
    assert_eq!(players.len(), mod_api::MAX_GAMEPLAY_PLAYERS);
    assert_eq!(players[0].runtime_id, near_id);
    assert_eq!(players[1].runtime_id, 2);
}

#[test]
fn malformed_actor_samples_do_not_enter_the_guest_snapshot() {
    let mut stream = stream();
    stream.submit(1, player(2, [1.0, 0.0, 0.0])).unwrap();
    let valid = stream.authority().remote_actors().next().unwrap().clone();
    let mut zero_id = valid.clone();
    zero_id.runtime_id = 0;
    let mut non_finite = valid.clone();
    non_finite.runtime_id = 3;
    non_finite.position[0] = f32::NAN;
    let mut infinite = valid.clone();
    infinite.runtime_id = 4;
    infinite.position[1] = f32::INFINITY;
    let samples = [zero_id, non_finite, infinite, valid];
    let players = nearest_players(samples.iter(), Vec3::ZERO);
    assert_eq!(players.len(), 1);
    assert_eq!(players[0].runtime_id, 2);
}

#[test]
fn camera_delta_wraps_yaw_and_preserves_roll_eye_and_feet() {
    let eye = Vec3::new(3.0, 70.5, -4.0);
    let feet = Vec3::new(3.0, 69.0, -4.0);
    let roll = 0.17;
    let mut view = LocalViewPose::new(
        eye,
        Quat::from_euler(EulerRot::YXZ, std::f32::consts::PI - 0.05, 0.2, roll),
    );
    view.set_subject_position(eye, feet);
    let original = view;
    apply_delta(&mut view, CameraDelta::default());
    assert_eq!(view, original);
    apply_delta(
        &mut view,
        CameraDelta {
            yaw: 0.1,
            pitch: 0.1,
        },
    );
    let (yaw, pitch, actual_roll) = view.rotation().to_euler(EulerRot::YXZ);
    assert!((yaw - (-std::f32::consts::PI + 0.05)).abs() < 1e-5);
    assert!((pitch - 0.3).abs() < 1e-5);
    assert!((actual_roll - roll).abs() < 1e-5);
    assert_eq!(view.eye_translation(), eye);
    assert_eq!(view.feet_translation(), feet);
}

#[test]
fn camera_delta_obeys_both_existing_pitch_limits_through_the_system_param() {
    fn rotate(mut context: GameplayContext, delta: Res<PendingDelta>) {
        context.apply(delta.0);
    }
    #[derive(Resource)]
    struct PendingDelta(CameraDelta);
    let mut app = App::new();
    app.insert_resource(LocalViewPose::new(
        Vec3::ZERO,
        Quat::from_euler(EulerRot::YXZ, 0.0, PITCH_LIMIT - 0.01, 0.0),
    ))
    .insert_resource(PendingDelta(CameraDelta {
        yaw: 0.0,
        pitch: 0.1,
    }))
    .add_systems(Update, rotate);
    app.update();
    let (_, pitch, _) = app
        .world()
        .resource::<LocalViewPose>()
        .rotation()
        .to_euler(EulerRot::YXZ);
    // Euler extraction near the pole has less precision than the quaternion.
    assert!((pitch - PITCH_LIMIT).abs() < 1e-4);
    app.world_mut()
        .resource_mut::<LocalViewPose>()
        .set_rotation(Quat::from_euler(
            EulerRot::YXZ,
            0.0,
            -PITCH_LIMIT + 0.01,
            0.0,
        ));
    app.world_mut().resource_mut::<PendingDelta>().0.pitch = -0.1;
    app.update();
    let (_, pitch, _) = app
        .world()
        .resource::<LocalViewPose>()
        .rotation()
        .to_euler(EulerRot::YXZ);
    assert!((pitch + PITCH_LIMIT).abs() < 1e-4);
}

fn mob(id: u64, identifier: &str, position: [f32; 3]) -> WorldEvent {
    spawn(
        id,
        ActorKind::Entity {
            identifier: identifier.into(),
        },
        position,
    )
}

#[test]
fn nearby_mobs_are_non_players_in_range_nearest_first_with_health() {
    let mut stream = stream();
    stream.submit(1, player(2, [1.0, 0.0, 0.0])).unwrap();
    stream
        .submit(2, mob(3, "cinnabar:hollow_warden", [5.0, 0.0, 0.0]))
        .unwrap();
    stream
        .submit(3, mob(4, "minecraft:zombie", [2.0, 0.0, 0.0]))
        .unwrap();
    let far = mod_api::MAX_MOB_RANGE_BLOCKS + 1.0;
    stream
        .submit(4, mob(5, "minecraft:zombie", [far, 0.0, 0.0]))
        .unwrap();
    let mut actors: Vec<_> = stream.authority().remote_actors().cloned().collect();
    let warden = actors
        .iter_mut()
        .find(|actor| actor.runtime_id == 3)
        .unwrap();
    warden.attributes.insert(
        "minecraft:health".into(),
        protocol::ActorAttribute {
            name: "minecraft:health".into(),
            min: 0.0,
            max: 400.0,
            current: 250.0,
            default: None,
            modifiers: Arc::from([]),
        },
    );
    let mobs = nearest_mobs(actors.iter(), Vec3::ZERO);
    let ids: Vec<_> = mobs.iter().map(|mob| mob.runtime_id).collect();
    assert_eq!(ids, [4, 3]);
    assert_eq!(mobs[1].type_id, "cinnabar:hollow_warden");
    assert_eq!(
        (mobs[1].health, mobs[1].max_health),
        (Some(250.0), Some(400.0))
    );
    assert_eq!(mobs[0].health, None);
}

#[test]
fn mobs_need_the_entities_grant_and_a_current_snapshot() {
    #[derive(Resource, Default)]
    struct Mobs(usize);
    fn capture_mobs(context: GameplayContext, request: Res<Request>, mut mobs: ResMut<Mobs>) {
        let snapshot = context.snapshot(request.allowed, &request.grants);
        mobs.0 = context.mobs(snapshot.as_ref(), &request.grants).len();
    }
    let mut app = app();
    app.init_resource::<Mobs>()
        .add_systems(Update, capture_mobs);
    app.world_mut()
        .resource_mut::<ClientWorld>()
        .stream
        .as_mut()
        .unwrap()
        .submit(1, mob(3, "cinnabar:hollow_warden", [5.0, 0.0, 0.0]))
        .unwrap();
    app.update();
    assert_eq!(app.world().resource::<Mobs>().0, 0);
    app.world_mut().resource_mut::<Request>().grants.entities = true;
    app.update();
    assert_eq!(app.world().resource::<Mobs>().0, 1);
    app.world_mut().resource_mut::<Request>().allowed = false;
    app.update();
    assert_eq!(app.world().resource::<Mobs>().0, 0);
}

#[test]
fn item_use_grant_builds_scoped_context_without_disclosing_players() {
    let mut app = app();
    app.world_mut().resource_mut::<Request>().grants = ModGrants {
        item_use: true,
        ..Default::default()
    };
    app.update();
    let snapshot = app.world().resource::<ResultSnapshot>().0.as_ref().unwrap();
    assert!(snapshot.players.is_empty());
    app.world_mut().resource_mut::<Request>().allowed = false;
    app.update();
    assert!(app.world().resource::<ResultSnapshot>().0.is_none());
}

#[test]
fn nearest_mob_selection_matches_full_order_for_a_large_scrambled_population() {
    let mut stream = stream();
    let count = 2048;
    for index in 0..count {
        let runtime = index as u64 + 2;
        let position = [((index * 17) % 24) as f32, ((index * 7) % 16) as f32, 0.0];
        stream
            .submit(index as u64 + 1, mob(runtime, "minecraft:zombie", position))
            .unwrap();
    }
    let mut expected: Vec<_> = stream.authority().remote_actors().collect();
    expected.sort_by(|a, b| {
        Vec3::from_array(a.position)
            .length_squared()
            .total_cmp(&Vec3::from_array(b.position).length_squared())
            .then(a.runtime_id.cmp(&b.runtime_id))
    });
    expected.truncate(mod_api::MAX_GAMEPLAY_MOBS);
    let expected: Vec<_> = expected.iter().map(|actor| actor.runtime_id).collect();
    let before = crate::tests::alloc_count::thread_allocations();
    let actual = nearest_mobs(stream.authority().remote_actors(), Vec3::ZERO);
    let allocated = crate::tests::alloc_count::thread_allocations() - before;
    assert!(
        allocated <= mod_api::MAX_GAMEPLAY_MOBS as u64 + 3,
        "selected mobs allocated {allocated} times"
    );
    println!("2048 eligible mobs: selected payload allocations={allocated}");
    assert_eq!(
        actual.iter().map(|mob| mob.runtime_id).collect::<Vec<_>>(),
        expected
    );
}
