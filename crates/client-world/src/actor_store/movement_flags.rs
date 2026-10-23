//! Movement flags accepted from actor metadata.

use super::*;

/// Movement-affecting metadata one local-player update carries; `None`
/// when the flag word holding it was absent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MovementFlagUpdate {
    pub immobile: Option<bool>,
    pub sneaking: Option<bool>,
    pub sprinting: Option<bool>,
    pub gliding: Option<bool>,
    pub swimming: Option<bool>,
    pub crawling: Option<bool>,
    pub has_gravity: Option<bool>,
    pub uniform_air_drag: Option<bool>,
    pub moving: Option<bool>,
    pub has_collision: Option<bool>,
    pub using_item: Option<bool>,
    pub spinning: Option<bool>,
    /// Raw second-word input for the climber correction owner.
    pub extended_flags: Option<u64>,
}

impl MovementFlagUpdate {
    /// Combines arrival-ordered sparse updates without clearing absent fields.
    pub fn merge(&mut self, update: Self) {
        *self = Self {
            immobile: update.immobile.or(self.immobile),
            sneaking: update.sneaking.or(self.sneaking),
            sprinting: update.sprinting.or(self.sprinting),
            gliding: update.gliding.or(self.gliding),
            swimming: update.swimming.or(self.swimming),
            crawling: update.crawling.or(self.crawling),
            has_gravity: update.has_gravity.or(self.has_gravity),
            uniform_air_drag: update.uniform_air_drag.or(self.uniform_air_drag),
            moving: update.moving.or(self.moving),
            has_collision: update.has_collision.or(self.has_collision),
            using_item: update.using_item.or(self.using_item),
            spinning: update.spinning.or(self.spinning),
            extended_flags: update.extended_flags.or(self.extended_flags),
        };
    }

    /// Reads the flag words from `metadata`; `None` when no word is present.
    #[must_use]
    pub fn from_metadata(metadata: &[protocol::ActorMetadata]) -> Option<Self> {
        let word = |key| {
            metadata.iter().rev().find_map(|item| match item.value {
                ActorMetadataValue::Flags(flags) | ActorMetadataValue::FlagsExtended(flags)
                    if item.key == key =>
                {
                    Some(flags)
                }
                _ => None,
            })
        };
        let words = [
            word(ActorDataKey::Flags.wire_value()),
            word(ActorDataKey::FlagsTwo.wire_value()),
            word(ActorDataKey::FlagsThird.wire_value()),
        ];
        if words.iter().all(Option::is_none) {
            return None;
        }
        let bit =
            |bit: u32| words[(bit / 64) as usize].map(|flags| flags & (1_u64 << (bit % 64)) != 0);
        Some(Self {
            immobile: bit(ActorFlag::NoAI.wire_value()),
            sneaking: bit(ActorFlag::Sneaking.wire_value()),
            sprinting: bit(ActorFlag::Sprinting.wire_value()),
            gliding: bit(ActorFlag::Gliding.wire_value()),
            swimming: bit(ActorFlag::Swimming.wire_value()),
            crawling: bit(ActorFlag::Crawling.wire_value()),
            has_gravity: bit(ActorFlag::HasGravity.wire_value()),
            uniform_air_drag: bit(ActorFlag::UsesUniformAirDrag.wire_value()),
            moving: bit(ActorFlag::Moving.wire_value()),
            has_collision: bit(ActorFlag::HasCollision.wire_value()),
            using_item: bit(ActorFlag::UsingItem.wire_value()),
            spinning: bit(ActorFlag::DamageNearbyMobs.wire_value()),
            extended_flags: words[1],
        })
    }
}
