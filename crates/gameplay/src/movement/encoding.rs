use protocol::PlayerInputFlags;

use super::PhysicsMovementSample;

/// Previous actor state used only for start/stop actions between ticks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct HeldInput {
    sneaking: bool,
    sprinting: bool,
    mode: sim::MovementMode,
}

impl From<&PhysicsMovementSample> for HeldInput {
    fn from(sample: &PhysicsMovementSample) -> Self {
        Self {
            sneaking: sample.processed.sneaking,
            sprinting: sample.processed.sprinting,
            mode: sample.processed.mode,
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
    if sample.processed.sneaking != previous.sneaking {
        flags |= if sample.processed.sneaking {
            PlayerInputFlags::START_SNEAKING
        } else {
            PlayerInputFlags::STOP_SNEAKING
        };
    }
    if sample.processed.sprinting != previous.sprinting {
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
fn mode_flags(sample: &PhysicsMovementSample, previous: HeldInput) -> PlayerInputFlags {
    use sim::MovementMode::{Crawling, Flying, Gliding, Swimming};
    let mut flags = PlayerInputFlags::NONE;
    let current = sample.processed.mode;
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
        (
            Flying,
            PlayerInputFlags::START_FLYING,
            PlayerInputFlags::STOP_FLYING,
        ),
    ] {
        match (current == mode, previous.mode == mode) {
            (true, false) => flags |= start,
            (false, true) => flags |= stop,
            _ => {}
        }
    }
    if current == sim::MovementMode::Riding && sample.processed.ride == Some(super::RideKind::Boat)
    {
        // Provisional mapping: a paddle is held on its own side's strafe or on forward input.
        let forward = sample.move_vector[1] > 0.0;
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

pub(super) fn normalize_move_vector(vector: [f32; 2]) -> [f32; 2] {
    let length_squared = vector[0].mul_add(vector[0], vector[1] * vector[1]);
    if length_squared > 1.0 {
        let inverse_length = length_squared.sqrt().recip();
        [vector[0] * inverse_length, vector[1] * inverse_length]
    } else {
        vector
    }
}

/// Converts processed right-positive controls to the wire's left-positive vector.
pub(super) fn wire_move_vector(vector: [f32; 2]) -> [f32; 2] {
    let [right, forward] = normalize_move_vector(vector);
    [if right == 0.0 { 0.0 } else { -right }, forward]
}
