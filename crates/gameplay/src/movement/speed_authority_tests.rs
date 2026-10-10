use super::{LocalMovementSpeedAuthority, preserve_effective_speed};
use client_world::MovementSpeedAttribute;

/// Builds a movement packet with explicit current, default and sprint authority.
pub(crate) fn attribute(current: f64, default: f32, factor: Option<f32>) -> MovementSpeedAttribute {
    let modifiers = factor
        .map(|factor| protocol::ActorAttributeModifier {
            id: std::sync::Arc::from(client_world::SPRINT_SPEED_MODIFIER_ID),
            name: std::sync::Arc::from("sprint"),
            amount: factor - 1.0,
            operation: 2,
            operand: 2,
            serializable: false,
        })
        .into_iter()
        .collect::<Vec<_>>();
    let mut attribute = MovementSpeedAttribute::from_attribute(&protocol::ActorAttribute {
        name: std::sync::Arc::from("minecraft:movement"),
        min: 0.0,
        max: f32::MAX,
        current: default,
        default: Some(default),
        modifiers: modifiers.into(),
    })
    .unwrap();
    attribute.current = current;
    attribute
}

/// Starts one authority with an admitted server attribute update.
fn started(current: f32) -> LocalMovementSpeedAuthority {
    let mut authority = LocalMovementSpeedAuthority::default();
    authority.begin_session(7, 0);
    assert!(authority.apply(7, 1, 0, attribute(f64::from(current), current, None)));
    authority
}

#[test]
fn server_effective_sprint_without_modifiers_is_never_boosted_twice() {
    let mut authority = started(0.1);
    authority.set_sprinting(true);
    assert!(authority.apply(7, 2, 0, attribute(f64::from(0.13_f32), 0.1, None)));
    authority.adopt_server_sprinting(Some(true));
    for _ in 0..20 {
        authority.set_sprinting(true);
    }
    assert_eq!(authority.current(), Some(f64::from(0.13_f32)));
    assert_eq!(
        authority.prediction_speed(),
        Some(f64::from(0.13_f32 / sim::SPRINT_SPEED_MULTIPLIER as f32))
    );

    // The packet removed the local modifier: native sprint stop cannot remove it again.
    authority.set_sprinting(false);
    assert_eq!(authority.current(), Some(f64::from(0.13_f32)));
    assert!(authority.apply(7, 3, 0, attribute(f64::from(0.1_f32), 0.1, None)));
    assert_eq!(authority.prediction_speed(), Some(f64::from(0.1_f32)));
}

/// Restarting sprint after an effective-speed resend must not stack another boost.
#[test]
fn sprint_restart_after_attribute_resend_does_not_compound_speed() {
    let mut authority = started(0.1);
    authority.set_sprinting(true);
    assert!(authority.apply(7, 2, 0, attribute(f64::from(0.13_f32), 0.1, None)));
    authority.set_sprinting(false);
    assert_eq!(authority.current(), Some(f64::from(0.13_f32)));
    authority.set_sprinting(true);
    assert_eq!(
        authority.current(),
        Some(f64::from(0.1_f32 * sim::SPRINT_SPEED_MULTIPLIER as f32))
    );
}

#[test]
fn local_edges_preserve_custom_speed_and_remove_only_installed_modifier() {
    let mut authority = started(0.12);
    let base = authority.current().unwrap() as f32;
    authority.set_sprinting(true);
    let boosted = base * sim::SPRINT_SPEED_MULTIPLIER as f32;
    assert_eq!(authority.current(), Some(f64::from(boosted)));
    authority.set_sprinting(true);
    assert_eq!(authority.current(), Some(f64::from(boosted)));
    authority.set_sprinting(false);
    assert_eq!(
        authority.current(),
        Some(f64::from(boosted / sim::SPRINT_SPEED_MULTIPLIER as f32))
    );

    authority.adopt_server_sprinting(Some(true));
    assert!(authority.apply(7, 2, 0, attribute(f64::from(0.18_f32), 0.12, Some(1.5))));
    authority.set_sprinting(false);
    assert_eq!(authority.current(), Some(f64::from(0.18_f32 / 1.5)));
}

#[test]
fn metadata_adopts_sprint_without_modifying_attribute() {
    let mut authority = started(0.12);
    authority.adopt_server_sprinting(Some(true));
    authority.set_sprinting(true);
    assert_eq!(authority.current(), Some(f64::from(0.12_f32)));
    authority.adopt_server_sprinting(Some(false));
    authority.set_sprinting(false);
    assert_eq!(authority.current(), Some(f64::from(0.12_f32)));
}

#[test]
fn authority_obeys_session_fifo_dimension_and_replacement_ordering() {
    let mut authority = started(0.25);
    assert!(!authority.apply(6, 3, 0, attribute(0.5, 0.5, None)));
    assert!(!authority.apply(7, 1, 0, attribute(0.5, 0.5, None)));
    assert!(!authority.apply(7, 3, 1, attribute(0.5, 0.5, None)));
    assert_eq!(authority.current(), Some(0.25));
    authority.set_sprinting(true);
    authority.replace_dimension(7, 1);
    assert_eq!(authority.current(), None);
    assert!(!authority.apply(7, 1, 0, attribute(0.75, 0.75, None)));
    assert!(authority.apply(7, 1, 1, attribute(0.0, 0.0, None)));
    authority.set_sprinting(false);
    assert_eq!(authority.current(), Some(0.0));
    authority.begin_session(8, -1);
    assert_eq!(authority.current(), None);
    assert!(!authority.apply(7, 2, -1, attribute(1.0, 1.0, None)));
    assert!(authority.apply(8, 1, -1, attribute(0.1, 0.1, None)));
}

#[test]
fn invalid_updates_are_consumed_without_overwriting_last_valid_authority() {
    let mut authority = started(0.2);
    for (sequence, value) in [(2, f64::NAN), (3, f64::INFINITY), (4, -0.1), (5, 1.0e6)] {
        assert!(!authority.apply(7, sequence, 0, attribute(value, 0.1, None)));
        assert_eq!(authority.current(), Some(f64::from(0.2_f32)));
    }
    assert!(!authority.apply(7, 5, 0, attribute(0.9, 0.9, None)));
    for (sequence, factor) in [
        (6, 0.0),
        (7, f32::NAN),
        (8, f32::INFINITY),
        (9, 1.0 + (-0.999_999_94_f32)),
    ] {
        let mut invalid = attribute(0.3, 0.1, None);
        invalid.sprint_modifier = Some(factor);
        assert!(!authority.apply(7, sequence, 0, invalid));
        assert_eq!(authority.current(), Some(f64::from(0.2_f32)));
    }
}

#[test]
fn processed_sprint_rewrites_keep_the_effective_attribute() {
    let mut input = sim::MovementInput {
        sprinting: true,
        movement_speed: Some(f64::from(0.13_f32 / sim::SPRINT_SPEED_MULTIPLIER as f32)),
        ..sim::MovementInput::default()
    };
    input.sprinting = false;
    preserve_effective_speed(&mut input, true);
    assert_eq!(input.movement_speed, Some(f64::from(0.13_f32)));
    input.sprinting = true;
    preserve_effective_speed(&mut input, false);
    assert_eq!(
        input.movement_speed,
        Some(f64::from(0.13_f32 / sim::SPRINT_SPEED_MULTIPLIER as f32))
    );
}

/// Liquid speeds arrive in the same attribute packet as movement and keep values a later update omits.
#[test]
fn liquid_speeds_order_independently_and_keep_omitted_values() {
    let mut authority = LocalMovementSpeedAuthority::default();
    authority.begin_session(1, 0);
    assert!(authority.apply(1, 5, 0, attribute(0.1, 0.1, None)));
    let update = authority.apply_liquid(1, 5, 0, Some(0.05), None).unwrap();
    assert_eq!(update.underwater, Some(0.05));
    authority.apply_liquid(1, 6, 0, None, Some(0.03)).unwrap();
    assert_eq!(
        authority.liquid(),
        super::LiquidMovementSpeeds {
            underwater: Some(0.05),
            lava: Some(0.03),
        }
    );
    assert!(authority.apply_liquid(1, 6, 0, Some(0.2), None).is_none());
    let invalid = authority
        .apply_liquid(1, 7, 0, Some(f64::NAN), None)
        .unwrap();
    assert_eq!(invalid.underwater, None);
    assert_eq!(authority.liquid().underwater, Some(0.05));
}

/// Liquid speeds beyond the sweep-safe bounds are skipped; the largest admitted
/// speeds keep worst-case liquid travel inside every simulator query budget.
#[test]
fn liquid_speed_admission_keeps_liquid_travel_simulable() {
    let mut authority = LocalMovementSpeedAuthority::default();
    authority.begin_session(1, 0);
    let skipped = authority
        .apply_liquid(1, 1, 0, Some(30.0), Some(30.0))
        .unwrap();
    assert_eq!((skipped.underwater, skipped.lava), (None, None));
    assert_eq!(authority.liquid(), super::LiquidMovementSpeeds::default());
    let underwater = super::MAX_SIMULABLE_UNDERWATER_SPEED;
    let lava = super::MAX_SIMULABLE_LAVA_SPEED;
    let admitted = authority
        .apply_liquid(1, 2, 0, Some(underwater), Some(lava))
        .unwrap();
    assert_eq!(
        (admitted.underwater, admitted.lava),
        (Some(underwater), Some(lava))
    );

    struct Liquid(sim::BlockPhysicsFlags);
    impl sim::CollisionWorld for Liquid {
        fn collision_boxes(
            &self,
            _: sim::Aabb,
        ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
            Ok(sim::CollisionQuery::synthetic(Vec::new()))
        }
        fn block_physics(
            &self,
            _: [i32; 3],
        ) -> Result<sim::BlockPhysicsSample, sim::WorldQueryError> {
            Ok(sim::BlockPhysicsSample {
                layers: Box::new([sim::BlockPhysicsFacts {
                    friction: 0.6,
                    horizontal_speed_factor: 1.0,
                    vertical_speed_factor: 1.0,
                    fluid_height_blocks: 1.0,
                    flags: self.0,
                    surface_response: sim::SurfaceResponse::None,
                }]),
                identity: sim::CollisionQuery::synthetic(()).identity,
            })
        }
    }
    let boosted = sim::MovementEffects {
        dolphin_boost: true,
        ..sim::MovementEffects::default()
    };
    // Diagonal sprint-swimming and lava wading, diving, level and climbing.
    for (flags, mode, effects) in [
        (
            sim::BlockPhysicsFlags::WATER,
            sim::MovementMode::Swimming,
            boosted,
        ),
        (
            sim::BlockPhysicsFlags::LAVA,
            sim::MovementMode::Walking,
            sim::MovementEffects::default(),
        ),
    ] {
        for pitch in [-90.0, 0.0, 90.0] {
            let mut state = sim::PlayerState::new(sim::Vec3::new(0.5, 5.0, 0.5));
            state.swim_amount = 1.0;
            state.swim_pose_active = mode == sim::MovementMode::Swimming;
            let input = sim::MovementInput {
                mode,
                forward: 1.0,
                strafe: 1.0,
                yaw_degrees: 45.0,
                pitch_degrees: pitch,
                sprinting: true,
                liquid_contact_height: Some(f64::from(sim::PLAYER_HEIGHT as f32)),
                underwater_movement_speed: Some(underwater),
                lava_movement_speed: Some(lava),
                effects,
                ..sim::MovementInput::default()
            };
            for tick in 0..300 {
                sim::Simulator::default()
                    .tick(&mut state, input, &Liquid(flags))
                    .unwrap_or_else(|error| panic!("{mode:?} pitch {pitch} tick {tick}: {error}"));
            }
        }
    }
}
