use super::{MovementInput, MovementMode, TickResult};

/// Fixed-tick primary controls, after item/pose slowdown but before the
/// simulation's movement impulse. Axes are left-positive strafe and forward.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProcessedControls {
    pub move_vector: [f64; 2],
}

/// A completed simulation and the exact controls used to produce it.
/// Keeping this separate preserves the historical trace/result schema.
#[derive(Debug, Clone, PartialEq)]
pub struct ControlledTickResult {
    pub tick_result: TickResult,
    pub controls: ProcessedControls,
    /// Whether this tick consumed a ground-jump request.
    pub jump_initiated: bool,
}

pub(super) fn process(input: MovementInput) -> ProcessedControls {
    let slowed = input.sneaking || input.mode == MovementMode::Crawling;
    if input.move_vector_is_raw {
        // Primary controls are a wire-f32 contract: round operands and each
        // item/pose multiplication before widening into existing f64 travel.
        let item = input.item_use_movement_modifier.map_or(
            if input.using_consumable {
                super::CONSUMABLE_INPUT_MULTIPLIER as f32
            } else {
                1.0
            },
            |value| value as f32,
        );
        let factor = item
            * if slowed {
                sneak_factor(input.swift_sneak)
            } else {
                1.0
            };
        return ProcessedControls {
            move_vector: [
                f64::from((input.strafe as f32).clamp(-1.0, 1.0) * factor),
                f64::from((input.forward as f32).clamp(-1.0, 1.0) * factor),
            ],
        };
    }
    let item = input
        .item_use_movement_modifier
        .unwrap_or(if input.using_consumable {
            super::CONSUMABLE_INPUT_MULTIPLIER
        } else {
            1.0
        });
    let factor = item
        * if slowed {
            f64::from(sneak_factor(input.swift_sneak))
        } else {
            1.0
        };
    let process_axis = |axis: f64| axis.clamp(-factor, factor);
    ProcessedControls {
        move_vector: [process_axis(input.strafe), process_axis(input.forward)],
    }
}

/// Swift Sneak increases crouch and crawl input up to ordinary walking input.
fn sneak_factor(level: u8) -> f32 {
    (f32::from(level) * 0.15_f32 + super::SNEAK_INPUT_MULTIPLIER as f32).min(1.0)
}
