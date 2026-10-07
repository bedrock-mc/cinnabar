use super::{ActorKind, ActorSnapshot};

impl ActorSnapshot {
    pub(crate) fn is_horse(&self) -> bool {
        matches!(&self.kind, ActorKind::Entity { identifier } if matches!(identifier.as_ref(),
            "minecraft:horse" | "minecraft:donkey" | "minecraft:mule" |
            "minecraft:zombie_horse" | "minecraft:skeleton_horse"))
    }

    /// Built-in living mount types; an unclassified custom entity stays on its own yaw path.
    pub fn is_known_living_mount(&self) -> bool {
        self.is_horse()
            || matches!(self.kind, ActorKind::Player { .. })
            || matches!(&self.kind, ActorKind::Entity { identifier } if matches!(identifier.as_ref(),
                "minecraft:pig" | "minecraft:llama" | "minecraft:trader_llama" |
                "minecraft:camel" | "minecraft:camel_husk" | "minecraft:strider" |
                "minecraft:happy_ghast"))
    }
}
