//! Shared metadata and values for local-world options.

use super::Tab;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The control and accepted values of an option.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Control {
    Toggle,
    Number { min: i32, max: i32, step: i32 },
}

/// Where BDS reads an option.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Storage {
    Rule(&'static str),
    Level(&'static str),
    Property(&'static str, &'static str),
    /// Composes with another setting before writing the world.
    Combined,
}

/// A world option's presentation, defaults and BDS destination.
#[derive(Clone, Copy, Debug)]
pub struct Definition {
    pub label: &'static str,
    pub tab: Tab,
    pub default: i32,
    pub control: Control,
    pub cheats: bool,
    pub storage: Storage,
    pub creation_only: bool,
}

macro_rules! options {
    ($($id:ident: $label:literal, $tab:ident, $default:expr, $control:expr, $cheats:expr, $storage:expr, $creation:expr;)*) => {
        /// Stable identities shared by the form, saved metadata and BDS configuration.
        #[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(rename_all = "snake_case")]
        pub enum WorldOption { $($id,)* }
        impl WorldOption {
            pub const ALL: &'static [Self] = &[$(Self::$id,)*];
            /// Returns this option's single metadata definition.
            pub const fn definition(self) -> Definition {
                match self { $(Self::$id => Definition {
                    label: $label, tab: Tab::$tab, default: $default, control: $control,
                    cheats: $cheats, storage: $storage, creation_only: $creation,
                },)* }
            }
        }
    };
}

use Control::{Number, Toggle};
use Storage::{Combined, Level, Property, Rule};
options! {
    StartingMap: "Starting map", Advanced, 0, Toggle, false, Level("startWithMapEnabled"), true;
    BonusChest: "Bonus chest", Advanced, 0, Toggle, false, Level("bonusChestEnabled"), true;
    ShowCoordinates: "Show coordinates", Advanced, 0, Toggle, false, Rule("showcoordinates"), false;
    ShowDaysPlayed: "Show days played", Advanced, 0, Toggle, false, Rule("showdaysplayed"), false;
    FireSpreads: "Fire spreads", Advanced, 1, Toggle, false, Rule("dofiretick"), false;
    TntExplodes: "TNT explodes", Advanced, 1, Toggle, false, Rule("tntexplodes"), false;
    RespawnBlocksExplode: "Respawn blocks explode", Advanced, 1, Toggle, false, Rule("respawnblocksexplode"), false;
    MobLoot: "Mob loot", Advanced, 1, Toggle, false, Rule("domobloot"), false;
    NaturalRegeneration: "Natural regeneration", Advanced, 1, Toggle, false, Rule("naturalregeneration"), false;
    TileDrops: "Tile drops", Advanced, 1, Toggle, false, Rule("dotiledrops"), false;
    ImmediateRespawn: "Immediate respawn", Advanced, 0, Toggle, false, Rule("doimmediaterespawn"), false;
    RecipesUnlock: "Recipes unlock", Advanced, 1, Toggle, false, Rule("recipesunlock"), false;
    LimitedCrafting: "Limited crafting", Advanced, 0, Toggle, false, Rule("dolimitedcrafting"), false;
    Pvp: "Friendly fire (PvP)", Advanced, 1, Toggle, false, Rule("pvp"), false;
    SleepSkipNight: "Sleep through the night", Advanced, 1, Toggle, false, Combined, false;
    SleepPercentage: "Players sleeping percentage", Advanced, 100, Number { min: 0, max: 100, step: 1 }, false, Rule("playerssleepingpercentage"), false;
    RespawnRadius: "Respawn radius", Advanced, 10, Number { min: 0, max: i32::MAX, step: 1 }, false, Rule("spawnradius"), false;
    SimulationDistance: "Simulation distance", Advanced, 4, Number { min: 4, max: 12, step: 2 }, false, Property("tick-distance", "serverChunkTickRange"), false;
    Multiplayer: "Multiplayer game", Multiplayer, 1, Toggle, false, Level("MultiplayerGame"), false;
    LanVisible: "Visible to LAN players", Multiplayer, 1, Toggle, false, Property("enable-lan-visibility", "LANBroadcast"), false;
    ActivateCheats: "Activate cheats", Cheats, 0, Toggle, false, Property("allow-cheats", "cheatsEnabled"), false;
    AlwaysDay: "Always day", Cheats, 0, Toggle, true, Combined, false;
    DaylightCycle: "Daylight cycle", Cheats, 1, Toggle, true, Rule("dodaylightcycle"), false;
    KeepInventory: "Keep inventory", Cheats, 0, Toggle, true, Rule("keepinventory"), false;
    MobSpawning: "Mob spawning", Cheats, 1, Toggle, true, Rule("domobspawning"), false;
    MobGriefing: "Mob griefing", Cheats, 1, Toggle, true, Rule("mobgriefing"), false;
    EntitiesDropLoot: "Entities drop loot", Cheats, 1, Toggle, true, Rule("doentitydrops"), false;
    WeatherCycle: "Weather cycle", Cheats, 1, Toggle, true, Rule("doweathercycle"), false;
    CommandBlocks: "Command blocks enabled", Cheats, 1, Toggle, true, Rule("commandblocksenabled"), false;
    RandomTickSpeed: "Random tick speed", Cheats, 1, Number { min: 0, max: i32::MAX, step: 1 }, true, Rule("randomtickspeed"), false;
}

/// Saved overrides; missing options inherit their defaults for older worlds.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct WorldOptions(BTreeMap<WorldOption, i32>);

impl WorldOptions {
    /// Reads an option, falling back to its default for missing or invalid saved values.
    pub fn get(&self, id: WorldOption) -> i32 {
        self.0
            .get(&id)
            .copied()
            .filter(|value| id.accepts(*value))
            .unwrap_or(id.definition().default)
    }

    /// Reads a toggle as a boolean.
    pub fn enabled(&self, id: WorldOption) -> bool {
        self.get(id) != 0
    }

    /// Changes a valid value and enables cheats when changing a gated option.
    pub fn set(&mut self, id: WorldOption, value: i32) -> bool {
        if !id.accepts(value) {
            return false;
        }
        if id.definition().cheats && value != self.get(id) {
            self.0.insert(WorldOption::ActivateCheats, 1);
        }
        if id == WorldOption::ActivateCheats && value == 0 {
            self.0.retain(|id, _| !id.definition().cheats);
        }
        if id == WorldOption::AlwaysDay && value == 1 {
            self.0.insert(WorldOption::DaylightCycle, 0);
        }
        if id == WorldOption::DaylightCycle && value == 1 {
            self.0.insert(WorldOption::AlwaysDay, 0);
        }
        self.0.insert(id, value);
        true
    }

    /// Restores the recorded cheats flag without changing existing world rules.
    pub fn restore_cheats(&mut self, enabled: bool) {
        self.0
            .insert(WorldOption::ActivateCheats, i32::from(enabled));
    }

    /// Whether a control can be edited under the world's current dependencies.
    pub fn editable(&self, id: WorldOption, editing: bool) -> bool {
        !(editing && id.definition().creation_only)
            && match id {
                WorldOption::LanVisible => self.enabled(WorldOption::Multiplayer),
                WorldOption::SleepPercentage => self.enabled(WorldOption::SleepSkipNight),
                _ => true,
            }
    }

    /// Whether local multiplayer and its LAN discovery are both enabled.
    pub fn lan_visible(&self) -> bool {
        self.enabled(WorldOption::Multiplayer) && self.enabled(WorldOption::LanVisible)
    }
}

impl WorldOption {
    /// Checks the shared bounds before a value reaches a form or server.
    pub fn accepts(self, value: i32) -> bool {
        match self.definition().control {
            Toggle => matches!(value, 0 | 1),
            Number { min, max, step } => (min..=max).contains(&value) && (value - min) % step == 0,
        }
    }
}

#[cfg(test)]
mod tests;
