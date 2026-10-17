//! Sends `ClientMovementPredictionSync` after the server corrected the local player.

use protocol_ids::{ActorDataKey, Attribute};
mod countdown;

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
        actor_flags: flag_words(&actor.metadata),
        bounding_box: bounding_box(&actor.metadata),
        attributes,
        unique_id: stream.local_player_unique_id(),
        flying: physics.mode() == sim::MovementMode::Flying,
    };
    if send(client_movement_prediction_sync(sync)) {
        physics.prediction_sync.clear();
    }
}

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

    struct LocalActor(client_world::ActorSnapshot);

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

    fn due_physics() -> LocalPhysicsController {
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
