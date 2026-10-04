use protocol::PlayerInputFlags;

use super::PhysicsMovementSample;

/// Previous-tick input lanes used to derive per-family edges between ticks.
///
/// The raw jump and sneak carriers track physical buttons; the processed
/// sneak and sprint lanes track what the simulator acted on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct HeldInput {
    jumping: bool,
    sneak_button: bool,
    sneaking: bool,
    sprinting: bool,
    mode: sim::MovementMode,
}

impl From<&PhysicsMovementSample> for HeldInput {
    fn from(sample: &PhysicsMovementSample) -> Self {
        Self {
            jumping: sample.jumping,
            sneak_button: sample.sneak_button,
            sneaking: sample.processed.sneaking,
            sprinting: sample.processed.sprinting,
            mode: sample.processed.mode,
        }
    }
}

/// Existing direction policy, independent of item/pose control magnitude.
pub(super) fn direction_flags(vector: [f32; 2]) -> PlayerInputFlags {
    let mut flags = PlayerInputFlags::NONE;
    if vector[1] > 0.0 {
        flags |= PlayerInputFlags::UP;
    } else if vector[1] < 0.0 {
        flags |= PlayerInputFlags::DOWN;
    }
    if vector[0] < 0.0 {
        flags |= PlayerInputFlags::LEFT;
    } else if vector[0] > 0.0 {
        flags |= PlayerInputFlags::RIGHT;
    }
    let processed = normalize_move_vector(vector);
    let diagonal = (processed[0].abs() - processed[1].abs()).abs() <= f32::EPSILON * 4.0
        && (processed[0].mul_add(processed[0], processed[1] * processed[1]) - 1.0).abs()
            <= f32::EPSILON * 4.0;
    if diagonal {
        if processed[0] < 0.0 && processed[1] > 0.0 {
            flags |= PlayerInputFlags::UP_LEFT;
        } else if processed[0] > 0.0 && processed[1] > 0.0 {
            flags |= PlayerInputFlags::UP_RIGHT;
        } else if processed[0] < 0.0 && processed[1] < 0.0 {
            flags |= PlayerInputFlags::DOWN_LEFT;
        } else if processed[0] > 0.0 && processed[1] < 0.0 {
            flags |= PlayerInputFlags::DOWN_RIGHT;
        }
    }
    flags
}

pub(super) fn input_flags(sample: &PhysicsMovementSample, previous: HeldInput) -> PlayerInputFlags {
    let mut flags = sample.processed.direction_flags.map_or_else(
        || direction_flags(sample.move_vector),
        |captured| {
            [
                PlayerInputFlags::UP,
                PlayerInputFlags::DOWN,
                PlayerInputFlags::LEFT,
                PlayerInputFlags::RIGHT,
                PlayerInputFlags::UP_LEFT,
                PlayerInputFlags::UP_RIGHT,
                PlayerInputFlags::DOWN_LEFT,
                PlayerInputFlags::DOWN_RIGHT,
            ]
            .into_iter()
            .fold(PlayerInputFlags::NONE, |flags, bit| {
                flags.with_mask(bit, captured.bits() & bit.bits() != 0)
            })
        },
    );

    if sample.horizontal_collision {
        flags |= PlayerInputFlags::HORIZONTAL_COLLISION;
    }
    if sample.vertical_collision {
        flags |= PlayerInputFlags::VERTICAL_COLLISION;
    }

    // Raw jump-button carriers track the physical button exactly. Native
    // 0x07108cc0 also sets processed up; 0x070fcfd0 sends it as WantUp,
    // which the server's 0x0998fe80 reads independently of JumpDown.
    if sample.jumping {
        flags |= PlayerInputFlags::JUMP_DOWN
            | PlayerInputFlags::JUMP_CURRENT_RAW
            | PlayerInputFlags::JUMPING
            | PlayerInputFlags::WANT_UP;
        if !previous.jumping {
            flags |= PlayerInputFlags::JUMP_PRESSED_RAW;
        }
    } else if previous.jumping {
        flags |= PlayerInputFlags::JUMP_RELEASED_RAW;
    }
    // Vanilla maps held jump to Jumping.
    // It maps actual takeoff to StartJumping.
    if sample.processed.jump_initiated {
        flags |= PlayerInputFlags::START_JUMPING;
    }

    // The corresponding processed down lane accompanies held sneak.
    if sample.processed.sneaking {
        flags |=
            PlayerInputFlags::SNEAKING | PlayerInputFlags::SNEAK_DOWN | PlayerInputFlags::WANT_DOWN;
        if !previous.sneaking {
            flags |= PlayerInputFlags::START_SNEAKING;
        }
    } else if previous.sneaking {
        flags |= PlayerInputFlags::STOP_SNEAKING;
    }
    if sample.sneak_button {
        flags |= PlayerInputFlags::SNEAK_CURRENT_RAW;
        if !previous.sneak_button {
            flags |= PlayerInputFlags::SNEAK_PRESSED_RAW;
        }
    } else if previous.sneak_button {
        flags |= PlayerInputFlags::SNEAK_RELEASED_RAW;
    }

    if sample.processed.sprinting {
        flags |= PlayerInputFlags::SPRINT_DOWN | PlayerInputFlags::SPRINTING;
        if !previous.sprinting {
            flags |= PlayerInputFlags::START_SPRINTING;
        }
    } else if previous.sprinting {
        flags |= PlayerInputFlags::STOP_SPRINTING;
    }
    if sample.processed.forced_sneak {
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
