use std::time::{Duration, Instant};

use protocol::{ActorBlockSyncMessage, ActorKind};

use super::ActorStore;

pub(super) const FALLING_VISIBILITY_FALLBACK: Duration = Duration::from_millis(150);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum TerrainInterlock {
    Pending {
        attached_at: Instant,
    },
    #[default]
    Visible,
    Hidden,
}

impl TerrainInterlock {
    pub(super) fn attached(kind: &ActorKind, now: Instant) -> Self {
        if matches!(kind, ActorKind::Entity { identifier } if identifier.as_ref() == "minecraft:falling_block")
        {
            Self::Pending { attached_at: now }
        } else {
            Self::Visible
        }
    }

    pub(super) fn visible_at(self, now: Instant) -> bool {
        match self {
            Self::Pending { attached_at } => {
                now.saturating_duration_since(attached_at) > FALLING_VISIBILITY_FALLBACK
            }
            Self::Visible => true,
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
