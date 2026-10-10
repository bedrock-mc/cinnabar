use protocol::ActorAttribute;

/// UUID of the current-value sprint speed modifier.
pub const SPRINT_SPEED_MODIFIER_ID: &str = "d208fc00-42aa-4aad-9276-d5446530de43";
pub const AIR_DRAG_MODIFIER_ATTRIBUTE: &str = "minecraft:air_drag_modifier";

/// Effective movement current and the defaults needed to recalculate a sprint edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MovementSpeedAttribute {
    /// Server current, including any installed modifiers.
    pub current: f64,
    /// Installed sprint factor; absence means a stop has nothing to remove.
    pub sprint_modifier: Option<f32>,
    default: f32,
    base: f32,
    min: f32,
    max: f32,
    other_modifiers: bool,
}

impl MovementSpeedAttribute {
    /// Retains packet current and evaluates its defaults without the sprint modifier.
    pub fn from_attribute(attribute: &ActorAttribute) -> Option<Self> {
        let (current, sprint_modifier) = effective_speed(attribute)?;
        let default = attribute.default?;
        if !default.is_finite()
            || !attribute.min.is_finite()
            || !attribute.max.is_finite()
            || attribute.min < 0.0
            || attribute.min > attribute.max
        {
            return None;
        }
        let modifiers = || {
            attribute
                .modifiers
                .iter()
                .filter(|modifier| !is_sprint(modifier))
        };
        let mut base = default;
        for modifier in
            modifiers().filter(|modifier| modifier.operation == 0 && modifier.operand == 2)
        {
            base += modifier.amount;
        }
        let added = base;
        for modifier in
            modifiers().filter(|modifier| modifier.operation == 1 && modifier.operand == 2)
        {
            base += added * modifier.amount;
        }
        for modifier in
            modifiers().filter(|modifier| modifier.operation == 2 && modifier.operand == 2)
        {
            base *= 1.0 + modifier.amount;
        }
        let mut max = attribute.max;
        for modifier in modifiers().filter(|modifier| modifier.operation == 3) {
            max = max.min(modifier.amount);
        }
        if !base.is_finite()
            || max < attribute.min
            || attribute
                .modifiers
                .iter()
                .any(|modifier| !modifier.amount.is_finite())
        {
            return None;
        }
        Some(Self {
            current: f64::from((current as f32).clamp(attribute.min, max)),
            sprint_modifier,
            default,
            base,
            min: attribute.min,
            max,
            other_modifiers: modifiers().next().is_some(),
        })
    }

    /// Adds a finite positive sprint factor or removes it, recalculating from defaults on a change.
    pub fn set_sprint_modifier(&mut self, factor: Option<f32>) {
        if self.sprint_modifier == factor {
            return;
        }
        self.sprint_modifier = factor;
        let computed = self.base * factor.unwrap_or(1.0);
        let current = if computed == self.default && (self.other_modifiers || factor.is_some()) {
            self.current as f32
        } else {
            computed
        };
        self.current = f64::from(current.clamp(self.min, self.max));
    }
}

/// Identifies the native current-value sprint modifier independently of its label.
fn is_sprint(modifier: &protocol::ActorAttributeModifier) -> bool {
    modifier.id.eq_ignore_ascii_case(SPRINT_SPEED_MODIFIER_ID)
        && modifier.operation == 2
        && modifier.operand == 2
}

/// Retains effective current and identifies the packet's native sprint modifier.
/// A packet without it replaces any locally predicted modifier. Ambiguous or
/// invalid authority is skipped, never promoted into a session error.
pub(super) fn effective_speed(attribute: &ActorAttribute) -> Option<(f64, Option<f32>)> {
    let speed = attribute.current;
    if !speed.is_finite() || speed < 0.0 {
        return None;
    }
    let mut sprint = attribute
        .modifiers
        .iter()
        .filter(|modifier| is_sprint(modifier));
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

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::ActorAttributeModifier;
    use std::sync::Arc;

    /// Builds a received movement attribute with a distinct current and default.
    fn packet(current: f32, default: f32, modifiers: &[ActorAttributeModifier]) -> ActorAttribute {
        ActorAttribute {
            name: Arc::from("minecraft:movement"),
            min: 0.0,
            max: f32::MAX,
            current,
            default: Some(default),
            modifiers: Arc::from(modifiers),
        }
    }

    /// Builds a current-value modifier, identifying sprint only when requested.
    fn modifier(amount: f32, operation: i32, sprint: bool) -> ActorAttributeModifier {
        ActorAttributeModifier {
            id: Arc::from(if sprint {
                SPRINT_SPEED_MODIFIER_ID
            } else {
                "custom"
            }),
            name: Arc::from("speed"),
            amount,
            operation,
            operand: 2,
            serializable: false,
        }
    }

    #[test]
    fn empty_modifier_resend_restarts_from_default_instead_of_effective_current() {
        let mut speed = MovementSpeedAttribute::from_attribute(&packet(0.15, 0.1, &[])).unwrap();
        speed.set_sprint_modifier(None);
        assert_eq!(speed.current, f64::from(0.15_f32));
        for _ in 0..10 {
            speed.set_sprint_modifier(Some(1.5));
            assert!((speed.current - 0.15).abs() < 1e-7);
            speed.set_sprint_modifier(None);
            assert_eq!(speed.current, f64::from(0.1_f32));
        }
    }

    #[test]
    fn removing_packet_sprint_recalculates_even_when_current_disagrees_with_default() {
        let mut speed =
            MovementSpeedAttribute::from_attribute(&packet(0.25, 0.1, &[modifier(0.5, 2, true)]))
                .unwrap();
        speed.set_sprint_modifier(None);
        assert_eq!(speed.current, f64::from(0.1_f32));
    }

    #[test]
    fn sprint_recalculation_preserves_other_speed_operations_and_caps() {
        let mut speed = MovementSpeedAttribute::from_attribute(&packet(
            0.4,
            0.1,
            &[
                modifier(0.02, 0, false),
                modifier(0.5, 1, false),
                modifier(0.5, 2, false),
                modifier(0.4, 3, false),
            ],
        ))
        .unwrap();
        speed.set_sprint_modifier(Some(2.0));
        assert_eq!(speed.current, f64::from(0.4_f32));
        speed.set_sprint_modifier(None);
        assert!((speed.current - 0.27).abs() < 1e-7);

        let mut capped = packet(0.12, 0.1, &[modifier(0.5, 2, true)]);
        capped.max = 0.12;
        let mut speed = MovementSpeedAttribute::from_attribute(&capped).unwrap();
        speed.set_sprint_modifier(None);
        assert_eq!(speed.current, f64::from(0.1_f32));
        speed.set_sprint_modifier(Some(1.5));
        assert_eq!(speed.current, f64::from(0.12_f32));
    }

    #[test]
    fn remaining_modifiers_that_leave_default_unchanged_keep_current() {
        let mut speed = MovementSpeedAttribute::from_attribute(&packet(
            0.25,
            0.1,
            &[modifier(0.0, 0, false), modifier(0.5, 2, true)],
        ))
        .unwrap();
        speed.set_sprint_modifier(None);
        assert_eq!(speed.current, f64::from(0.25_f32));
    }
}
