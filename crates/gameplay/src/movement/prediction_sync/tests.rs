//! Prediction-sync packet construction, scheduling and flag ownership.

use super::*;

#[test]
fn flags_read_all_three_words_and_missing_ones_are_zero() {
    let mut metadata = HashMap::new();
    assert_eq!(flag_words(&metadata), [0; 3]);
    metadata.insert(
        ActorDataKey::Flags.wire_value(),
        ActorMetadataValue::Flags(0b1010),
    );
    metadata.insert(
        ActorDataKey::FlagsTwo.wire_value(),
        ActorMetadataValue::FlagsExtended(1),
    );
    assert_eq!(flag_words(&metadata), [0b1010, 1, 0]);
    metadata.insert(
        ActorDataKey::FlagsThird.wire_value(),
        ActorMetadataValue::FlagsExtended(1),
    );
    assert_eq!(flag_words(&metadata), [0b1010, 1, 1]);
}

#[test]
fn bounding_box_falls_back_to_the_player_box() {
    let mut metadata = HashMap::new();
    assert_eq!(bounding_box(&metadata), [1.0, 0.6, 1.8]);
    metadata.insert(
        ActorDataKey::Height.wire_value(),
        ActorMetadataValue::Float(0.6),
    );
    metadata.insert(
        ActorDataKey::Width.wire_value(),
        ActorMetadataValue::Float(f32::NAN),
    );
    assert_eq!(bounding_box(&metadata), [1.0, 0.6, 0.6]);
}

#[test]
fn local_walking_size_waits_for_a_sample_and_preserves_other_models() {
    let metadata = HashMap::new();
    let standing = [1.0, sim::PLAYER_WIDTH as f32, sim::PLAYER_HEIGHT as f32];
    let crouching = sim::MovementMode::Walking.hitbox_height(true) as f32;
    assert_eq!(predicted_bounding_box(&metadata, None, [0; 3]), standing);
    assert_eq!(
        predicted_bounding_box(&metadata, Some(crouching), [0; 3]),
        [standing[0], standing[1], crouching]
    );
    for key in [ActorDataKey::Width, ActorDataKey::Height] {
        let metadata = HashMap::from([(key.wire_value(), ActorMetadataValue::Float(2.0))]);
        assert_eq!(
            predicted_bounding_box(&metadata, Some(crouching), [0; 3]),
            bounding_box(&metadata)
        );
    }
    // The synced size is unscaled, so a scaled player keeps its pose size.
    let scaled = HashMap::from([(
        ActorDataKey::Scale.wire_value(),
        ActorMetadataValue::Float(2.0),
    )]);
    assert_eq!(
        predicted_bounding_box(&scaled, Some(crouching), [0; 3]),
        [2.0, standing[1], crouching]
    );
}

#[test]
fn undefined_attributes_use_wire_defaults_without_discarding_known_values() {
    assert_eq!(
        attributes(|_| None, None),
        [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0]
    );
    assert_eq!(
        attributes(
            |name| (name != Attribute::PlayerHunger.wire_name()).then_some(0.1),
            None
        ),
        [0.1, 0.1, 0.1, 0.1, 0.1, 0.0, 0.1, 0.1, 1.0]
    );
    let required = |name: &str| ATTRIBUTE_NAMES.contains(&name).then_some(0.1);
    let block = attributes(required, None);
    assert_eq!(&block[..6], &[0.1; 6]);
    assert_eq!(&block[6..], &[1.0, 0.0, 1.0]);
    let block = attributes(|_| Some(0.1), Some(0.1));
    assert_eq!(
        &block[6..],
        &[0.1; 3],
        "defined modifiers send their current"
    );
}

#[test]
fn non_finite_stored_modifiers_never_reach_the_sync() {
    let stored = |name: &str| {
        if ATTRIBUTE_NAMES.contains(&name) {
            Some(0.1)
        } else {
            Some(f32::INFINITY)
        }
    };
    let block = attributes(stored, Some(2.0));
    assert_eq!(
        &block[6..],
        &[1.0, 0.0, 2.0],
        "air drag follows the simulated value; others fall back"
    );
}

/// Exposes one authority snapshot through the gameplay world query boundary.
pub(super) struct LocalActor(pub(super) client_world::ActorSnapshot);

impl crate::GameplayWorld for LocalActor {
    fn canonical_item_stack(
        &self,
        _stack: &protocol::NetworkItemStack,
    ) -> Option<client_world::CanonicalItemStack> {
        None
    }
    fn actor_by_unique_id(&self, _unique_id: i64) -> Option<&client_world::ActorSnapshot> {
        None
    }
    fn actor(&self, runtime_id: u64) -> Option<&client_world::ActorSnapshot> {
        (runtime_id == self.0.runtime_id).then_some(&self.0)
    }
    fn local_player_runtime_id(&self) -> u64 {
        self.0.runtime_id
    }
    fn local_player_unique_id(&self) -> i64 {
        self.0.unique_id
    }
    fn local_rider_seat_pose(&self) -> Option<([f32; 3], f32)> {
        None
    }
    fn network_id_mode(&self) -> assets::NetworkIdMode {
        assets::NetworkIdMode::Sequential
    }
    fn resolve_block_network_id(&self, network_id: u32) -> u32 {
        network_id
    }
    fn air_block_id(&self) -> u32 {
        0
    }
}

/// The local actor with the required attributes and a stored non-finite air drag.
fn local_actor() -> LocalActor {
    use std::sync::Arc;
    let attribute = |name: &str, current: f32| protocol::ActorAttribute {
        name: Arc::from(name),
        min: 0.0,
        max: f32::MAX,
        current,
        default: None,
        modifiers: Arc::from([]),
    };
    let mut attributes: Vec<_> = ATTRIBUTE_NAMES
        .iter()
        .map(|name| attribute(name, 0.1))
        .collect();
    attributes.push(attribute(Attribute::AirDragModifier.wire_name(), f32::NAN));
    let mut authority = client_world::WorldAuthority::new(
        protocol::WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: protocol::air_network_id(false),
            block_network_ids_are_hashes: false,
        },
        Arc::new(assets::RuntimeAssets::diagnostic()),
        None,
        [0.0; 3],
        None,
    );
    let spawn = protocol::ActorSpawnEvent {
        dimension: 0,
        unique_id: 70,
        runtime_id: 7,
        kind: protocol::ActorKind::Entity {
            identifier: "minecraft:pig".into(),
        },
        position: [0.0; 3],
        velocity: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
        body_yaw: 0.0,
        held_item: Default::default(),
        metadata: Arc::from([]),
        attributes: Arc::from(attributes),
        properties: Arc::from([]),
        links: Arc::from([]),
    };
    authority
        .apply_ordered_event(
            protocol::WorldEvent::Actor(protocol::ActorEvent::Spawn(spawn)),
            Some(1),
        )
        .unwrap();
    LocalActor(authority.actor(7).expect("spawn committed").clone())
}

/// Advances the correction countdown without completing a movement sample.
pub(super) fn due_physics() -> LocalPhysicsController {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.5, 72.0, 0.5], 100, false);
    physics.prediction_sync.arm();
    while !physics.prediction_sync.due() {
        physics.prediction_sync.tick();
    }
    physics
}

fn sent_air_drag(physics: &mut LocalPhysicsController, accepted: Option<f32>) -> f32 {
    let mut sent = None;
    send_movement_prediction_sync(physics, &local_actor(), accepted, None, false, |packet| {
        sent = Some(packet);
        true
    });
    protocol::test_support::packets::prediction_sync_attributes(&sent.expect("sync sent"))
        .expect("wrong packet")[8]
}

/// Undefined attributes still produce a due sync, and transport refusal keeps it pending.
#[test]
fn undefined_attributes_publish_defaults_and_retry_until_accepted() {
    let mut actor = local_actor();
    actor.0.attributes.clear();
    let mut physics = due_physics();
    let mut attempts = Vec::new();
    for accepted in [false, true] {
        send_movement_prediction_sync(&mut physics, &actor, None, None, false, |packet| {
            attempts.push(
                protocol::test_support::packets::prediction_sync_attributes(&packet)
                    .expect("wrong packet"),
            );
            accepted
        });
        assert_eq!(physics.prediction_sync.due(), !accepted);
    }
    assert_eq!(
        attempts,
        vec![[0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0]; 2]
    );
}

struct OpenWorld;

impl sim::CollisionWorld for OpenWorld {
    /// Supplies empty terrain so the test runs the ordinary local tick.
    fn collision_boxes(
        &self,
        _query: sim::Aabb,
    ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
        Ok(sim::CollisionQuery::synthetic(Vec::new()))
    }
}

/// Correction sync follows completed walking posture and size before a server echo.
#[test]
fn correction_sync_uses_local_sneak_and_sprint_before_server_echo() {
    use protocol_ids::ActorFlag;
    let sneak = 1_u64 << ActorFlag::Sneaking.wire_value();
    let sprint = 1_u64 << ActorFlag::Sprinting.wire_value();
    let unrelated = 1_u64 << ActorFlag::AlwaysShowName.wire_value();
    // Forward input on collidable terrain also owns the moving and collision flags.
    let walking =
        (1_u64 << ActorFlag::Moving.wire_value()) | (1_u64 << ActorFlag::HasCollision.wire_value());
    for (sneaking, sprinting) in [(false, true), (true, false), (false, false)] {
        let mut actor = local_actor();
        let stale =
            unrelated | if sneaking { 0 } else { sneak } | if sprinting { 0 } else { sprint };
        actor.0.metadata.insert(
            ActorDataKey::Flags.wire_value(),
            ActorMetadataValue::Flags(stale),
        );
        actor.0.metadata.insert(
            ActorDataKey::FlagsTwo.wire_value(),
            ActorMetadataValue::FlagsExtended(1),
        );
        actor.0.metadata.insert(
            ActorDataKey::FlagsThird.wire_value(),
            ActorMetadataValue::FlagsExtended(2),
        );
        let mut physics = due_physics();
        physics.advance(
            std::time::Duration::from_millis(100),
            sim::MovementInput {
                forward: 1.0,
                sneaking,
                sprinting,
                ..Default::default()
            },
            &OpenWorld,
        );
        assert_eq!(physics.latest_sneak_sprint(), Some((sneaking, sprinting)));
        let expected_flags = unrelated
            | walking
            | if sneaking { sneak } else { 0 }
            | if sprinting { sprint } else { 0 };
        for accepted in [false, true] {
            let mut sent = None;
            send_movement_prediction_sync(&mut physics, &actor, None, None, false, |packet| {
                sent = Some(packet);
                accepted
            });
            assert_eq!(
                sent.expect("due correction sync"),
                client_movement_prediction_sync(MovementPredictionSync {
                    actor_flags: [expected_flags, 1, 2],
                    bounding_box: [
                        1.0,
                        sim::PLAYER_WIDTH as f32,
                        sim::MovementMode::Walking.hitbox_height(sneaking) as f32,
                    ],
                    attributes: [0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 1.0, 0.0, 1.0],
                    unique_id: 70,
                    flying: false,
                }),
            );
            assert_eq!(physics.prediction_sync.due(), !accepted);
        }
    }
}

/// Client-owned flags follow local state; every other server bit is kept.
#[test]
fn client_owned_flags_follow_local_state() {
    let words_with = |flags: &[ActorFlag]| {
        let mut words = [0; 3];
        for flag in flags {
            set_flag(&mut words, *flag, true);
        }
        words
    };
    let owned = [
        ActorFlag::Sneaking,
        ActorFlag::Sprinting,
        ActorFlag::UsingItem,
        ActorFlag::Moving,
        ActorFlag::HasCollision,
        ActorFlag::Swimming,
        ActorFlag::Crawling,
        ActorFlag::Gliding,
    ];
    let mut stale = owned.to_vec();
    stale.push(ActorFlag::AlwaysShowName);
    let server = words_with(&stale);
    let metadata = [
        ActorDataKey::Flags,
        ActorDataKey::FlagsTwo,
        ActorDataKey::FlagsThird,
    ]
    .into_iter()
    .zip(server)
    .map(|(key, bits)| (key.wire_value(), ActorMetadataValue::FlagsExtended(bits)))
    .collect::<HashMap<_, _>>();
    let mut without_item = server;
    set_flag(&mut without_item, ActorFlag::UsingItem, false);
    assert_eq!(predicted_flag_words(&metadata, None, false), without_item);

    let idle = SyncPose {
        sneaking: false,
        sprinting: false,
        mode: sim::MovementMode::Walking,
        moving: false,
        has_collision: Some(false),
    };
    assert_eq!(
        predicted_flag_words(&metadata, Some(idle), false),
        words_with(&[ActorFlag::AlwaysShowName])
    );
    for (pose, using_item, flags) in [
        (
            SyncPose {
                sneaking: true,
                sprinting: true,
                moving: true,
                has_collision: Some(true),
                ..idle
            },
            true,
            vec![
                ActorFlag::Sneaking,
                ActorFlag::Sprinting,
                ActorFlag::Moving,
                ActorFlag::HasCollision,
                ActorFlag::UsingItem,
            ],
        ),
        (
            SyncPose {
                mode: sim::MovementMode::Swimming,
                ..idle
            },
            false,
            vec![ActorFlag::Swimming],
        ),
        (
            SyncPose {
                mode: sim::MovementMode::Crawling,
                ..idle
            },
            false,
            vec![ActorFlag::Crawling],
        ),
        (
            SyncPose {
                mode: sim::MovementMode::Gliding,
                ..idle
            },
            false,
            vec![ActorFlag::Gliding],
        ),
        (
            SyncPose {
                has_collision: None,
                ..idle
            },
            false,
            vec![ActorFlag::HasCollision],
        ),
    ] {
        let mut expected = flags;
        expected.push(ActorFlag::AlwaysShowName);
        assert_eq!(
            predicted_flag_words(&metadata, Some(pose), using_item),
            words_with(&expected),
            "{pose:?}"
        );
    }
}

struct DeepWater;

impl sim::CollisionWorld for DeepWater {
    /// Leaves the swimmer unconstrained.
    fn collision_boxes(
        &self,
        _query: sim::Aabb,
    ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
        Ok(sim::CollisionQuery::synthetic(Vec::new()))
    }

    /// Fills every block with still water.
    fn block_physics(
        &self,
        _block: [i32; 3],
    ) -> Result<sim::BlockPhysicsSample, sim::WorldQueryError> {
        Ok(sim::BlockPhysicsSample {
            layers: Box::new([sim::BlockPhysicsFacts {
                friction: 0.6,
                horizontal_speed_factor: 1.0,
                vertical_speed_factor: 1.0,
                fluid_height_blocks: 1.0,
                flags: sim::BlockPhysicsFlags::WATER,
                surface_response: sim::SurfaceResponse::None,
            }]),
            identity: sim::CollisionQuery::synthetic(()).identity,
        })
    }
}

/// A sync that falls due mid-swim reports the swim even when server metadata lacks it.
#[test]
fn correction_sync_reports_a_local_swim_before_server_metadata() {
    let swim = 1_u64 << ActorFlag::Swimming.wire_value();
    let actor = local_actor();
    let mut physics = due_physics();
    let input = sim::MovementInput {
        forward: 1.0,
        sprinting: true,
        ..Default::default()
    };
    for _ in 0..4 {
        physics.advance(std::time::Duration::from_millis(50), input, &DeepWater);
    }
    assert_eq!(physics.mode(), sim::MovementMode::Swimming);
    let mut sent = None;
    send_movement_prediction_sync(&mut physics, &actor, None, None, false, |packet| {
        sent = Some(packet);
        true
    });
    let sent = sent.expect("due correction sync");
    let flags =
        protocol::test_support::packets::prediction_sync_flags(&sent).expect("wrong packet");
    assert_ne!(
        flags[0] & swim,
        0,
        "the sync must carry the local swim pose"
    );
    let size =
        protocol::test_support::packets::prediction_sync_bounding_box(&sent).expect("wrong packet");
    assert_eq!(
        size,
        [1.0, sim::PLAYER_WIDTH as f32, sim::PLAYER_WIDTH as f32]
    );
}

/// Every horizontal pose sends the width as the height, overriding the walking size.
#[test]
fn horizontal_poses_send_the_width_as_the_height() {
    let width = sim::PLAYER_WIDTH as f32;
    let crouching = sim::MovementMode::Walking.hitbox_height(true) as f32;
    let standing = HashMap::new();
    let swimming_echo = HashMap::from([(
        ActorDataKey::Height.wire_value(),
        ActorMetadataValue::Float(width),
    )]);
    for flag in HORIZONTAL_POSE_FLAGS {
        let mut words = [0; 3];
        set_flag(&mut words, flag, true);
        for metadata in [&standing, &swimming_echo] {
            assert_eq!(
                predicted_bounding_box(metadata, Some(crouching), words),
                [1.0, width, width],
                "{flag:?}"
            );
        }
    }
    assert_eq!(
        predicted_bounding_box(&swimming_echo, Some(sim::PLAYER_HEIGHT as f32), [0; 3]),
        [1.0, width, sim::PLAYER_HEIGHT as f32],
        "a completed walking sample replaces a stale horizontal echo"
    );
}

#[test]
fn horizontal_poses_resize_custom_dimensions() {
    for width in [0.4, 0.75, 2.0] {
        for height in [0.6, 1.49, 1.8, 2.5] {
            let metadata = HashMap::from([
                (
                    ActorDataKey::Scale.wire_value(),
                    ActorMetadataValue::Float(2.0),
                ),
                (
                    ActorDataKey::Width.wire_value(),
                    ActorMetadataValue::Float(width),
                ),
                (
                    ActorDataKey::Height.wire_value(),
                    ActorMetadataValue::Float(height),
                ),
            ]);
            for flag in HORIZONTAL_POSE_FLAGS {
                let mut words = [0; 3];
                set_flag(&mut words, flag, true);
                assert_eq!(
                    predicted_bounding_box(&metadata, Some(1.49), words),
                    [2.0, width, width],
                    "{flag:?}: {width} by {height}"
                );
            }
        }
    }
}

#[test]
fn riding_sync_retains_the_effective_flight_ability() {
    use client_world::game_mode_capabilities::FlightAuthority;
    for flying in [false, true] {
        let mut physics = due_physics();
        let frame = physics.advance_with_context(
            std::time::Duration::from_millis(100),
            sim::MovementInput::default(),
            super::super::PhysicsSampleContext {
                mode_intent: super::super::ModeIntent {
                    ride: Some(super::super::RideKind::Other),
                    can_fly: true,
                    flight: FlightAuthority::Fixed(flying),
                    ..Default::default()
                },
                ..Default::default()
            },
            &OpenWorld,
        );
        assert!(frame.completed_ticks > 0);
        assert_eq!(physics.mode(), sim::MovementMode::Riding);
        let mut sent = None;
        send_movement_prediction_sync(&mut physics, &local_actor(), None, None, false, |packet| {
            sent = Some(packet);
            true
        });
        assert_eq!(
            protocol::test_support::packets::prediction_sync_flying(&sent.expect("due sync")),
            Some(flying)
        );
    }
}

#[test]
fn sync_uses_effective_movement_speed_before_the_server_echo() {
    let mut speed = super::super::LocalMovementSpeedAuthority::default();
    speed.begin_session(1, 0);
    assert!(speed.apply(
        1,
        1,
        0,
        client_world::MovementSpeedAttribute::unmodified(0.1)
    ));
    for sprinting in [true, false] {
        speed.set_sprinting(sprinting);
        let mut physics = due_physics();
        let mut sent = None;
        send_movement_prediction_sync(
            &mut physics,
            &local_actor(),
            None,
            speed.current(),
            false,
            |packet| {
                sent = Some(packet);
                true
            },
        );
        let attributes =
            protocol::test_support::packets::prediction_sync_attributes(&sent.expect("due sync"))
                .expect("prediction sync");
        assert_eq!(attributes[0], speed.current().unwrap() as f32);
    }
}

/// Only a replayed correction schedules a sync; a snap (far teleport, respawn) does not.
#[test]
fn only_replayed_corrections_schedule_a_sync() {
    for (mode, scheduled) in [
        (
            crate::movement::PhysicsCorrectionMode::ReplayIfRetained,
            true,
        ),
        (crate::movement::PhysicsCorrectionMode::Snap, false),
    ] {
        let mut physics = LocalPhysicsController::default();
        physics.reanchor_network_position([0.5, 72.0, 0.5], 100, false);
        let frame = physics.advance(
            std::time::Duration::from_millis(100),
            sim::MovementInput::default(),
            &OpenWorld,
        );
        let first = frame.samples.first().expect("completed tick").clone();
        physics
            .apply_correction(
                crate::movement::PhysicsAnchor {
                    network_position: first.position,
                    tick: first.tick,
                    on_ground: false,
                    velocity: Some(first.velocity),
                },
                mode,
                None,
                &OpenWorld,
            )
            .expect("correction applies");
        let mut ticks = 0;
        while !physics.prediction_sync.due() && ticks < 400 {
            physics.prediction_sync.tick();
            ticks += 1;
        }
        assert_eq!(physics.prediction_sync.due(), scheduled, "{mode:?}");
    }
}

/// Live metadata wins over a refused sync's retained sample until another tick completes.
#[test]
fn correction_sync_retry_preserves_server_posture_without_a_completed_tick() {
    let sneak = 1_u64 << ActorFlag::Sneaking.wire_value();
    let sprint = 1_u64 << ActorFlag::Sprinting.wire_value();
    let unrelated = 1_u64 << ActorFlag::AlwaysShowName.wire_value();
    let walking =
        (1_u64 << ActorFlag::Moving.wire_value()) | (1_u64 << ActorFlag::HasCollision.wire_value());
    for (sneaking, sprinting) in [(true, false), (false, true), (true, true), (false, false)] {
        for stamp in [0, 102, 103] {
            let mut actor = local_actor();
            let mut physics = due_physics();
            let input = sim::MovementInput {
                forward: 1.0,
                sneaking,
                sprinting,
                ..Default::default()
            };
            let frame = physics.advance(std::time::Duration::from_millis(100), input, &OpenWorld);
            assert_eq!(frame.completed_ticks, 2);
            let completed = physics.latest_sneak_sprint().unwrap();
            send_movement_prediction_sync(&mut physics, &actor, None, None, false, |_| false);
            assert!(physics.prediction_sync.due());

            let accepted = (!completed.0, !completed.1);
            let expected_flags = unrelated
                | if accepted.0 { sneak } else { 0 }
                | if accepted.1 { sprint } else { 0 };
            actor.0.metadata.insert(
                ActorDataKey::Flags.wire_value(),
                ActorMetadataValue::Flags(expected_flags),
            );
            assert_eq!(
                physics.apply_server_movement_flags(
                    stamp,
                    client_world::MovementFlagUpdate {
                        sneaking: Some(accepted.0),
                        sprinting: Some(accepted.1),
                        ..Default::default()
                    },
                ),
                None,
            );
            // The frame owner consumes these latches even when its frame completes no tick.
            assert!(physics.take_server_control_flags().is_some());
            let frame = physics.advance(std::time::Duration::ZERO, input, &OpenWorld);
            assert_eq!(frame.completed_ticks, 0);
            assert_eq!(physics.latest_sneak_sprint(), Some(completed));

            for admitted in [false, true] {
                send_movement_prediction_sync(&mut physics, &actor, None, None, false, |packet| {
                    assert_eq!(
                        packet,
                        client_movement_prediction_sync(MovementPredictionSync {
                            actor_flags: [expected_flags | walking, 0, 0],
                            bounding_box: [
                                1.0,
                                sim::PLAYER_WIDTH as f32,
                                sim::MovementMode::Walking.hitbox_height(accepted.0) as f32,
                            ],
                            attributes: [0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 1.0, 0.0, 1.0],
                            unique_id: 70,
                            flying: false,
                        }),
                        "server posture at stamp {stamp} must supersede {completed:?}",
                    );
                    admitted
                });
                assert_eq!(physics.prediction_sync.due(), !admitted);
            }
        }
    }
}

/// Sparse server updates compose, then a completed local tick can replace their posture.
#[test]
fn correction_sync_posture_replaces_only_supplied_flags_until_the_next_tick() {
    let mut physics = due_physics();
    let input = sim::MovementInput {
        forward: 1.0,
        sneaking: true,
        ..Default::default()
    };
    physics.advance(std::time::Duration::from_millis(50), input, &OpenWorld);
    physics.apply_server_movement_flags(
        0,
        client_world::MovementFlagUpdate {
            sprinting: Some(true),
            ..Default::default()
        },
    );
    assert_eq!(physics.prediction_sync_posture(), Some((true, true)));
    physics.apply_server_movement_flags(
        0,
        client_world::MovementFlagUpdate {
            sneaking: Some(false),
            ..Default::default()
        },
    );
    assert_eq!(physics.prediction_sync_posture(), Some((false, true)));
    assert_eq!(
        physics.prediction_sync_walking_height(),
        Some(sim::PLAYER_HEIGHT as f32),
    );
    physics.take_server_control_flags();
    let frame = physics.advance(std::time::Duration::from_millis(50), input, &OpenWorld);
    assert_eq!(frame.completed_ticks, 1);
    assert_eq!(physics.prediction_sync_posture(), Some((true, false)));
    assert_eq!(
        physics.prediction_sync_walking_height(),
        Some(sim::MovementMode::Walking.hitbox_height(true) as f32),
    );
}

struct NoEffects;

impl crate::movement::physics::MovementEffectSource for NoEffects {
    fn snapshot(&self) -> sim::MovementEffects {
        sim::MovementEffects::default()
    }
    fn commit_successful_tick(&mut self) {}
}

/// The accepted modifier survives the history a dimension wait clears.
#[test]
fn a_sync_due_during_a_dimension_wait_reports_the_accepted_air_drag() {
    let mut physics = due_physics();
    physics.advance_dimension_wait(
        std::time::Duration::from_millis(50),
        0.0,
        crate::movement::PhysicsSampleContext::default(),
        sim::CollisionRegistry::new().identity(),
        &mut NoEffects,
    );
    assert_eq!(sent_air_drag(&mut physics, Some(2.0)), 2.0);
}

/// The accepted modifier survives the history a hard correction clears.
#[test]
fn a_sync_due_after_a_hard_correction_reports_the_accepted_air_drag() {
    let mut physics = due_physics();
    physics.reanchor_network_position([3.5, 80.0, 3.5], 140, true);
    assert_eq!(sent_air_drag(&mut physics, Some(2.0)), 2.0);
    let mut physics = due_physics();
    assert_eq!(
        sent_air_drag(&mut physics, None),
        1.0,
        "an undefined modifier sends one, never the stored non-finite value"
    );
}
