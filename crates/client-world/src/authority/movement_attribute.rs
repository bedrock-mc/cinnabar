use protocol::ActorAttribute;

const SPEED_EFFECT_STEP: f32 = 0.2;

/// Effective movement current and the defaults needed to recalculate a sprint edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MovementSpeedAttribute {
    /// Server current, including any installed modifiers.
    pub current: f64,
    /// Installed sprint factor; absence means a stop has nothing to remove.
    pub sprint_modifier: Option<f32>,
    speed_effect_modifier: Option<f32>,
    default: f32,
    base: f32,
    min: f32,
    max: f32,
    other_modifiers: bool,
}

impl MovementSpeedAttribute {
    /// Creates the initial unmodified attribute before the server sends its defaults.
    pub fn unmodified(default: f32) -> Self {
        Self {
            current: f64::from(default),
            sprint_modifier: None,
            speed_effect_modifier: None,
            default,
            base: default,
            min: 0.0,
            max: f32::MAX,
            other_modifiers: false,
        }
    }

    /// Retains packet current and normalizes its defaults for later modifier edits.
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
                .filter(|modifier| !is_sprint(modifier) && !is_speed_effect(modifier))
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
            speed_effect_modifier: modifier_factor(
                attribute,
                protocol_ids::SPEED_EFFECT_MODIFIER_ID,
            )?,
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
        self.recalculate_current();
    }

    /// Applies a server Speed effect change, preserving sprint and custom modifiers.
    /// Attribute packets remain authoritative and can replace this modifier afterward.
    /// Updating or removing an absent modifier leaves current untouched.
    pub fn apply_speed_effect(&mut self, action: protocol::ActorEffectAction, amplifier: i32) {
        use protocol::ActorEffectAction;
        match action {
            ActorEffectAction::Update | ActorEffectAction::Remove
                if self.speed_effect_modifier.is_none() =>
            {
                return;
            }
            ActorEffectAction::Add | ActorEffectAction::Update => {
                self.speed_effect_modifier =
                    Some(1.0 + (amplifier as f32 + 1.0) * SPEED_EFFECT_STEP);
            }
            ActorEffectAction::Remove => self.speed_effect_modifier = None,
            ActorEffectAction::Unknown(_) => return,
        }
        self.recalculate_current();
    }

    /// Evaluates the current operand after a local modifier edit and applies its bounds.
    fn recalculate_current(&mut self) {
        let computed = self.base
            * self.speed_effect_modifier.unwrap_or(1.0)
            * self.sprint_modifier.unwrap_or(1.0);
        let current = if computed == self.default
            && (self.other_modifiers
                || self.speed_effect_modifier.is_some()
                || self.sprint_modifier.is_some())
        {
            self.current as f32
        } else {
            computed
        };
        self.current = f64::from(current.clamp(self.min, self.max));
    }
}

/// Identifies the current-value movement modifier owned by the Speed effect.
fn is_speed_effect(modifier: &protocol::ActorAttributeModifier) -> bool {
    is_current_modifier(modifier, protocol_ids::SPEED_EFFECT_MODIFIER_ID)
}

/// Identifies the native current-value sprint modifier independently of its label.
fn is_sprint(modifier: &protocol::ActorAttributeModifier) -> bool {
    is_current_modifier(modifier, protocol_ids::SPRINT_SPEED_MODIFIER_ID)
}

/// Matches a known multiply-total modifier on the current operand.
fn is_current_modifier(modifier: &protocol::ActorAttributeModifier, id: &str) -> bool {
    modifier.id.eq_ignore_ascii_case(id) && modifier.operation == 2 && modifier.operand == 2
}

/// Reads one positive factor, rejecting duplicate or malformed known modifiers.
fn modifier_factor(attribute: &ActorAttribute, id: &str) -> Option<Option<f32>> {
    let mut modifiers = attribute
        .modifiers
        .iter()
        .filter(|modifier| is_current_modifier(modifier, id));
    let Some(modifier) = modifiers.next() else {
        return Some(None);
    };
    let factor = 1.0 + modifier.amount;
    (modifiers.next().is_none() && factor.is_finite() && factor > 0.0).then_some(Some(factor))
}

/// Retains effective current and identifies the packet's native sprint modifier.
/// A packet without it replaces any locally predicted modifier. Ambiguous or
/// invalid authority is skipped, never promoted into a session error.
pub(super) fn effective_speed(attribute: &ActorAttribute) -> Option<(f64, Option<f32>)> {
    let speed = attribute.current;
    if !speed.is_finite() || speed < 0.0 {
        return None;
    }
    let sprint_modifier = modifier_factor(attribute, protocol_ids::SPRINT_SPEED_MODIFIER_ID)?;
    Some((f64::from(speed), sprint_modifier))
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::ActorAttributeModifier;
    use protocol_ids::Attribute;
    use std::sync::Arc;

    /// Builds a received movement attribute with a distinct current and default.
    fn packet(current: f32, default: f32, modifiers: &[ActorAttributeModifier]) -> ActorAttribute {
        ActorAttribute {
            name: Arc::from(Attribute::Movement.wire_name()),
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
                protocol_ids::SPRINT_SPEED_MODIFIER_ID
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

    /// An absent Speed modifier leaves the server's distinct current untouched.
    #[test]
    fn speed_effect_without_an_installed_modifier_preserves_server_current() {
        use protocol::ActorEffectAction::{Remove, Update};

        let authoritative =
            MovementSpeedAttribute::from_attribute(&packet(0.17, 0.1, &[])).unwrap();
        for action in [Update, Remove] {
            let mut speed = authoritative;
            speed.apply_speed_effect(action, 2);
            assert_eq!(speed, authoritative, "{action:?}");
        }
    }

    /// Speed changes preserve other operations, sprint, and effective bounds.
    #[test]
    fn speed_effect_changes_recalculate_without_compounding_other_modifiers() {
        use protocol::ActorEffectAction::{Add, Remove, Update};

        let mut speed_modifier = modifier(0.6, 2, false);
        speed_modifier.id = Arc::from(protocol_ids::SPEED_EFFECT_MODIFIER_ID.to_uppercase());
        let mut received = packet(
            0.4,
            0.1,
            &[
                modifier(0.02, 0, false),
                modifier(0.5, 1, false),
                speed_modifier,
                modifier(0.3, 2, true),
            ],
        );
        received.max = 0.3;
        let mut speed = MovementSpeedAttribute::from_attribute(&received).unwrap();
        speed.apply_speed_effect(Update, 1);
        assert!((speed.current - 0.3).abs() < 1e-7);
        speed.apply_speed_effect(Remove, 0);
        assert!((speed.current - 0.234).abs() < 1e-7);
        speed.apply_speed_effect(Add, 0);
        assert!((speed.current - 0.2808).abs() < 1e-7);
        speed.set_sprint_modifier(None);
        assert!((speed.current - 0.216).abs() < 1e-7);
        speed.apply_speed_effect(Update, 1);
        assert!((speed.current - 0.252).abs() < 1e-7);
        speed.set_sprint_modifier(Some(1.3));
        assert!((speed.current - 0.3).abs() < 1e-7);
    }

    #[test]
    fn speed_effect_packets_reject_duplicate_and_invalid_factors() {
        let mut speed_modifier = modifier(0.6, 2, false);
        speed_modifier.id = Arc::from(protocol_ids::SPEED_EFFECT_MODIFIER_ID);
        assert!(
            MovementSpeedAttribute::from_attribute(&packet(
                0.1,
                0.1,
                &[speed_modifier.clone(), speed_modifier.clone(),]
            ))
            .is_none()
        );
        for amount in [f32::NAN, f32::INFINITY, -1.0] {
            speed_modifier.amount = amount;
            assert!(
                MovementSpeedAttribute::from_attribute(&packet(
                    0.1,
                    0.1,
                    &[speed_modifier.clone()]
                ))
                .is_none()
            );
        }
    }
}
