//! World settings the host changes from Settings while playing their own local world.

use super::options::{MEMBER, OPERATOR, Storage, WorldOption};
use super::world::{Difficulty, World, WorldUpdate};
use bridge::{Backend, GameMode};

mod edits;
pub(crate) use edits::Edits;

/// When a change made while the world runs takes effect.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Timing {
    /// The running server applies it at once.
    Live,
    /// Saved now; the server reads it when the world next starts.
    NextStart,
    /// Fixed once the world exists.
    Locked,
}

/// When a default game mode change takes effect: the dedicated server has no console command for
/// it, so it applies on the next start.
pub const GAME_MODE_TIMING: Timing = Timing::NextStart;

/// When a change to `id` takes effect in the running world. Vanilla also locks simulation
/// distance once the world is open.
pub fn timing(id: WorldOption) -> Timing {
    let definition = id.definition();
    if definition.creation_only || id == WorldOption::SimulationDistance {
        return Timing::Locked;
    }
    match (id, definition.storage) {
        (WorldOption::ActivateCheats, _) | (_, Storage::Rule(_) | Storage::Combined) => {
            Timing::Live
        }
        (_, Storage::Level(_) | Storage::Property(..)) => Timing::NextStart,
    }
}

/// One row of the in-game Game section.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Row {
    /// A group heading, by lang key.
    Heading(&'static str),
    Name,
    GameMode,
    Hardcore,
    Difficulty,
    WorldType,
    Seed,
    /// A catalog option with its vanilla label and description lang keys.
    Option(WorldOption, &'static str, &'static str),
}

impl Row {
    /// The row's vanilla label lang key.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Heading(key) | Self::Option(_, key, _) => key,
            Self::Name => "selectWorld.enterName",
            Self::GameMode => "createWorldScreen.gameMode.default",
            Self::Hardcore => "createWorldScreen.isHardcore",
            Self::Difficulty => "options.difficulty",
            Self::WorldType => "createWorldScreen.worldType",
            Self::Seed => "createWorldScreen.levelSeed",
        }
    }
}

/// Vanilla's in-game Game section in its order; experiments and education settings are left out.
pub const GAME_SECTION: &[Row] = {
    use WorldOption as O;
    const fn option(id: WorldOption, label: &'static str) -> Row {
        Row::Option(id, label, "")
    }
    &[
        Row::Heading("createWorldScreen.game.settings"),
        Row::Name,
        Row::GameMode,
        Row::Hardcore,
        Row::Difficulty,
        Row::Heading("createWorldScreen.worldPreferences"),
        option(O::StartingMap, "createWorldScreen.startWithMap"),
        option(O::BonusChest, "createWorldScreen.bonusChest"),
        option(O::PlayerPermissions, "permissions.title.settings"),
        Row::WorldType,
        Row::Seed,
        option(O::SimulationDistance, "createWorldScreen.serverSimDistance"),
        Row::Heading("createWorldScreen.worldOptions"),
        option(O::Pvp, "createWorldScreen.pvp"),
        option(O::ShowCoordinates, "createWorldScreen.showCoordinates"),
        option(O::PlayerWaypoints, "createWorldScreen.playerWaypoints"),
        option(O::ShowDaysPlayed, "createWorldScreen.showDaysPlayed"),
        option(O::FireSpreads, "createWorldScreen.fireSpreads"),
        option(O::RecipesUnlock, "createWorldScreen.recipesUnlock"),
        option(O::TntExplodes, "createWorldScreen.tntexplodes"),
        option(
            O::RespawnBlocksExplode,
            "createWorldScreen.respawnblocksexplode",
        ),
        option(O::MobLoot, "createWorldScreen.mobLoot"),
        option(
            O::NaturalRegeneration,
            "createWorldScreen.naturalregeneration",
        ),
        option(O::TileDrops, "createWorldScreen.tileDrops"),
        option(O::SleepSkipNight, "createWorldScreen.playerSleep"),
        Row::Option(
            O::SleepPercentage,
            "createWorldScreen.playerSleepPercentage",
            "createWorldScreen.playerSleepPercentageDescription",
        ),
        option(O::ImmediateRespawn, "createWorldScreen.immediaterespawn"),
        option(O::RespawnRadius, "createWorldScreen.respawnRadius"),
        Row::Heading("createWorldScreen.cheatSettings"),
        option(O::ActivateCheats, "selectWorld.cheats"),
        option(O::AlwaysDay, "createWorldScreen.alwaysDay"),
        option(O::DaylightCycle, "createWorldScreen.daylightcycle"),
        option(O::KeepInventory, "createWorldScreen.keepInventory"),
        option(O::MobSpawning, "createWorldScreen.mobSpawn"),
        option(O::MobGriefing, "createWorldScreen.mobgriefing"),
        option(O::EntitiesDropLoot, "createWorldScreen.entitiesdroploot"),
        option(O::WeatherCycle, "createWorldScreen.weathercycle"),
        option(O::CommandBlocks, "createWorldScreen.commandblocksenabled"),
        option(O::RandomTickSpeed, "createWorldScreen.randomtickspeed"),
    ]
};

/// Catalog options vanilla's Game section does not show.
pub const OUTSIDE_GAME_SECTION: &[WorldOption] =
    &[WorldOption::LimitedCrafting, WorldOption::Multiplayer];

/// A press in the in-game world settings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    Option(WorldOption, i32),
    GameMode(GameMode),
    Difficulty(Difficulty),
    /// Focuses a numeric option's text box.
    OptionField(WorldOption),
}

/// A world as an in-game change saved it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Applied {
    pub world: World,
    /// The running server did not take the change, which then applies on the next start.
    pub missed: bool,
}

/// The local world the player is in, as its in-game settings show it.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub world: World,
    /// A change this session saved but the running server did not take.
    pub missed: bool,
    /// A metadata save failed; the controls show the last confirmed values again.
    pub error: Option<String>,
    /// Numeric options' text as typed while their box has focus.
    pub option_text: std::collections::BTreeMap<WorldOption, String>,
}

impl View {
    /// The in-game settings of `world`; only dedicated-server worlds with readable options have them.
    pub fn of(world: &World) -> Option<Self> {
        (world.backend == Backend::Bds && !world.options_unavailable).then(|| Self {
            world: world.clone(),
            missed: false,
            error: None,
            option_text: Default::default(),
        })
    }

    /// A numeric option's text as typed, or its current value.
    pub fn numeric_text(&self, id: WorldOption) -> String {
        self.option_text
            .get(&id)
            .cloned()
            .unwrap_or_else(|| self.world.options.get(id).to_string())
    }

    /// Whether the control for `id` accepts changes in the running world; cheat rules wait
    /// for cheats.
    pub fn editable(&self, id: WorldOption) -> bool {
        timing(id) != Timing::Locked
            && self.world.options.editable(id, true)
            && (!id.definition().cheats || self.cheats())
    }

    pub fn cheats(&self) -> bool {
        self.world.options.enabled(WorldOption::ActivateCheats)
    }
}

/// The update `action` makes to `world`; `None` when it changes nothing or cannot change.
pub fn update(world: &World, action: Action) -> Option<WorldUpdate> {
    let update = match action {
        Action::GameMode(mode) => {
            // A creative default turns cheats on with it.
            let mut options = world.options.clone();
            if mode == GameMode::Creative {
                options.restore_cheats(true);
            }
            let changes = options.changes_from(&world.options);
            WorldUpdate {
                game_mode: (mode != world.game_mode).then_some(mode),
                options: (!changes.is_empty()).then_some(changes),
                ..Default::default()
            }
        }
        Action::Difficulty(difficulty) => WorldUpdate {
            difficulty: (difficulty != world.difficulty).then_some(difficulty),
            ..Default::default()
        },
        Action::Option(id, value) => {
            let view = View::of(world)?;
            if !view.editable(id) {
                return None;
            }
            let mut options = world.options.clone();
            if id == WorldOption::ActivateCheats {
                // Rules keep their values, and an operator default drops to member without cheats.
                if !id.accepts(value) {
                    return None;
                }
                options.restore_cheats(value != 0);
                if value == 0 && options.get(WorldOption::PlayerPermissions) == OPERATOR {
                    options.set(WorldOption::PlayerPermissions, MEMBER);
                }
            } else if !options.set(id, value) {
                return None;
            }
            let changes = options.changes_from(&world.options);
            WorldUpdate {
                options: (!changes.is_empty()).then_some(changes),
                ..Default::default()
            }
        }
        Action::OptionField(_) => return None,
    };
    (update != WorldUpdate::default()).then_some(update)
}

/// Applies `update` to `world` as the store saves it.
pub fn apply(world: &mut World, update: &WorldUpdate) {
    if let Some(name) = &update.name {
        world.name.clone_from(name);
    }
    if let Some(mode) = update.game_mode {
        world.game_mode = mode;
    }
    if let Some(difficulty) = update.difficulty {
        world.difficulty = difficulty;
    }
    if let Some(changes) = &update.options {
        world.options.apply_changes(changes);
    }
}

#[cfg(test)]
mod tests;
