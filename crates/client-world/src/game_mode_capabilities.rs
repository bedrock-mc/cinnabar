//! What a game mode lets the local player do, decoupled from the block-breaking
//! wire mode. Mode defaults follow dragonfly's `world.GameMode` interface (MIT);
//! server ability bits refine them where the session actually sent evidence.

use protocol::{AbilitiesUpdate, AbilityLayersEvidence, PlayerGameMode};

/// Documented survival/adventure melee reach from the eye.
pub const SURVIVAL_ATTACK_REACH: f64 = 3.0;
/// Creative actor comparison cap; the input mode's shorter pick ray still applies.
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
    pub const PRIVILEGED_BUILDER: u32 = 1 << 18;
    pub const FLY_SPEED: u32 = 1 << 13;
    pub const VERTICAL_FLY_SPEED: u32 = 1 << 19;
}

/// Known ability-layer identities shared by effective permission consumers.
pub mod ability_layer {
    pub const COUNT: usize = 6;
    pub const BASE: usize = 1;
    pub const SPECTATOR: usize = super::SPECTATOR_LAYER_TYPE as usize;
}

/// Spectator ability layer type, per the pinned gophertunnel
/// `minecraft/protocol/ability.go` (`AbilityLayerTypeSpectator`).
pub const SPECTATOR_LAYER_TYPE: u16 = 2;

/// Effective server flight and whether local requests may change it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlightAuthority {
    /// Ordinary layers permit the local flight toggle.
    Mutable(bool),
    /// A higher layer keeps its value across local toggle requests.
    Fixed(bool),
}

impl Default for FlightAuthority {
    fn default() -> Self {
        Self::Mutable(false)
    }
}

impl FlightAuthority {
    /// Returns the effective server value independently of local requests.
    #[must_use]
    pub const fn is_flying(self) -> bool {
        match self {
            Self::Mutable(value) | Self::Fixed(value) => value,
        }
    }
}

/// Which rule vanilla's block-permission check applies before the Build or Mine grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditPermission {
    /// Survival and Creative: the Build and Mine grants alone decide.
    Granted,
    /// Adventure: a held item's CanPlaceOn or CanDestroy list decides, and an empty hand
    /// needs the PrivilegedBuilder ability.
    Adventure { privileged_builder: bool },
}

/// What the player holds when vanilla checks one block edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeldEditItem {
    /// No item, or an empty stack.
    EmptyHand,
    /// An item whose relevant restriction list does (`true`) or does not name the block.
    Item { lists_block: bool },
}

/// The interaction gates one game mode grants, after server ability overrides.
///
/// `attack_reach` is 0.0 exactly when attacking is disallowed. `creative_reach`
/// selects the block-pick reach profile, not a capability. `can_build` and `can_mine`
/// are the Build and Mine grants; [`Self::permits_edit`] adds the per-block adventure
/// rule. The full surface is modeled from the authoritative reference; not every field
/// gates a producer yet.
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
    pub flight: FlightAuthority,
    pub creative_inventory: bool,
    /// Creative destruction route, independent of the Instabuild ability.
    pub instant_break: bool,
    pub invulnerable: bool,
    pub visible: bool,
    pub has_collision: bool,
    pub attack_reach: f64,
    pub creative_reach: bool,
    pub edit_permission: EditPermission,
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
                flight: FlightAuthority::Mutable(false),
                creative_inventory: false,
                instant_break: false,
                invulnerable: false,
                visible: true,
                has_collision: true,
                attack_reach: SURVIVAL_ATTACK_REACH,
                creative_reach: false,
                edit_permission: EditPermission::Granted,
            },
            PlayerGameMode::Creative => Self {
                can_build: true,
                can_mine: true,
                can_use_switches: true,
                can_open_containers: true,
                can_use_items: true,
                can_attack: true,
                can_fly: true,
                flight: FlightAuthority::Mutable(false),
                creative_inventory: true,
                instant_break: true,
                invulnerable: true,
                visible: true,
                has_collision: true,
                attack_reach: CREATIVE_ATTACK_REACH,
                creative_reach: true,
                edit_permission: EditPermission::Granted,
            },
            // Adventure interacts and attacks; building and mining wait for
            // explicit server grants, and each edit also passes the adventure rule.
            PlayerGameMode::Adventure => Self {
                can_build: false,
                can_mine: false,
                can_use_switches: true,
                can_open_containers: true,
                can_use_items: true,
                can_attack: true,
                can_fly: false,
                flight: FlightAuthority::Mutable(false),
                creative_inventory: false,
                instant_break: false,
                invulnerable: false,
                visible: true,
                has_collision: true,
                attack_reach: SURVIVAL_ATTACK_REACH,
                creative_reach: false,
                edit_permission: EditPermission::Adventure {
                    privileged_builder: false,
                },
            },
            PlayerGameMode::Spectator => Self {
                can_build: false,
                can_mine: false,
                can_use_switches: false,
                can_open_containers: false,
                can_use_items: false,
                can_attack: false,
                can_fly: false,
                flight: FlightAuthority::Fixed(true),
                creative_inventory: false,
                instant_break: false,
                invulnerable: true,
                visible: false,
                has_collision: false,
                attack_reach: 0.0,
                creative_reach: false,
                edit_permission: EditPermission::Granted,
            },
            // Vanilla gives any id but survival its base game-mode logic, which
            // interacts freely; server abilities still refine Build and Mine.
            PlayerGameMode::Unknown => Self::for_mode(PlayerGameMode::Survival),
        }
    }

    /// Whether vanilla's block-permission rule admits one edit before its Build or Mine grant.
    ///
    /// In Adventure, holding an item replaces the PrivilegedBuilder check with that item's
    /// list: an item whose list omits the block, or has no list, may not edit it.
    pub const fn permits_edit(&self, held: HeldEditItem) -> bool {
        match (self.edit_permission, held) {
            (EditPermission::Granted, _) => true,
            (EditPermission::Adventure { privileged_builder }, HeldEditItem::EmptyHand) => {
                privileged_builder
            }
            (EditPermission::Adventure { .. }, HeldEditItem::Item { lists_block }) => lists_block,
        }
    }

    /// These capabilities for one block edit: Build and Mine stay granted only when
    /// [`Self::permits_edit`] admits what the player holds.
    #[must_use]
    pub const fn for_edit(mut self, held: HeldEditItem) -> Self {
        let permitted = self.permits_edit(held);
        self.can_build &= permitted;
        self.can_mine &= permitted;
        self
    }

    /// Whether any block use (placement or interaction) is permitted.
    pub const fn can_use_blocks(&self) -> bool {
        self.can_use_items || self.can_build || self.can_use_switches || self.can_open_containers
    }

    /// Mode defaults with any server-sent ability bits folded in. Only bits an
    /// ability layer actually defines override a default; the rest stand.
    pub fn resolve(mode: PlayerGameMode, abilities: Option<&AbilitiesUpdate>) -> Self {
        let mut caps = Self::for_mode(mode);
        // Spectator mode supplies its own permission layer above ordinary grants.
        if mode == PlayerGameMode::Spectator {
            return caps;
        }
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
        if let EditPermission::Adventure { privileged_builder } = &mut caps.edit_permission {
            *privileged_builder = resolved(ability_bit::PRIVILEGED_BUILDER).unwrap_or(false);
        }
        if let Some(may_fly) = resolved(ability_bit::MAY_FLY) {
            caps.can_fly = may_fly;
        }
        if let Some(layer) = resolved_layer(abilities, ability_bit::FLYING) {
            let flying = layer.values & ability_bit::FLYING != 0;
            caps.flight = if layer.layer_type >= SPECTATOR_LAYER_TYPE {
                FlightAuthority::Fixed(flying)
            } else {
                FlightAuthority::Mutable(flying)
            };
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
    (0..ability_layer::COUNT as u16).rev().find_map(|kind| {
        layers
            .iter()
            .rev()
            .find(|layer| layer.layer_type == kind)
            .filter(|layer| layer.abilities & bit != 0)
    })
}

#[cfg(test)]
mod tests;
