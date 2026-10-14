//! Fixtures shared with application adapter tests, unavailable in production.

pub mod survival_mining;

/// Builds a movement packet with explicit current, default and sprint authority.
#[cfg(test)]
pub(crate) fn movement_speed_attribute(
    current: f64,
    default: f32,
    factor: Option<f32>,
) -> client_world::MovementSpeedAttribute {
    let modifiers = factor
        .map(|factor| protocol::ActorAttributeModifier {
            id: std::sync::Arc::from(protocol_ids::SPRINT_SPEED_MODIFIER_ID),
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

/// Returns facts for an actual pinned identity, or unresolved facts for an unknown identity.
#[cfg(test)]
pub(crate) fn block_fact(name: &str) -> assets::facts::BlockFact {
    static PALETTE: std::sync::OnceLock<(
        Box<[assets::RegistryRecord]>,
        assets::facts::BlockFacts,
    )> = std::sync::OnceLock::new();
    let (records, facts) = PALETTE.get_or_init(|| {
        let records = assets::read_registry_for_protocol(
            assets::pinned_block_registry_bytes(),
            assets::active_content_registry_protocol(),
        )
        .unwrap();
        let facts = assets::facts::BlockFacts::new(&records).unwrap();
        (records, facts)
    });
    records
        .iter()
        .find(|r| r.name.as_ref() == name)
        .map_or_else(Default::default, |r| facts.get(r.sequential_id))
}
