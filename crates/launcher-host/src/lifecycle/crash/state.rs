//! What this process was doing, for crash reports: its renderer attempt and game state.
//!
//! Owners publish here only when their state changes; each change is pushed to the crash
//! monitor at once, so a crashing process never has to gather it.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard, PoisonError},
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::renderer_startup::Attempt;

/// The `game_state` key for [`SessionPhase`].
pub const SESSION_PHASE: &str = "session_phase";
/// The `game_state` key for [`ServerKind`].
pub const SERVER_KIND: &str = "server_kind";
/// The `game_state` key for the current [`Dimension`].
pub const DIMENSION: &str = "dimension";
/// The `game_state` key for the number of resource packs the server applied.
pub const SERVER_PACKS: &str = "server_packs";

/// Where the player is.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionPhase {
    #[default]
    Menus,
    Joining,
    InWorld,
}

impl SessionPhase {
    /// The value recorded under [`SESSION_PHASE`].
    pub fn name(self) -> String {
        recorded(self)
    }

    /// The phase recorded as `name`.
    pub fn from_name(name: &str) -> Option<Self> {
        parse(name)
    }
}

/// The kind of destination, never its address.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerKind {
    Server,
    FeaturedServer,
    Realm,
    FriendWorld,
    Experience,
    LocalWorld,
}

impl ServerKind {
    /// The value recorded under [`SERVER_KIND`].
    pub fn name(self) -> String {
        recorded(self)
    }

    /// The kind recorded as `name`.
    pub fn from_name(name: &str) -> Option<Self> {
        parse(name)
    }
}

/// A dimension Bedrock names by id.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dimension {
    Overworld,
    Nether,
    TheEnd,
}

impl Dimension {
    /// The dimension with Bedrock's id `id`.
    pub const fn from_id(id: i32) -> Option<Self> {
        match id {
            0 => Some(Self::Overworld),
            1 => Some(Self::Nether),
            2 => Some(Self::TheEnd),
            _ => None,
        }
    }

    /// The dimension recorded as `name` under [`DIMENSION`].
    pub fn from_name(name: &str) -> Option<Self> {
        parse(name)
    }
}

/// How `value` is recorded in a report: its serde name, so [`parse`] always reads it back.
fn recorded(value: impl Serialize) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(name)) => name,
        other => unreachable!("state enums serialize as names, not {other:?}"),
    }
}

fn parse<T: DeserializeOwned>(name: &str) -> Option<T> {
    serde_json::from_value(serde_json::Value::String(name.to_owned())).ok()
}

/// The game's state as crash reports record it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GameState {
    pub phase: SessionPhase,
    /// The destination while joining or in a world.
    pub server: Option<ServerKind>,
    /// Bedrock's dimension id while in a world; see [`Dimension::from_id`].
    pub dimension: Option<i32>,
    /// Resource packs the server applied this session.
    pub server_packs: usize,
}

impl GameState {
    /// The report's `game_state` entries.
    pub fn entries(&self) -> BTreeMap<String, String> {
        let mut entries = BTreeMap::from([(SESSION_PHASE.to_owned(), self.phase.name())]);
        if let Some(server) = self.server {
            entries.insert(SERVER_KIND.to_owned(), server.name());
        }
        if let Some(id) = self.dimension {
            let name = Dimension::from_id(id).map_or_else(|| id.to_string(), recorded);
            entries.insert(DIMENSION.to_owned(), name);
        }
        if self.phase != SessionPhase::Menus {
            entries.insert(SERVER_PACKS.to_owned(), self.server_packs.to_string());
        }
        entries
    }
}

#[derive(Default)]
struct State {
    /// Renderer attempts by data root, so concurrent tests and clients attribute only their own.
    renderers: BTreeMap<PathBuf, Attempt>,
    game: GameState,
}

static STATE: Mutex<State> = Mutex::new(State {
    renderers: BTreeMap::new(),
    game: GameState {
        phase: SessionPhase::Menus,
        server: None,
        dimension: None,
        server_packs: 0,
    },
});

fn lock() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Records this process's pending or completed renderer attempt under `root`, or clears it.
pub(crate) fn set_renderer(root: &Path, attempt: Option<&Attempt>) {
    {
        let mut state = lock();
        if let Some(attempt) = attempt {
            state.renderers.insert(root.to_owned(), attempt.clone());
        } else {
            state.renderers.remove(root);
        }
    }
    super::state_changed();
}

/// Publishes the game's state; unchanged state sends nothing.
pub fn publish(game: GameState) {
    {
        let mut state = lock();
        if state.game == game {
            return;
        }
        state.game = game;
    }
    super::state_changed();
}

/// This process's renderer attempt under `root`.
pub(crate) fn renderer(root: &Path) -> Option<Attempt> {
    lock().renderers.get(root).cloned()
}

/// The game state last published.
pub(crate) fn game() -> GameState {
    lock().game
}

/// [`renderer`] and [`game`] without waiting: `None` while another thread holds the state, as
/// a faulting one might.
#[cfg(windows)]
pub(crate) fn try_snapshot(root: &Path) -> Option<(Option<Attempt>, GameState)> {
    let state = STATE.try_lock().ok()?;
    Some((state.renderers.get(root).cloned(), state.game))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_state_records_kinds_without_addresses() {
        let menus = GameState::default().entries();
        assert_eq!(
            menus,
            BTreeMap::from([(SESSION_PHASE.to_owned(), "menus".to_owned())])
        );
        let playing = GameState {
            phase: SessionPhase::InWorld,
            server: Some(ServerKind::Realm),
            dimension: Some(1),
            server_packs: 3,
        }
        .entries();
        assert_eq!(playing[SESSION_PHASE], "in_world");
        assert_eq!(playing[SERVER_KIND], "realm");
        assert_eq!(playing[DIMENSION], "nether");
        assert_eq!(playing[SERVER_PACKS], "3");
    }

    #[test]
    fn recorded_names_read_back() {
        let state = GameState {
            phase: SessionPhase::Joining,
            server: Some(ServerKind::FriendWorld),
            dimension: Some(2),
            server_packs: 0,
        };
        let entries = state.entries();
        assert_eq!(
            SessionPhase::from_name(&entries[SESSION_PHASE]),
            Some(SessionPhase::Joining)
        );
        assert_eq!(
            ServerKind::from_name(&entries[SERVER_KIND]),
            Some(ServerKind::FriendWorld)
        );
        assert_eq!(
            Dimension::from_name(&entries[DIMENSION]),
            Some(Dimension::TheEnd)
        );
        assert_eq!(ServerKind::from_name("lan"), None);
        let unknown = GameState {
            dimension: Some(7),
            ..state
        };
        assert_eq!(unknown.entries()[DIMENSION], "7");
    }

    #[test]
    fn renderer_attempts_belong_to_their_data_root() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let attempt = Attempt {
            backend: launcher::graphics_backend::Backend::Metal,
            adapter: None,
            stage: crate::renderer_startup::Stage::Ready,
            explicit: false,
        };
        set_renderer(first.path(), Some(&attempt));
        assert_eq!(renderer(first.path()), Some(attempt));
        assert_eq!(renderer(second.path()), None);
        set_renderer(first.path(), None);
        assert_eq!(renderer(first.path()), None);
    }
}
