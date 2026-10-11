use super::*;

/// Returns selected-slot remaining use in context units, or in an explicit normalized range.
/// Invalid arguments, absent equipment, and inactive use read zero.
pub(super) fn remaining(evaluator: &QueryInputs<'_>, arguments: &[MolangValue]) -> f32 {
    let (off_hand, range) = match arguments {
        [] => (false, None),
        [MolangValue::Number(range)] => (false, Some(*range)),
        [MolangValue::String(slot)] => match slot.as_ref() {
            "main_hand" => (false, None),
            "off_hand" => (true, None),
            _ => return 0.0,
        },
        [MolangValue::String(slot), MolangValue::Number(range)] => match slot.as_ref() {
            "main_hand" => (false, Some(*range)),
            "off_hand" => (true, Some(*range)),
            _ => return 0.0,
        },
        _ => return 0.0,
    };
    if range.is_some_and(|range| !range.is_finite()) {
        return 0.0;
    }
    let context = evaluator.context;
    let duration = if off_hand {
        context.off_hand_max_use_ticks
    } else {
        context.main_hand_max_use_ticks
    };
    let (elapsed, units) = if let Some(attachable) = context.attachable {
        let Some(elapsed) = attachable.use_elapsed_ticks else {
            return 0.0;
        };
        (elapsed, 1.0)
    } else {
        if evaluator.input.item_use_ticks == 0 && !evaluator.actor.is_using_item() {
            return 0.0;
        }
        (
            evaluator.input.item_use_ticks,
            ACTOR_TICK_DURATION.as_secs_f32(),
        )
    };
    if duration == 0 {
        return 0.0;
    }
    let remaining = duration.saturating_sub(elapsed) as f32;
    match range {
        Some(range) => remaining / duration as f32 * range,
        None => remaining * units,
    }
}
