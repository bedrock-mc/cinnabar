//! Fixtures shared with application adapter tests, unavailable in production.

pub mod survival_mining;

/// The retained enchantment identity used by movement owner fixtures.
pub const DEPTH_STRIDER_ENCHANTMENT_ID: i16 =
    crate::movement::local_facts::DEPTH_STRIDER_ENCHANTMENT_ID;

/// Builds a movement packet with explicit current, default and sprint authority.
#[cfg(test)]
pub(crate) fn movement_speed_attribute(
    current: f64,
    default: f32,
    factor: Option<f32>,
) -> client_world::MovementSpeedAttribute {
    let modifiers = factor
        .map(|factor| protocol::ActorAttributeModifier {
            id: std::sync::Arc::from(client_world::SPRINT_SPEED_MODIFIER_ID),
            name: std::sync::Arc::from("sprint"),
            amount: factor - 1.0,
            operation: 2,
            operand: 2,
            serializable: false,
        })
        .into_iter()
        .collect::<Vec<_>>();
    let mut attribute =
        client_world::MovementSpeedAttribute::from_attribute(&protocol::ActorAttribute {
            name: std::sync::Arc::from("minecraft:movement"),
            min: 0.0,
            max: f32::MAX,
            current: default,
            default: Some(default),
            modifiers: modifiers.into(),
        })
        .unwrap();
    attribute.current = current;
    attribute
}
