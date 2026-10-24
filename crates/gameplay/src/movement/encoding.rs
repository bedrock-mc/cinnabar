use protocol::PlayerInputFlags;

use super::PhysicsMovementSample;

/// Previous actor state used only for start/stop actions between ticks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct HeldInput {
    spinning: bool,
    sneaking: bool,
    sprinting: bool,
    mode: sim::MovementMode,
    pose_flags: Option<super::PoseFlags>,
}

impl From<&PhysicsMovementSample> for HeldInput {
    fn from(sample: &PhysicsMovementSample) -> Self {
        Self {
            spinning: sample.processed.spinning,
            sneaking: sample.processed.sneaking,
            sprinting: sample.processed.sprinting,
            mode: sample.processed.mode,
            pose_flags: sample.processed.pose_flags,
        }
    }
}

/// Maps physical digital buttons without inferring them from the movement vector.
pub(super) fn direction_flags(buttons: semantic_input::MovementButtons) -> PlayerInputFlags {
    PlayerInputFlags::NONE
        .with_mask(PlayerInputFlags::UP, buttons.forward)
        .with_mask(PlayerInputFlags::DOWN, buttons.backward)
        .with_mask(PlayerInputFlags::LEFT, buttons.left)
        .with_mask(PlayerInputFlags::RIGHT, buttons.right)
}

/// Encodes independent input requests, raw button edges and resulting actor transitions.
pub(super) fn input_flags(sample: &PhysicsMovementSample, previous: HeldInput) -> PlayerInputFlags {
    let mut flags = direction_flags(sample.input.movement_buttons);
    if sample.processed.spin_started {
        flags |= PlayerInputFlags::START_SPIN_ATTACK;
    }
    // Server flag changes are not actions: compare with the state they left.
    let previous_spinning = sample.input.server_spinning.unwrap_or(previous.spinning);
    if !sample.processed.spinning && (previous_spinning || sample.processed.spin_started) {
        flags |= PlayerInputFlags::STOP_SPIN_ATTACK;
    }

    if sample.horizontal_collision {
        flags |= PlayerInputFlags::HORIZONTAL_COLLISION;
    }
    if sample.vertical_collision {
        flags |= PlayerInputFlags::VERTICAL_COLLISION;
    }

    flags = flags
        .with_mask(PlayerInputFlags::JUMP_DOWN, sample.input.jump.held)
        .with_mask(PlayerInputFlags::JUMP_CURRENT_RAW, sample.input.jump.held)
        .with_mask(
            PlayerInputFlags::JUMP_PRESSED_RAW,
            sample.input.jump.pressed,
        )
        .with_mask(
            PlayerInputFlags::JUMP_RELEASED_RAW,
            sample.input.jump.released,
        )
        .with_mask(PlayerInputFlags::JUMPING, sample.jumping)
        .with_mask(PlayerInputFlags::WANT_UP, sample.jumping);
    // Vanilla maps held jump to Jumping.
    // It maps actual takeoff to StartJumping.
    if sample.processed.jump_initiated {
        flags |= PlayerInputFlags::START_JUMPING;
    }

    flags = flags
        .with_mask(PlayerInputFlags::SNEAKING, sample.input.sneak_down)
        .with_mask(PlayerInputFlags::SNEAK_DOWN, sample.input.sneak_down)
        .with_mask(PlayerInputFlags::WANT_DOWN, sample.input.sneak_down)
        .with_mask(PlayerInputFlags::SNEAK_CURRENT_RAW, sample.input.sneak.held)
        .with_mask(
            PlayerInputFlags::SNEAK_PRESSED_RAW,
            sample.input.sneak.pressed,
        )
        .with_mask(
            PlayerInputFlags::SNEAK_RELEASED_RAW,
            sample.input.sneak.released,
        )
        .with_mask(PlayerInputFlags::SPRINT_DOWN, sample.input.sprint_down)
        .with_mask(PlayerInputFlags::SPRINTING, sample.input.sprint_down);
    // Older queued inputs still need the local transition even if metadata agrees with this tick.
    let sneak_changed = sample.processed.sneaking != previous.sneaking
        || sample
            .input
            .server_sneaking
            .is_some_and(|sneaking| sneaking != sample.processed.sneaking);
    if sneak_changed {
        flags |= if sample.processed.sneaking {
            PlayerInputFlags::START_SNEAKING
        } else {
            PlayerInputFlags::STOP_SNEAKING
        };
    }
    let previous_sprinting = sample.input.server_sprinting.unwrap_or(previous.sprinting);
    if sample.processed.sprinting != previous_sprinting {
        flags |= if sample.processed.sprinting {
            PlayerInputFlags::START_SPRINTING
        } else {
            PlayerInputFlags::STOP_SPRINTING
        };
    }
    if sample.input_mode.persists_sneak() {
        flags |= PlayerInputFlags::PERSIST_SNEAK;
    }
    flags | mode_flags(sample, previous)
}

/// Start/stop edges for each locomotion mode, plus flight's vertical intent.
///
/// Flight edges follow the double-tap request alone: ability updates and ground
/// contact change flight without one, as in vanilla.
fn mode_flags(sample: &PhysicsMovementSample, previous: HeldInput) -> PlayerInputFlags {
    use sim::MovementMode::{Crawling, Flying, Gliding, Swimming};
    let mut flags = PlayerInputFlags::NONE;
    let current = sample.processed.mode;
    let previous_mode = sample.input.server_mode.unwrap_or(previous.mode);
    let current_poses = sample
        .processed
        .pose_flags
        .unwrap_or_else(|| super::PoseFlags::from_mode(current));
    let previous_poses = sample
        .input
        .server_pose_flags
        .or(previous.pose_flags)
        .unwrap_or_else(|| super::PoseFlags::from_mode(previous_mode));
    for (mode, start, stop) in [
        (
            Swimming,
            PlayerInputFlags::START_SWIMMING,
            PlayerInputFlags::STOP_SWIMMING,
        ),
        (
            Gliding,
            PlayerInputFlags::START_GLIDING,
            PlayerInputFlags::STOP_GLIDING,
        ),
        (
            Crawling,
            PlayerInputFlags::START_CRAWLING,
            PlayerInputFlags::STOP_CRAWLING,
        ),
    ] {
        match (current_poses.has(mode), previous_poses.has(mode)) {
            (true, false) => flags |= start,
            (false, true) => flags |= stop,
            _ => {}
        }
    }
    match sample.processed.flight_request {
        Some(true) => flags |= PlayerInputFlags::START_FLYING,
        Some(false) => flags |= PlayerInputFlags::STOP_FLYING,
        None => {}
    }
    if current == sim::MovementMode::Riding && sample.processed.ride == Some(super::RideKind::Boat)
    {
        // Other device modes retain the provisional steering mapping.
        let forward =
            sample.input_mode != protocol::PlayerInputMode::Mouse && sample.move_vector[1] > 0.0;
        if forward || sample.move_vector[0] < 0.0 {
            flags |= PlayerInputFlags::PADDLING_LEFT;
        }
        if forward || sample.move_vector[0] > 0.0 {
            flags |= PlayerInputFlags::PADDLING_RIGHT;
        }
    }
    if current == Flying {
        if sample.jumping {
            flags |= PlayerInputFlags::ASCEND;
        }
        if sample.sneaking {
            flags |= PlayerInputFlags::DESCEND;
        }
    }
    flags
}

/// Converts processed right-positive controls to the wire's left-positive vector.
pub(super) fn wire_move_vector(vector: [f32; 2]) -> [f32; 2] {
    let [right, forward] = semantic_input::clamp_movement_vector(vector);
    [if right == 0.0 { 0.0 } else { -right }, forward]
}
