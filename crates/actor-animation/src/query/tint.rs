//! The native actor tint that render layers inherit as their change colour when their controller
//! sets no `color`.
use super::{ActorMetadataValue, AnimatedActor, actor_flag};
use assets::facts::entity::NativeEntityClass;
use protocol_ids::{ActorDataKey, ActorFlag};

/// The exact name that turns a sheep's wool into the rainbow.
const RAINBOW_NAME: &str = "jeb_";
/// Ticks between successive rainbow palette entries.
const RAINBOW_TICKS_PER_COLOR: u32 = 25;
/// Multiplies the low unique-id word to offset each sheep's rainbow phase.
const RAINBOW_PHASE_MULTIPLIER: u32 = 0x0210_6547;

/// Sheep take their byte dye, or the `jeb_` rainbow, every tick; tamed wolves take their byte
/// collar dye. Every other actor keeps the neutral multiplier.
///
/// The sheep rule follows the native system that writes the tint for actors carrying the sheep
/// flag: a `jeb_` name (exact string metadata) blends palette entries
/// `n / 25` and `n / 25 + 1` by `(n % 25) / 25`, where
/// `n = low32(unique id) * 0x2106547 + actor tick count` in wrapping 32-bit arithmetic; otherwise
/// a byte colour index selects the palette entry and any other type reads as white.
pub(crate) fn default_render_color(actor: &dyn AnimatedActor, life_tick: u64) -> [f32; 4] {
    match actor.entity_fact().native_class {
        NativeEntityClass::Sheep if has_rainbow_name(actor) => rainbow_color(actor, life_tick),
        NativeEntityClass::Sheep => metadata_color(actor),
        NativeEntityClass::Wolf if actor_flag(actor, ActorFlag::Tamed.wire_value()) => {
            metadata_color(actor)
        }
        _ => [1.0; 4],
    }
}

fn has_rainbow_name(actor: &dyn AnimatedActor) -> bool {
    matches!(
        actor.metadata().get(&ActorDataKey::Name.wire_value()),
        Some(ActorMetadataValue::String(name)) if name.as_ref() == RAINBOW_NAME
    )
}

/// The palette entry of the byte colour index; white when absent or of another type.
fn metadata_color(actor: &dyn AnimatedActor) -> [f32; 4] {
    let index = match actor.metadata().get(&ActorDataKey::ColorIndex.wire_value()) {
        Some(ActorMetadataValue::Byte(value)) => *value as u8,
        _ => 0,
    };
    assets::dye::actor_palette_color(index)
}

fn rainbow_color(actor: &dyn AnimatedActor, life_tick: u64) -> [f32; 4] {
    // Native arithmetic is 32-bit and wraps; the palette index keeps the low four bits.
    let ticks = (actor.unique_id() as u32)
        .wrapping_mul(RAINBOW_PHASE_MULTIPLIER)
        .wrapping_add(life_tick as u32);
    let step = ticks / RAINBOW_TICKS_PER_COLOR;
    let weight = (ticks % RAINBOW_TICKS_PER_COLOR) as f32 / RAINBOW_TICKS_PER_COLOR as f32;
    let from = assets::dye::actor_palette_color(step as u8);
    let to = assets::dye::actor_palette_color(step.wrapping_add(1) as u8);
    std::array::from_fn(|channel| from[channel] * (1.0 - weight) + to[channel] * weight)
}

#[cfg(test)]
mod tests;
