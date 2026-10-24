//! Local animation flags kept separate from accepted gameplay metadata.

use super::{ActorSnapshot, LocalItemUse, LocalPlayerFeed};
use protocol::ActorMetadataValue;
use protocol_ids::{ActorDataKey, ActorFlag};

impl ActorSnapshot {
    /// Updates the local rig's primary flag word without authoring gameplay metadata.
    /// Unpredicted use retains its visual state until a server word or another use pose arrives.
    pub(super) fn apply_local_flags(&mut self, feed: &LocalPlayerFeed) {
        let mut flags = if let Some(flags) = self.local_presentation_flags {
            flags
        } else {
            match self.metadata.get(&ActorDataKey::Flags.wire_value()) {
                Some(
                    ActorMetadataValue::Flags(flags) | ActorMetadataValue::FlagsExtended(flags),
                ) => *flags,
                None => 0,
                _ => return,
            }
        };
        let mut set = |flag: ActorFlag, enabled| {
            let mask = 1_u64
                .checked_shl(flag.wire_value())
                .expect("local presentation flags belong to the primary word");
            flags = (flags & !mask) | if enabled { mask } else { 0 };
        };
        set(ActorFlag::Sneaking, feed.sneaking);
        set(ActorFlag::Sprinting, feed.sprinting);
        set(ActorFlag::Gliding, feed.gliding);
        set(ActorFlag::DamageNearbyMobs, feed.spinning);
        // Keep the rig's sprint/water pose independent of gameplay swimming.
        let in_water = self.status.fluid.is_some_and(|(water, _)| water);
        set(ActorFlag::Swimming, feed.sprinting && in_water);
        match feed.item_use {
            LocalItemUse::Unpredicted => {}
            LocalItemUse::Idle => set(ActorFlag::UsingItem, false),
            LocalItemUse::Using => set(ActorFlag::UsingItem, true),
            LocalItemUse::Predicted { elapsed_ticks, .. } => {
                set(ActorFlag::UsingItem, elapsed_ticks.is_some());
            }
        }
        self.local_presentation_flags = Some(flags);
    }
}
