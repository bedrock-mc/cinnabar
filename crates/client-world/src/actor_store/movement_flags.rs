//! Movement flags accepted from actor metadata.

use super::*;

/// Metadata words in the movement prediction packet's fixed wire order.
const FLAG_KEYS: [ActorDataKey; 3] = [
    ActorDataKey::Flags,
    ActorDataKey::FlagsTwo,
    ActorDataKey::FlagsThird,
];

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
    /// Supplies every actor-owned flag, using spawn defaults for absent words.
    pub(crate) fn for_replacement(actor: Option<&ActorSnapshot>) -> Self {
        let words = FLAG_KEYS.map(|key| {
            match actor.and_then(|actor| actor.metadata.get(&key.wire_value())) {
                Some(ActorMetadataValue::Flags(word) | ActorMetadataValue::FlagsExtended(word)) => {
                    Some(*word)
                }
                _ => None,
            }
        });
        let flags = Self::from_words(words);
        Self {
            immobile: Some(flags.immobile.unwrap_or(false)),
            sneaking: Some(flags.sneaking.unwrap_or(false)),
            sprinting: Some(flags.sprinting.unwrap_or(false)),
            gliding: Some(flags.gliding.unwrap_or(false)),
            swimming: Some(flags.swimming.unwrap_or(false)),
            crawling: Some(flags.crawling.unwrap_or(false)),
            has_gravity: Some(flags.has_gravity.unwrap_or(true)),
            uniform_air_drag: Some(flags.uniform_air_drag.unwrap_or(false)),
            moving: Some(flags.moving.unwrap_or(false)),
            has_collision: Some(flags.has_collision.unwrap_or(true)),
            using_item: Some(flags.using_item.unwrap_or(false)),
            spinning: Some(flags.spinning.unwrap_or(false)),
            extended_flags: Some(words[1].unwrap_or(0)),
        }
    }

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
        let words = FLAG_KEYS.map(|key| word(key.wire_value()));
        if words.iter().all(Option::is_none) {
            return None;
        }
        Some(Self::from_words(words))
    }

    /// Reads supplied flag words only when the actor retained them after admission.
    pub(crate) fn from_admitted_metadata(
        metadata: &[protocol::ActorMetadata],
        actor: &ActorSnapshot,
    ) -> Option<Self> {
        let words = FLAG_KEYS.map(|key| {
            let key = key.wire_value();
            if !metadata.iter().any(|entry| entry.key == key) {
                return None;
            }
            match actor.metadata.get(&key) {
                Some(ActorMetadataValue::Flags(word) | ActorMetadataValue::FlagsExtended(word)) => {
                    Some(*word)
                }
                _ => None,
            }
        });
        words
            .iter()
            .any(Option::is_some)
            .then(|| Self::from_words(words))
    }

    /// Decodes the shared sparse movement fields from their three accepted flag words.
    fn from_words(words: [Option<u64>; 3]) -> Self {
        let bit =
            |bit: u32| words[(bit / 64) as usize].map(|flags| flags & (1_u64 << (bit % 64)) != 0);
        Self {
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_defaults_cover_every_movement_field() {
        assert_eq!(
            MovementFlagUpdate::for_replacement(None),
            MovementFlagUpdate {
                immobile: Some(false),
                sneaking: Some(false),
                sprinting: Some(false),
                gliding: Some(false),
                swimming: Some(false),
                crawling: Some(false),
                has_gravity: Some(true),
                uniform_air_drag: Some(false),
                moving: Some(false),
                has_collision: Some(true),
                using_item: Some(false),
                spinning: Some(false),
                extended_flags: Some(0),
            }
        );
    }
}
