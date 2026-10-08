use protocol::ActorAttribute;

const SPRINT_MODIFIER_ID: &str = "d208fc00-42aa-4aad-9276-d5446530de43";
pub const AIR_DRAG_MODIFIER_ATTRIBUTE: &str = "minecraft:air_drag_modifier";

/// Retains effective current and identifies the packet's native sprint modifier.
/// A packet without it replaces any locally predicted modifier. Ambiguous or
/// invalid authority is skipped, never promoted into a session error.
pub(super) fn effective_speed(attribute: &ActorAttribute) -> Option<(f64, Option<f32>)> {
    let speed = attribute.current;
    if !speed.is_finite() || speed < 0.0 {
        return None;
    }
    let mut sprint = attribute.modifiers.iter().filter(|modifier| {
        modifier.id.eq_ignore_ascii_case(SPRINT_MODIFIER_ID)
            && modifier.operation == 2
            && modifier.operand == 2
    });
    let sprint_modifier = if let Some(modifier) = sprint.next() {
        if sprint.next().is_some() || !modifier.amount.is_finite() {
            return None;
        }
        let denominator = 1.0 + modifier.amount;
        if !denominator.is_finite() || denominator <= 0.0 {
            return None;
        }
        Some(denominator)
    } else {
        None
    };
    Some((f64::from(speed), sprint_modifier))
}
