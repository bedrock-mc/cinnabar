use protocol::PlayerInputFlags;

use super::PhysicsMovementSample;

/// Held jump/sneak/sprint state used to derive edge flags between ticks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct HeldInput {
    jumping: bool,
    sneaking: bool,
    sprinting: bool,
}

impl From<&PhysicsMovementSample> for HeldInput {
    fn from(sample: &PhysicsMovementSample) -> Self {
        Self {
            jumping: sample.jumping,
            sneaking: sample.sneaking,
            sprinting: sample.sprinting,
        }
    }
}

pub(super) fn input_flags(sample: &PhysicsMovementSample, previous: HeldInput) -> PlayerInputFlags {
    let mut flags = PlayerInputFlags::NONE;
    if sample.move_vector[1] > 0.0 {
        flags |= PlayerInputFlags::UP;
    } else if sample.move_vector[1] < 0.0 {
        flags |= PlayerInputFlags::DOWN;
    }
    if sample.move_vector[0] < 0.0 {
        flags |= PlayerInputFlags::LEFT;
    } else if sample.move_vector[0] > 0.0 {
        flags |= PlayerInputFlags::RIGHT;
    }
    let processed = normalize_move_vector(sample.move_vector);
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

    if sample.horizontal_collision {
        flags |= PlayerInputFlags::HORIZONTAL_COLLISION;
    }
    if sample.vertical_collision {
        flags |= PlayerInputFlags::VERTICAL_COLLISION;
    }

    if sample.jumping {
        flags |= PlayerInputFlags::JUMP_DOWN
            | PlayerInputFlags::JUMPING
            | PlayerInputFlags::JUMP_CURRENT_RAW;
        if !previous.jumping {
            flags |= PlayerInputFlags::START_JUMPING | PlayerInputFlags::JUMP_PRESSED_RAW;
        }
    } else if previous.jumping {
        flags |= PlayerInputFlags::JUMP_RELEASED_RAW;
    }

    if sample.sneaking {
        flags |= PlayerInputFlags::SNEAKING | PlayerInputFlags::SNEAK_DOWN;
        if !previous.sneaking {
            flags |= PlayerInputFlags::START_SNEAKING | PlayerInputFlags::SNEAK_PRESSED_RAW;
        }
    } else if previous.sneaking {
        flags |= PlayerInputFlags::STOP_SNEAKING | PlayerInputFlags::SNEAK_RELEASED_RAW;
    }

    if sample.sprinting {
        flags |= PlayerInputFlags::SPRINT_DOWN | PlayerInputFlags::SPRINTING;
        if !previous.sprinting {
            flags |= PlayerInputFlags::START_SPRINTING;
        }
    } else if previous.sprinting {
        flags |= PlayerInputFlags::STOP_SPRINTING;
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
