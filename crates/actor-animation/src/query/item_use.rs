use super::*;

/// Returns the active main-hand counter in ticks, including attachable use state.
pub(super) fn remaining_ticks(evaluator: &QueryInputs<'_>) -> f32 {
    let duration = evaluator
        .context
        .attachable
        .map_or(evaluator.context.main_hand_max_use_ticks, |attachable| {
            attachable.max_use_ticks
        });
    elapsed_ticks(evaluator).map_or(0, |elapsed| duration.saturating_sub(elapsed)) as f32
}

/// Reads the caller-owned use clock and distinguishes inactive use from its first tick.
fn elapsed_ticks(evaluator: &QueryInputs<'_>) -> Option<u32> {
    if let Some(attachable) = evaluator.context.attachable {
        attachable.use_elapsed_ticks
    } else if let Some(elapsed) = evaluator.context.local_item_use_ticks {
        evaluator.actor.is_using_item().then_some(elapsed)
    } else if evaluator.input.item_use_ticks > 0 || evaluator.actor.is_using_item() {
        Some(evaluator.input.item_use_ticks)
    } else {
        None
    }
}

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
    if elapsed_ticks(evaluator).is_none() {
        return 0.0;
    }
    let units = if context.attachable.is_some() {
        1.0
    } else if arguments.is_empty() {
        ACTOR_TICK_DURATION.as_secs_f32()
    } else {
        ACTOR_TICK_DURATION.as_secs_f32().recip()
    };
    if duration == 0 {
        return 0.0;
    }
    let remaining = remaining_ticks(evaluator);
    match range {
        Some(range) => (remaining / duration as f32).clamp(0.0, 1.0) * range,
        None => remaining * units,
    }
}
