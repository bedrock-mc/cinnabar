use protocol::ActorAttribute;

const SPRINT_MODIFIER_ID: &str = "d208fc00-42aa-4aad-9276-d5446530de43";

/// Removes only the identified total/current sprint multiplier. Other
/// modifiers and effects remain in the effective walk speed. Ambiguous or
/// invalid authority is skipped, never promoted into a session error.
pub(super) fn walk_speed(attribute: &ActorAttribute) -> Option<f64> {
    let mut speed = attribute.current;
    if !speed.is_finite() || speed < 0.0 {
        return None;
    }
    let mut sprint = attribute.modifiers.iter().filter(|modifier| {
        modifier.id.eq_ignore_ascii_case(SPRINT_MODIFIER_ID)
            && modifier.operation == 2
            && modifier.operand == 2
    });
    if let Some(modifier) = sprint.next() {
        if sprint.next().is_some() || !modifier.amount.is_finite() {
            return None;
        }
        let denominator = 1.0 + modifier.amount;
        if !denominator.is_finite() || denominator <= 0.0 {
            return None;
        }
        // Attribute arithmetic is f32 at this boundary, before widening into
        // the existing f64 simulator. This is not a physics precision change.
        speed /= denominator;
    }
    (speed.is_finite() && speed >= 0.0).then_some(f64::from(speed))
}
