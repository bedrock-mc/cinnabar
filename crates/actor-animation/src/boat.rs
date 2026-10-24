//! Boat paddle strokes, which vanilla poses natively from the streamed row times.
use super::*;
use assets::facts::entity::NativeEntityClass;
use protocol_ids::ActorDataKey;
use std::f32::consts::{FRAC_PI_3, FRAC_PI_4, PI};

/// Stroke phase in radians per unit of row time.
const PHASE_PER_ROW_TIME: f32 = -20.0;
/// Blade pitch from the top of a stroke to the bottom, radians.
const PITCH: [f32; 2] = [-FRAC_PI_3, -PI / 12.0];
/// Fore-and-aft sweep of the left paddle, radians; the right paddle mirrors it.
const SWEEP: [f32; 2] = [-FRAC_PI_4, FRAC_PI_4];

/// Boats and chest boats share the native paddle model whatever geometry their pack supplies.
pub(super) fn is_boat(actor: &dyn AnimatedActor) -> bool {
    actor.entity_fact().native_class == NativeEntityClass::Boat
}

/// Linear interpolation that holds the bounds outside `[0, 1]`.
fn clamped_lerp([low, high]: [f32; 2], amount: f32) -> f32 {
    if amount < 0.0 {
        low
    } else if amount > 1.0 {
        high
    } else {
        low + amount * (high - low)
    }
}

/// Left-paddle pitch and sweep in radians for one row time; `None` when the phase overflows.
fn stroke(row_time: f32) -> Option<[f32; 2]> {
    let phase = row_time * PHASE_PER_ROW_TIME;
    if !phase.is_finite() {
        return None;
    }
    let swing = |offset: f32| ((phase + offset).sin() + 1.0) * 0.5;
    Some([
        clamped_lerp(PITCH, swing(0.0)),
        clamped_lerp(SWEEP, swing(1.0)),
    ])
}

fn row_time(actor: &dyn AnimatedActor, key: ActorDataKey) -> f32 {
    match actor.metadata().get(&key.wire_value()) {
        Some(ActorMetadataValue::Float(value)) if value.is_finite() => *value,
        _ => 0.0,
    }
}

/// Adds each paddle's stroke to its authored rest rotation, in authored degrees.
pub(super) fn pose_paddles(
    actor: &dyn AnimatedActor,
    names: &[Box<str>],
    local: &mut [pose::LocalDelta],
) {
    for (index, name) in names.iter().enumerate() {
        let (key, sweep_sign) = match name.as_ref() {
            "paddle_left" => (ActorDataKey::RowTimeLeft, 1.0),
            "paddle_right" => (ActorDataKey::RowTimeRight, -1.0),
            _ => continue,
        };
        let Some(delta) = local.get_mut(index) else {
            continue;
        };
        // An overflowing row time leaves the paddle at rest rather than poisoning the pose.
        let Some([pitch, sweep]) = stroke(row_time(actor, key)) else {
            continue;
        };
        delta.rotation[0] += pitch.to_degrees();
        delta.rotation[1] += sweep_sign * sweep.to_degrees();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_actor::TestActor;

    #[test]
    fn paddles_stroke_with_their_own_row_time_and_mirror_their_sweep() {
        let mut actor = TestActor {
            kind: ActorKind::Entity {
                identifier: assets::facts::VanillaEntity::Boat.identifier().into(),
            },
            ..Default::default()
        };
        actor.metadata.insert(
            ActorDataKey::RowTimeLeft.wire_value(),
            ActorMetadataValue::Float(0.1),
        );
        assert!(is_boat(&actor));
        let names: [Box<str>; 3] = ["bottom".into(), "paddle_left".into(), "paddle_right".into()];
        let mut local = [pose::LocalDelta::default(); 3];
        pose_paddles(&actor, &names, &mut local);
        assert_eq!(local[0].rotation, [0.0; 3], "hull parts keep their pose");
        let [pitch, sweep] = stroke(0.1).unwrap().map(f32::to_degrees);
        assert_eq!(local[1].rotation, [pitch, sweep, 0.0]);
        let [rest_pitch, rest_sweep] = stroke(0.0).unwrap().map(f32::to_degrees);
        assert_eq!(local[2].rotation, [rest_pitch, -rest_sweep, 0.0]);
        assert_ne!(local[1].rotation, local[2].rotation);
        for step in 0..200 {
            let [pitch, sweep] = stroke(step as f32 * 0.01).unwrap();
            assert!((PITCH[0]..=PITCH[1]).contains(&pitch));
            assert!((SWEEP[0]..=SWEEP[1]).contains(&sweep));
        }
    }

    /// A finite row time whose phase overflows leaves the paddle at rest with a finite pose.
    #[test]
    fn overflowing_row_times_leave_the_paddle_at_rest() {
        let mut actor = TestActor {
            kind: ActorKind::Entity {
                identifier: assets::facts::VanillaEntity::Boat.identifier().into(),
            },
            ..Default::default()
        };
        actor.metadata.insert(
            ActorDataKey::RowTimeLeft.wire_value(),
            ActorMetadataValue::Float(2e37),
        );
        let names: [Box<str>; 1] = ["paddle_left".into()];
        let mut local = [pose::LocalDelta::default(); 1];
        pose_paddles(&actor, &names, &mut local);
        assert_eq!(local[0].rotation, [0.0; 3]);
    }
}
