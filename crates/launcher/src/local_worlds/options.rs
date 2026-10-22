//! Shared metadata and values for local-world options.

use super::Tab;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The control and accepted values of an option.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Control {
    Toggle,
    Choice(&'static [Choice]),
    Number { min: i32, max: i32 },
}

/// One supported value of a named world option.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Choice {
    pub label: &'static str,
    pub value: i32,
    /// The value's name in server properties or game rule commands.
    pub wire: &'static str,
}
/// Default player permission values of [`WorldOption::PlayerPermissions`].
pub const MEMBER: i32 = 1;
pub const OPERATOR: i32 = 2;
const PERMISSIONS: &[Choice] = &[
    Choice {
        label: "Visitor",
        value: 0,
        wire: "visitor",
    },
    Choice {
        label: "Member",
        value: MEMBER,
        wire: "member",
    },
    Choice {
        label: "Operator",
        value: OPERATOR,
        wire: "operator",
    },
];
const WAYPOINTS: &[Choice] = &[
    Choice {
        label: "Off",
        value: 0,
        wire: "off",
    },
    Choice {
        label: "On",
        value: 1,
        wire: "everyone",
    },
];

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
        #[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
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
    SleepPercentage: "Players sleeping percentage", Advanced, 100, Number { min: 0, max: 100 }, false, Rule("playerssleepingpercentage"), false;
    RespawnRadius: "Respawn radius", Advanced, 10, Number { min: 0, max: 128 }, false, Rule("spawnradius"), false;
    SimulationDistance: "Simulation distance", Advanced, 4, Number { min: 4, max: 12 }, false, Property("tick-distance", "serverChunkTickRange"), false;
    Multiplayer: "Multiplayer game", Multiplayer, 1, Toggle, false, Level("MultiplayerGame"), false;
    PlayerPermissions: "Default player permission", Multiplayer, 1, Control::Choice(PERMISSIONS), false, Property("default-player-permission-level", "playerPermissionsLevel"), false;
    PlayerWaypoints: "Player waypoints", Multiplayer, 1, Control::Choice(WAYPOINTS), false, Rule("playerwaypoints"), false;
    ActivateCheats: "Activate cheats", Cheats, 0, Toggle, false, Property("allow-cheats", "cheatsEnabled"), false;
    AlwaysDay: "Always day", Cheats, 0, Toggle, true, Combined, false;
    DaylightCycle: "Daylight cycle", Cheats, 1, Toggle, true, Rule("dodaylightcycle"), false;
    KeepInventory: "Keep inventory", Cheats, 0, Toggle, true, Rule("keepinventory"), false;
    MobSpawning: "Mob spawning", Cheats, 1, Toggle, true, Rule("domobspawning"), false;
    MobGriefing: "Mob griefing", Cheats, 1, Toggle, true, Rule("mobgriefing"), false;
    EntitiesDropLoot: "Entities drop loot", Cheats, 1, Toggle, true, Rule("doentitydrops"), false;
    WeatherCycle: "Weather cycle", Cheats, 1, Toggle, true, Rule("doweathercycle"), false;
    CommandBlocks: "Command blocks enabled", Cheats, 1, Toggle, true, Rule("commandblocksenabled"), false;
    RandomTickSpeed: "Random tick speed", Cheats, 1, Number { min: 0, max: 4096 }, true, Rule("randomtickspeed"), false;
}

/// Saved overrides; missing options inherit their defaults for older worlds.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
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
                WorldOption::SleepPercentage => self.enabled(WorldOption::SleepSkipNight),
                _ => true,
            }
    }
}

impl WorldOption {
    /// Whether changing this toggle resets `other` ([`WorldOptions::set`]) or decides whether
    /// it can be edited ([`WorldOptions::editable`]).
    pub fn gates(self, other: WorldOption) -> bool {
        match self {
            WorldOption::ActivateCheats => other.definition().cheats,
            WorldOption::SleepSkipNight => other == WorldOption::SleepPercentage,
            _ => false,
        }
    }

    /// Checks the shared bounds before a value reaches a form or server.
    pub fn accepts(self, value: i32) -> bool {
        match self.definition().control {
            Toggle => matches!(value, 0 | 1),
            Control::Choice(choices) => choices.iter().any(|choice| choice.value == value),
            Number { min, max } => (min..=max).contains(&value),
        }
    }
}

#[cfg(test)]
mod tests;

impl PartialEq for WorldOptions {
    /// Compares effective values, treating omitted overrides as their defaults.
    fn eq(&self, other: &Self) -> bool {
        WorldOption::ALL
            .iter()
            .all(|&id| self.get(id) == other.get(id))
    }
}
impl Eq for WorldOptions {}

/// Individual menu edits, including explicit changes back to a default value.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct OptionChanges(BTreeMap<WorldOption, i32>);

impl OptionChanges {
    /// Whether the menu changed any effective value.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// Adds newer edits without discarding unrelated pending changes.
    pub fn extend(&mut self, newer: &Self) {
        self.0
            .extend(newer.0.iter().map(|(&id, &value)| (id, value)));
    }
    /// Returns an explicitly edited value.
    pub fn get(&self, id: WorldOption) -> Option<i32> {
        self.0.get(&id).copied()
    }
    /// The edits to options `keep` accepts.
    pub fn filtered(&self, keep: impl Fn(WorldOption) -> bool) -> Self {
        Self(
            self.0
                .iter()
                .filter(|(id, _)| keep(**id))
                .map(|(&id, &value)| (id, value))
                .collect(),
        )
    }
    /// Drops edits identical to `applied`, keeping any made since it was captured.
    pub fn acknowledge(&mut self, applied: &Self) {
        self.0.retain(|id, value| applied.0.get(id) != Some(value));
    }
}

impl WorldOptions {
    /// Captures only values changed since a form was opened.
    pub fn changes_from(&self, previous: &Self) -> OptionChanges {
        OptionChanges(
            WorldOption::ALL
                .iter()
                .filter_map(|&id| {
                    let value = self.get(id);
                    (value != previous.get(id)).then_some((id, value))
                })
                .collect(),
        )
    }
    /// Applies recorded menu edits without re-running toggle side effects.
    pub fn apply_changes(&mut self, changes: &OptionChanges) {
        for (&id, &value) in &changes.0 {
            if id.accepts(value) {
                self.0.insert(id, value);
            }
        }
    }
    /// Refreshes active game rules while retaining launcher preferences and inactive values.
    pub fn merge_server_rules(&mut self, saved: &Self) {
        for &id in WorldOption::ALL {
            if matches!(
                id.definition().storage,
                Storage::Rule(_) | Storage::Combined
            ) && (id != WorldOption::SleepPercentage
                || saved.enabled(WorldOption::SleepSkipNight))
            {
                self.0.insert(id, saved.get(id));
            }
        }
    }
}
