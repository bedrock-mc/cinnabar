use super::{ACTOR_FLAG_TAMED, ActorKind, ActorMetadataValue, ActorSnapshot};

pub(crate) const WOLF_IDENTIFIER: &str = "minecraft:wolf";
pub(crate) const COLOR_METADATA_KEY: u32 = 3;

/// Supplies the wolf's tamed dye tint when its controller inherits actor color.
/// Other actors and wild wolves retain the neutral multiplier.
pub(crate) fn default_render_color(actor: &ActorSnapshot) -> [f32; 4] {
    if !matches!(&actor.kind, ActorKind::Entity { identifier } if identifier.as_ref() == WOLF_IDENTIFIER)
        || !actor.flag(ACTOR_FLAG_TAMED)
    {
        return [1.0; 4];
    }
    let index = match actor.metadata.get(&COLOR_METADATA_KEY) {
        Some(ActorMetadataValue::Byte(value)) => *value as u8,
        _ => 0,
    };
    assets::dye::actor_palette_color(index)
}
