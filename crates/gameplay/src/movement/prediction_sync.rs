//! Sends `ClientMovementPredictionSync` after the server corrected the local player.

use protocol_ids::{ActorDataKey, ActorFlag, Attribute};
mod countdown;
#[cfg(test)]
mod presentation_tests;

pub(super) use countdown::PredictionSyncCountdown;

use std::collections::HashMap;

use protocol::{ActorMetadataValue, MovementPredictionSync, client_movement_prediction_sync};

use super::LocalPhysicsController;

/// Attribute names in wire order; undefined values are sent as zero.
const ATTRIBUTE_NAMES: [&str; 6] = [
    Attribute::Movement.wire_name(),
    Attribute::UnderwaterMovement.wire_name(),
    Attribute::LavaMovement.wire_name(),
    Attribute::HorseJumpStrength.wire_name(),
    Attribute::Health.wire_name(),
    Attribute::PlayerHunger.wire_name(),
];
/// Optional modifier attributes in wire order, with the value sent while one is undefined.
const MODIFIER_ATTRIBUTES: [(&str, f32); 2] = [
    (Attribute::FrictionModifier.wire_name(), 1.0),
    (Attribute::Bounciness.wire_name(), 0.0),
];
/// Air drag modifier sent while the attribute is undefined.
const UNDEFINED_AIR_DRAG_MODIFIER: f32 = 1.0;

/// Sends a due correction sync and clears its countdown only after admission.
/// `air_drag_modifier` is the session's last accepted finite value, which
/// outlives the replay history that dimension waits and hard corrections clear.
pub fn send_movement_prediction_sync(
    physics: &mut LocalPhysicsController,
    stream: &impl crate::GameplayWorld,
    air_drag_modifier: Option<f32>,
    send: impl FnOnce(protocol::Packet) -> bool,
) {
    if !physics.prediction_sync.due() {
        return;
    }
    let Some(actor) = stream.actor(stream.local_player_runtime_id()) else {
        return;
    };
    let attributes = attributes(
        |name| {
            actor
                .attributes
                .get(name)
                .map(|attribute| attribute.current)
        },
        air_drag_modifier,
    );
    let sync = MovementPredictionSync {
        actor_flags: predicted_flag_words(&actor.metadata, physics.prediction_sync_posture()),
        bounding_box: predicted_bounding_box(
            &actor.metadata,
            physics.prediction_sync_walking_height(),
        ),
        attributes,
        unique_id: stream.local_player_unique_id(),
        flying: physics.mode() == sim::MovementMode::Flying,
    };
    if send(client_movement_prediction_sync(sync)) {
        physics.prediction_sync.clear();
    }
}

/// Overlays current accepted posture while preserving flags owned by server metadata.
fn predicted_flag_words(
    metadata: &HashMap<u32, ActorMetadataValue>,
    posture: Option<(bool, bool)>,
) -> [u64; 3] {
    let mut words = flag_words(metadata);
    if let Some((sneaking, sprinting)) = posture {
        for (flag, enabled) in [
            (ActorFlag::Sneaking, sneaking),
            (ActorFlag::Sprinting, sprinting),
        ] {
            let bit = flag.wire_value() as usize;
            let mask = 1_u64 << (bit % u64::BITS as usize);
            let word = &mut words[bit / u64::BITS as usize];
            *word = (*word & !mask) | if enabled { mask } else { 0 };
        }
    }
    words
}

/// Reads all server-owned flag words, defaulting absent words to zero.
fn flag_words(metadata: &HashMap<u32, ActorMetadataValue>) -> [u64; 3] {
    let word = |key| match metadata.get(&key) {
        Some(ActorMetadataValue::Flags(bits) | ActorMetadataValue::FlagsExtended(bits)) => *bits,
        _ => 0,
    };
    [
        word(ActorDataKey::Flags.wire_value()),
        word(ActorDataKey::FlagsTwo.wire_value()),
        word(ActorDataKey::FlagsThird.wire_value()),
    ]
}

/// Uses current accepted walking size for the standard unscaled player model.
/// Other models and poses retain their supplied metadata until their local contracts are available.
fn predicted_bounding_box(
    metadata: &HashMap<u32, ActorMetadataValue>,
    walking_height: Option<f32>,
) -> [f32; 3] {
    let mut size = bounding_box(metadata);
    if size == [1.0, sim::PLAYER_WIDTH as f32, sim::PLAYER_HEIGHT as f32]
        && let Some(height) = walking_height
    {
        size[2] = height;
    }
    size
}

/// Reads the server-supplied model, with defaults for absent or non-finite fields.
fn bounding_box(metadata: &HashMap<u32, ActorMetadataValue>) -> [f32; 3] {
    let float = |key, default| match metadata.get(&key) {
        Some(ActorMetadataValue::Float(value)) if value.is_finite() => *value,
        _ => default,
    };
    [
        float(ActorDataKey::Scale.wire_value(), 1.0),
        float(ActorDataKey::Width.wire_value(), sim::PLAYER_WIDTH as f32),
        float(ActorDataKey::Height.wire_value(), sim::PLAYER_HEIGHT as f32),
    ]
}

/// `air_drag_modifier` is the accepted value, so a skipped non-finite update
/// never reaches the sync; other modifiers skip it here.
fn attributes(current: impl Fn(&str) -> Option<f32>, air_drag_modifier: Option<f32>) -> [f32; 9] {
    let value = |index: usize| current(ATTRIBUTE_NAMES[index]).unwrap_or(0.0);
    let modifier = |index: usize| {
        let (name, undefined) = MODIFIER_ATTRIBUTES[index];
        current(name)
            .filter(|value| value.is_finite())
            .unwrap_or(undefined)
    };
    [
        value(0),
        value(1),
        value(2),
        value(3),
        value(4),
        value(5),
        modifier(0),
        modifier(1),
        air_drag_modifier.unwrap_or(UNDEFINED_AIR_DRAG_MODIFIER),
    ]
}

#[cfg(test)]
mod tests {
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
        assert_eq!(predicted_bounding_box(&metadata, None), standing);
        assert_eq!(
            predicted_bounding_box(&metadata, Some(crouching)),
            [standing[0], standing[1], crouching]
        );
        for key in [
            ActorDataKey::Scale,
            ActorDataKey::Width,
            ActorDataKey::Height,
        ] {
            let metadata = HashMap::from([(key.wire_value(), ActorMetadataValue::Float(2.0))]);
            assert_eq!(
                predicted_bounding_box(&metadata, Some(crouching)),
                bounding_box(&metadata)
            );
        }
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
        send_movement_prediction_sync(physics, &local_actor(), accepted, |packet| {
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
            send_movement_prediction_sync(&mut physics, &actor, None, |packet| {
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
            let expected_flags =
                unrelated | if sneaking { sneak } else { 0 } | if sprinting { sprint } else { 0 };
            for accepted in [false, true] {
                let mut sent = None;
                send_movement_prediction_sync(&mut physics, &actor, None, |packet| {
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

    /// Live metadata wins over a refused sync's retained sample until another tick completes.
    #[test]
    fn correction_sync_retry_preserves_server_posture_without_a_completed_tick() {
        let sneak = 1_u64 << ActorFlag::Sneaking.wire_value();
        let sprint = 1_u64 << ActorFlag::Sprinting.wire_value();
        let unrelated = 1_u64 << ActorFlag::AlwaysShowName.wire_value();
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
                let frame =
                    physics.advance(std::time::Duration::from_millis(100), input, &OpenWorld);
                assert_eq!(frame.completed_ticks, 2);
                let completed = physics.latest_sneak_sprint().unwrap();
                send_movement_prediction_sync(&mut physics, &actor, None, |_| false);
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
                    send_movement_prediction_sync(&mut physics, &actor, None, |packet| {
                        assert_eq!(
                            packet,
                            client_movement_prediction_sync(MovementPredictionSync {
                                actor_flags: [expected_flags, 0, 0],
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
}
