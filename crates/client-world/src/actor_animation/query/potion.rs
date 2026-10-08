use super::*;

pub(super) const AUX_VALUE_METADATA_KEY: u32 = 36;

/// Potion projectiles expose the effect appearance of their own short auxiliary value.
pub(super) fn variant(actor: &ActorSnapshot) -> Option<f32> {
    if !matches!(&actor.kind, ActorKind::Entity { identifier }
        if matches!(identifier.as_ref(), "minecraft:splash_potion" | "minecraft:lingering_potion"))
    {
        return None;
    }
    let auxiliary = match actor.metadata.get(&AUX_VALUE_METADATA_KEY) {
        Some(ActorMetadataValue::Short(auxiliary)) => *auxiliary,
        _ => 0,
    };
    Some(assets::vanilla_potion_variant(auxiliary).map_or(0.0, f32::from))
}
