use std::time::Duration;
use web_time::Instant;

use assets::facts::entity::NativeEntityClass;
use protocol::ActorBlockSyncMessage;

use super::ActorStore;

pub(super) const FALLING_VISIBILITY_FALLBACK: Duration = Duration::from_millis(150);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum TerrainInterlock {
    /// No terrain gate or explicit sync has been applied to this constructor yet.
    InitialVisible {
        attached_at: Instant,
    },
    Pending {
        attached_at: Instant,
    },
    #[default]
    Visible,
    Hidden,
}

impl TerrainInterlock {
    pub(super) fn attached(class: NativeEntityClass, now: Instant) -> Self {
        if class == NativeEntityClass::FallingBlock {
            Self::Pending { attached_at: now }
        } else {
            Self::InitialVisible { attached_at: now }
        }
    }

    /// Adds a newly admitted constructor's gate while preserving received sync state.
    pub(super) fn admit_class(&mut self, class: NativeEntityClass) {
        if let Self::InitialVisible { attached_at } = *self {
            *self = Self::attached(class, attached_at);
        }
    }

    pub(super) fn visible_at(self, now: Instant) -> bool {
        match self {
            Self::Pending { attached_at } => {
                now.saturating_duration_since(attached_at) > FALLING_VISIBILITY_FALLBACK
            }
            Self::InitialVisible { .. } | Self::Visible => true,
            Self::Hidden => false,
        }
    }
}

impl ActorStore {
    pub(crate) fn apply_terrain_sync(&mut self, sync: ActorBlockSyncMessage) -> bool {
        if sync.actor_unique_id == -1 {
            return false;
        }
        let state = match sync.message {
            1 => TerrainInterlock::Visible,
            2 => TerrainInterlock::Hidden,
            _ => return false,
        };
        let Some(actor) = self
            .unique_to_runtime
            .get(&sync.actor_unique_id)
            .and_then(|runtime_id| self.actors.get_mut(runtime_id))
        else {
            return false;
        };
        actor.status.terrain_interlock = state;
        true
    }
}
