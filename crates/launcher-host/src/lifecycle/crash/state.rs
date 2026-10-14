//! What this process was doing, for crash reports: its renderer attempt and game state.
//!
//! Owners publish here only when their state changes; each change is pushed to the crash
//! monitor at once, so a crashing process never has to gather it.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard, PoisonError},
};

use crate::renderer_startup::Attempt;

/// The `game_state` key for [`SessionPhase`].
pub const SESSION_PHASE: &str = "session_phase";
/// The `game_state` key for [`ServerKind`].
pub const SERVER_KIND: &str = "server_kind";
/// The `game_state` key for the current dimension.
pub const DIMENSION: &str = "dimension";
/// The `game_state` key for the number of resource packs the server applied.
pub const SERVER_PACKS: &str = "server_packs";

/// Where the player is.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SessionPhase {
    #[default]
    Menus,
    Joining,
    InWorld,
}

impl SessionPhase {
    /// The value recorded under [`SESSION_PHASE`].
    pub const fn name(self) -> &'static str {
        match self {
            Self::Menus => "menus",
            Self::Joining => "joining",
            Self::InWorld => "in_world",
        }
    }
}

/// The kind of destination, never its address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
    pub const fn name(self) -> &'static str {
        match self {
            Self::Server => "server",
            Self::FeaturedServer => "featured_server",
            Self::Realm => "realm",
            Self::FriendWorld => "friend_world",
            Self::Experience => "experience",
            Self::LocalWorld => "local_world",
        }
    }
}

/// The game's state as crash reports record it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GameState {
    pub phase: SessionPhase,
    /// The destination while joining or in a world.
    pub server: Option<ServerKind>,
    /// Bedrock's dimension id while in a world: 0 overworld, 1 nether, 2 end.
    pub dimension: Option<i32>,
    /// Resource packs the server applied this session.
    pub server_packs: usize,
}

impl GameState {
    /// The report's `game_state` entries.
    pub fn entries(&self) -> BTreeMap<String, String> {
        let mut entries =
            BTreeMap::from([(SESSION_PHASE.to_owned(), self.phase.name().to_owned())]);
        if let Some(server) = self.server {
            entries.insert(SERVER_KIND.to_owned(), server.name().to_owned());
        }
        if let Some(dimension) = self.dimension {
            let name = match dimension {
                0 => "overworld".to_owned(),
                1 => "nether".to_owned(),
                2 => "the_end".to_owned(),
                other => other.to_string(),
            };
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
