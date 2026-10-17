use super::*;
use assets::facts::entity::NativeEntityClass;
use protocol_ids::ActorDataKey;

/// Potion projectiles expose the effect appearance of their own short auxiliary value.
pub(super) fn variant(actor: &dyn AnimatedActor) -> Option<f32> {
    if !matches!(
        actor.entity_fact().native_class,
        NativeEntityClass::SplashPotion | NativeEntityClass::LingeringPotion
    ) {
        return None;
    }
    let auxiliary = match actor
        .metadata()
        .get(&ActorDataKey::AuxValueData.wire_value())
    {
        Some(ActorMetadataValue::Short(auxiliary)) => *auxiliary,
        _ => 0,
    };
    Some(assets::potion::vanilla_potion_variant(auxiliary).map_or(0.0, f32::from))
}
