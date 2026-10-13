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

/// Processes both raw and already-scaled controls entirely in wire precision.
pub(super) fn process(input: MovementInput) -> ProcessedControls {
    let slowed = input.sneaking || input.mode == MovementMode::Crawling;
    let item = input.item_use_movement_modifier.map_or(
        if input.using_consumable {
            super::CONSUMABLE_INPUT_MULTIPLIER as f32
        } else {
            1.0
        },
        |value| value as f32,
    );
    let pose = if slowed {
        sneak_factor(input.swift_sneak)
    } else {
        1.0
    };
    let process_axis = |axis: f64| {
        let axis = axis as f32;
        f64::from(if input.move_vector_is_raw {
            axis.clamp(-1.0, 1.0) * pose * item
        } else {
            let factor = item * pose;
            axis.clamp(-factor, factor)
        })
    };
    ProcessedControls {
        move_vector: [process_axis(input.strafe), process_axis(input.forward)],
    }
}

/// Swift Sneak increases crouch and crawl input up to ordinary walking input.
fn sneak_factor(level: u8) -> f32 {
    (f32::from(level) * 0.15_f32 + super::SNEAK_INPUT_MULTIPLIER as f32).min(1.0)
}
