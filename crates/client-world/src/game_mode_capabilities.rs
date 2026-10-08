//! What a game mode lets the local player do, decoupled from the block-breaking
//! wire mode. Mode defaults follow dragonfly's `world.GameMode` interface (MIT);
//! server ability bits refine them where the session actually sent evidence.

use protocol::{AbilitiesUpdate, AbilityLayersEvidence, PlayerGameMode};

/// Documented survival/adventure melee reach from the eye.
pub const SURVIVAL_ATTACK_REACH: f64 = 3.0;
/// Creative melee reach. Needs independent measurement.
const CREATIVE_ATTACK_REACH: f64 = 7.0;

/// Bedrock ability bit positions, per the pinned gophertunnel
/// `minecraft/protocol/ability.go`. Only the bits the client gates on are named.
pub mod ability_bit {
    pub const BUILD: u32 = 1 << 0;
    pub const MINE: u32 = 1 << 1;
    pub const DOORS_AND_SWITCHES: u32 = 1 << 2;
    pub const OPEN_CONTAINERS: u32 = 1 << 3;
    pub const INVULNERABLE: u32 = 1 << 8;
    pub const FLYING: u32 = 1 << 9;
    pub const MAY_FLY: u32 = 1 << 10;
    // Retained wire evidence, not a mining gate. Vanilla instant destruction checks
    // the creative game mode, not the Instabuild ability (see game-mode-updates.md).
    pub const INSTANT_BUILD: u32 = 1 << 11;
    pub const NO_CLIP: u32 = 1 << 17;
    pub const FLY_SPEED: u32 = 1 << 13;
    pub const VERTICAL_FLY_SPEED: u32 = 1 << 19;
}

/// The interaction gates one game mode grants, after server ability overrides.
///
/// `attack_reach` is 0.0 exactly when attacking is disallowed. `creative_reach`
/// selects the block-pick reach profile, not a capability. The full surface is
/// modeled from the authoritative reference; not every field gates a producer yet.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub struct GameModeCapabilities {
    pub can_build: bool,
    pub can_mine: bool,
    /// Doors, trapdoors, fence gates, buttons and levers.
    pub can_use_switches: bool,
    pub can_open_containers: bool,
    /// Using a held item in the air (bows, food, tridents).
    pub can_use_items: bool,
    pub can_attack: bool,
    pub can_fly: bool,
    pub flying: bool,
    pub creative_inventory: bool,
    /// Creative destruction route, independent of the Instabuild ability.
    pub instant_break: bool,
    pub invulnerable: bool,
    pub visible: bool,
    pub has_collision: bool,
    pub attack_reach: f64,
    pub creative_reach: bool,
}

impl GameModeCapabilities {
    /// Mode defaults with no server evidence applied.
    pub const fn for_mode(mode: PlayerGameMode) -> Self {
        match mode {
            PlayerGameMode::Survival => Self {
                can_build: true,
                can_mine: true,
                can_use_switches: true,
                can_open_containers: true,
                can_use_items: true,
                can_attack: true,
                can_fly: false,
                flying: false,
                creative_inventory: false,
                instant_break: false,
                invulnerable: false,
                visible: true,
                has_collision: true,
                attack_reach: SURVIVAL_ATTACK_REACH,
                creative_reach: false,
            },
            PlayerGameMode::Creative => Self {
                can_build: true,
                can_mine: true,
                can_use_switches: true,
                can_open_containers: true,
                can_use_items: true,
                can_attack: true,
                can_fly: true,
                flying: false,
                creative_inventory: true,
                instant_break: true,
                invulnerable: true,
                visible: true,
                has_collision: true,
                attack_reach: CREATIVE_ATTACK_REACH,
                creative_reach: true,
            },
            // Adventure interacts and attacks; building and mining wait for
            // explicit server grants.
            PlayerGameMode::Adventure => Self {
                can_build: false,
                can_mine: false,
                can_use_switches: true,
                can_open_containers: true,
                can_use_items: true,
                can_attack: true,
                can_fly: false,
                flying: false,
                creative_inventory: false,
                instant_break: false,
                invulnerable: false,
                visible: true,
                has_collision: true,
                attack_reach: SURVIVAL_ATTACK_REACH,
                creative_reach: false,
            },
            PlayerGameMode::Spectator => Self {
                can_build: false,
                can_mine: false,
                can_use_switches: false,
                can_open_containers: false,
                can_use_items: false,
                can_attack: false,
                can_fly: true,
                flying: true,
                creative_inventory: false,
                instant_break: false,
                invulnerable: true,
                visible: false,
                has_collision: false,
                attack_reach: 0.0,
                creative_reach: false,
            },
            // Vanilla gives any id but survival its base game-mode logic, which
            // interacts freely; server abilities still refine Build and Mine.
            PlayerGameMode::Unknown => Self::for_mode(PlayerGameMode::Survival),
        }
    }

    /// Whether any block use (placement or interaction) is permitted.
    pub const fn can_use_blocks(&self) -> bool {
        self.can_use_items || self.can_build || self.can_use_switches || self.can_open_containers
    }

    /// Mode defaults with any server-sent ability bits folded in. Only bits an
    /// ability layer actually defines override a default; the rest stand.
    pub fn resolve(mode: PlayerGameMode, abilities: Option<&AbilitiesUpdate>) -> Self {
        let mut caps = Self::for_mode(mode);
        let Some(abilities) = abilities else {
            return caps;
        };
        let resolved = |bit: u32| resolved_ability(abilities, bit);
        for (bit, field) in [
            (ability_bit::BUILD, &mut caps.can_build),
            (ability_bit::MINE, &mut caps.can_mine),
            (ability_bit::DOORS_AND_SWITCHES, &mut caps.can_use_switches),
            (ability_bit::OPEN_CONTAINERS, &mut caps.can_open_containers),
        ] {
            if let Some(value) = resolved(bit) {
                *field = value;
            }
        }
        if let Some(may_fly) = resolved(ability_bit::MAY_FLY) {
            caps.can_fly = may_fly;
        }
        if let Some(flying) = resolved(ability_bit::FLYING) {
            caps.flying = flying;
        }
        if let Some(invulnerable) = resolved(ability_bit::INVULNERABLE) {
            caps.invulnerable = invulnerable;
        }
        if let Some(no_clip) = resolved(ability_bit::NO_CLIP) {
            caps.has_collision = !no_clip;
        }
        caps
    }
}

/// Resolves a boolean using the same typed layers as float abilities.
fn resolved_ability(update: &AbilitiesUpdate, bit: u32) -> Option<bool> {
    resolved_layer(update, bit).map(|layer| layer.values & bit != 0)
}

/// Higher layer types win; a repeated layer replaces that layer's entire definition.
pub fn resolved_layer(
    update: &AbilitiesUpdate,
    bit: u32,
) -> Option<&protocol::AbilityLayerEvidence> {
    let AbilityLayersEvidence::Received(layers) = &update.layers else {
        return None;
    };
    (0..6).rev().find_map(|kind| {
        layers
            .iter()
            .rev()
            .find(|layer| layer.layer_type == kind)
            .filter(|layer| layer.abilities & bit != 0)
    })
}

#[cfg(test)]
mod tests;
