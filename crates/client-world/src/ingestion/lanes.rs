//! Commit lanes: which earlier world events a later one must not overtake.
//!
//! Each admitted event carries a footprint. A later event commits as soon as no earlier
//! unfinished event conflicts with it, so final state equals strict wire-order application.

use protocol::{ActorEvent, AudioEvent, ItemActorEvent, ParticleEvent, UiEvent, WorldEvent};
use world::ChunkKey;

/// Ordered consumers an event writes; two events sharing one keep wire order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Consumers(u8);

impl Consumers {
    pub const NONE: Self = Self(0);
    /// The actor store, which rejects any sequence older than its newest.
    pub const ACTORS: Self = Self(1);
    pub const CONTROLS: Self = Self(1 << 1);
    pub const UI: Self = Self(1 << 2);
    pub const AUDIO: Self = Self(1 << 3);
    pub const CAMERA: Self = Self(1 << 4);
    pub const PARTICLES: Self = Self(1 << 5);
    pub const SHAPES: Self = Self(1 << 6);
    /// App-owned inventory and equipment stores advanced by the commit frontier.
    pub const INVENTORY: Self = Self(1 << 7);

    #[cfg(test)]
    pub(crate) const fn from_bit(bit: u8) -> Self {
        Self(1 << bit)
    }

    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// What one admitted event reads and writes, for out-of-order commit decisions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Footprint {
    /// Conflicts with every earlier and later event.
    pub barrier: bool,
    /// Changes which columns are retained, so it conflicts with every terrain event and
    /// every other retention change.
    pub retention: bool,
    /// Moves the server position, which scopes retention while local physics does not.
    pub positional: bool,
    /// Waits for earlier block mutations, never for chunk decode.
    pub local_authority: bool,
    /// Rewrites resident blocks that local authority must observe first.
    pub mutation: bool,
    /// Spends heavy admission and the terrain commit budget.
    pub heavy: bool,
    pub consumers: Consumers,
    /// Sorted, deduplicated terrain columns read or written.
    pub columns: Vec<ChunkKey>,
}

impl Footprint {
    #[must_use]
    pub fn barrier() -> Self {
        Self {
            barrier: true,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn session(consumers: Consumers) -> Self {
        Self {
            consumers,
            ..Self::default()
        }
    }

    /// An app-owned inventory or equipment commit.
    #[must_use]
    pub fn inventory() -> Self {
        Self {
            local_authority: true,
            consumers: Consumers::INVENTORY.with(Consumers::UI),
            ..Self::default()
        }
    }

    #[must_use]
    pub fn terrain(columns: impl IntoIterator<Item = ChunkKey>, heavy: bool) -> Self {
        let mut columns = columns.into_iter().collect::<Vec<_>>();
        columns.sort_unstable();
        columns.dedup();
        Self {
            heavy,
            columns,
            ..Self::default()
        }
    }

    /// Pure chunk data neither inventory nor local authority waits for.
    #[must_use]
    pub fn is_chunk_data(&self) -> bool {
        !self.columns.is_empty()
            && !self.barrier
            && !self.retention
            && !self.mutation
            && self.consumers.is_empty()
    }
}

/// Local identities that decide whether an event addresses the local player.
#[derive(Debug, Clone, Copy)]
pub struct LaneContext {
    pub local_runtime_id: u64,
    pub local_unique_id: i64,
    pub dimension: i32,
}

/// Classifies one wire event. Whatever local physics or the local player's stores consume
/// waits for earlier block mutations; session-wide definitions are barriers.
#[must_use]
pub fn classify(event: &WorldEvent, context: LaneContext) -> Footprint {
    let mut footprint = classify_event(event, context);
    footprint.local_authority |= footprint
        .consumers
        .intersects(Consumers::CONTROLS.with(Consumers::INVENTORY));
    footprint
}

fn classify_event(event: &WorldEvent, context: LaneContext) -> Footprint {
    use Consumers as C;
    let local = |runtime_id: u64| runtime_id == context.local_runtime_id;
    let column = |dimension: i32, position: [i32; 3]| {
        ChunkKey::new(
            dimension,
            position[0].div_euclid(16),
            position[2].div_euclid(16),
        )
    };
    match event {
        WorldEvent::DimensionHeights(_)
        | WorldEvent::BiomeDefinitions(_)
        | WorldEvent::ChangeDimension(_) => Footprint::barrier(),
        WorldEvent::ChunkRadiusUpdated(_) | WorldEvent::PublisherUpdate(_) => Footprint {
            retention: true,
            ..Footprint::session(C::NONE)
        },
        WorldEvent::Respawn(_) => Footprint {
            retention: true,
            ..Footprint::session(C::CONTROLS)
        },
        WorldEvent::MovePlayer(movement) if movement.mode.is_teleport() => Footprint {
            retention: true,
            ..Footprint::session(C::CONTROLS.with(C::ACTORS))
        },
        WorldEvent::MovePlayer(_) => Footprint {
            positional: true,
            local_authority: true,
            ..Footprint::session(C::CONTROLS.with(C::ACTORS))
        },
        WorldEvent::PlayerMovementCorrection(correction) if correction.subject.is_player() => {
            Footprint {
                positional: true,
                local_authority: true,
                ..Footprint::session(C::CONTROLS)
            }
        }
        WorldEvent::PlayerMovementCorrection(_) => Footprint::session(C::NONE),
        WorldEvent::LevelChunk(event) => {
            Footprint::terrain([ChunkKey::new(event.dimension, event.x, event.z)], true)
        }
        WorldEvent::ChunkResync(event) => {
            Footprint::terrain([ChunkKey::new(event.dimension, event.x, event.z)], true)
        }
        WorldEvent::SubChunks(batch) => Footprint::terrain(
            batch
                .entries
                .iter()
                .map(|entry| ChunkKey::new(batch.dimension, entry.position[0], entry.position[2])),
            true,
        ),
        WorldEvent::SubChunkReplyAdmission(admission) => Footprint::terrain(
            admission
                .positions
                .iter()
                .map(|position| ChunkKey::new(admission.dimension, position[0], position[2])),
            false,
        ),
        WorldEvent::BlockUpdates(updates) => Footprint {
            mutation: true,
            ..Footprint::terrain(
                updates
                    .iter()
                    .map(|update| column(update.dimension, update.position)),
                true,
            )
        },
        // Actor/terrain transitions resolve their actor once the terrain is shown.
        WorldEvent::SyncedBlockUpdates(updates) => Footprint {
            mutation: true,
            consumers: if updates
                .iter()
                .any(|update| update.sync.actor_unique_id != -1 && update.sync.message != 0)
            {
                C::ACTORS
            } else {
                C::NONE
            },
            ..Footprint::terrain(
                updates
                    .iter()
                    .map(|update| column(update.update.dimension, update.update.position)),
                true,
            )
        },
        WorldEvent::BlockEntityUpdate(update) => {
            Footprint::terrain([column(update.dimension, update.position)], true)
        }
        WorldEvent::BlockEvent(event) => {
            Footprint::terrain([column(event.dimension, event.position)], false)
        }
        WorldEvent::BlockCrack(event) => Footprint {
            consumers: C::UI,
            ..Footprint::terrain([column(context.dimension, event.position)], false)
        },
        WorldEvent::OpenSign(event) => Footprint {
            consumers: C::UI,
            ..Footprint::terrain([column(event.dimension, event.position)], false)
        },
        WorldEvent::NetworkStackLatency(_) => Footprint {
            local_authority: true,
            ..Footprint::session(C::CONTROLS)
        },
        WorldEvent::ActorMotion(motion) if local(motion.actor_runtime_id) => Footprint {
            local_authority: true,
            ..Footprint::session(C::ACTORS.with(C::CONTROLS))
        },
        WorldEvent::ActorMotion(_) => Footprint::session(C::ACTORS),
        WorldEvent::MovementEffect(event) if local(event.actor_runtime_id) => {
            Footprint::session(C::CONTROLS)
        }
        WorldEvent::MovementEffect(_) => Footprint::session(C::NONE),
        WorldEvent::DimensionChangeAck { .. }
        | WorldEvent::SetTime(_)
        | WorldEvent::WorldClocks(_)
        | WorldEvent::Weather(_) => Footprint::session(C::CONTROLS),
        WorldEvent::GameRules(_) => Footprint::session(C::CONTROLS.with(C::UI)),
        // Positional sounds may wait in the actor store for their actor's synchronization.
        WorldEvent::Audio(AudioEvent::Level(level)) if level.fire_at_position.is_some() => {
            Footprint::session(C::AUDIO.with(C::ACTORS))
        }
        // Level-event sounds such as record playback resolve items through the actor
        // store's item registry when consumed.
        WorldEvent::Audio(AudioEvent::LevelEvent(_)) => {
            Footprint::session(C::AUDIO.with(C::ACTORS))
        }
        WorldEvent::Audio(_) => Footprint::session(C::AUDIO),
        // Camera switches, targets and attachments resolve actors when consumed.
        WorldEvent::Camera(_) => Footprint::session(C::CAMERA.with(C::AUDIO).with(C::ACTORS)),
        WorldEvent::PrimitiveShapes(_) => Footprint::session(C::SHAPES),
        WorldEvent::Actor(event) => Footprint::session(actor_consumers(event, context)),
        WorldEvent::ActorEffect(event) if local(event.actor_runtime_id) => {
            Footprint::session(C::CONTROLS.with(C::UI))
        }
        WorldEvent::ActorEffect(_) => Footprint::session(C::NONE),
        WorldEvent::Abilities(event) if event.actor_unique_id == context.local_unique_id => {
            Footprint {
                local_authority: true,
                ..Footprint::session(C::UI)
            }
        }
        WorldEvent::Abilities(_) => Footprint::session(C::NONE),
        WorldEvent::ArmorEquipment(_)
        | WorldEvent::ActorPropertySync(_)
        | WorldEvent::Equipment(_) => Footprint::session(C::ACTORS),
        WorldEvent::ActorLink(_) => Footprint::session(C::ACTORS.with(C::UI)),
        WorldEvent::ItemActor(ItemActorEvent::Action(_)) => {
            Footprint::session(C::ACTORS.with(C::PARTICLES))
        }
        WorldEvent::ItemActor(ItemActorEvent::Registry(_)) => {
            Footprint::session(C::ACTORS.with(C::INVENTORY))
        }
        WorldEvent::Ui(UiEvent::PlayerGameMode { .. } | UiEvent::DefaultGameMode(_)) => {
            Footprint::session(C::UI.with(C::ACTORS))
        }
        WorldEvent::Ui(UiEvent::Boss(_)) => Footprint::session(C::UI.with(C::ACTORS)),
        WorldEvent::Ui(_) | WorldEvent::Experience(_) | WorldEvent::MapData(_) => {
            Footprint::session(C::UI)
        }
        // An actor-bound effect finds its actor when consumed, so it follows that spawn.
        WorldEvent::Particle(ParticleEvent::Spawn(spawn)) if spawn.actor_unique_id.is_some() => {
            Footprint::session(C::PARTICLES.with(C::ACTORS))
        }
        WorldEvent::Particle(_) => Footprint::session(C::PARTICLES),
        WorldEvent::Inventory(_) => Footprint::inventory(),
    }
}

fn actor_consumers(event: &ActorEvent, context: LaneContext) -> Consumers {
    use Consumers as C;
    let local_runtime = |runtime_id: u64| runtime_id == context.local_runtime_id;
    match event {
        ActorEvent::Move(_) | ActorEvent::Identifiers(_) => C::ACTORS,
        ActorEvent::Metadata(update) if local_runtime(update.runtime_id) => {
            C::ACTORS.with(C::CONTROLS).with(C::UI)
        }
        ActorEvent::Attributes(update) if local_runtime(update.runtime_id) => {
            C::ACTORS.with(C::CONTROLS).with(C::UI)
        }
        ActorEvent::Status(status) if local_runtime(status.runtime_id) => {
            C::ACTORS.with(C::CONTROLS).with(C::UI)
        }
        ActorEvent::PlayerList(_) => C::ACTORS.with(C::CONTROLS).with(C::UI),
        // A pickup voices its own sound, which the audio consumer orders by sequence.
        ActorEvent::TakeItem(_) => C::ACTORS.with(C::UI).with(C::AUDIO),
        // Spawns and removals can change the local mount, which publishes a UI delta.
        _ => C::ACTORS.with(C::UI),
    }
}

/// Earlier unfinished events folded into one conflict summary, in wire order.
#[derive(Debug, Default)]
pub(crate) struct EarlierEvents {
    any: bool,
    barrier: bool,
    retention: bool,
    terrain: bool,
    mutation: bool,
    consumers: Consumers,
    columns: Vec<ChunkKey>,
}

impl EarlierEvents {
    pub(crate) fn clear(&mut self) {
        let mut columns = std::mem::take(&mut self.columns);
        columns.clear();
        *self = Self {
            columns,
            ..Self::default()
        };
    }

    /// An unadmitted sequence has an unknown footprint, so nothing later may pass it.
    pub(crate) fn add_unknown(&mut self) {
        self.any = true;
        self.barrier = true;
    }

    pub(crate) fn add(&mut self, earlier: &Footprint, couple_position: bool) {
        self.any = true;
        self.barrier |= earlier.barrier;
        self.retention |= earlier.retention || (couple_position && earlier.positional);
        self.terrain |= !earlier.columns.is_empty();
        self.mutation |= earlier.mutation;
        self.consumers = self.consumers.with(earlier.consumers);
        self.columns.extend_from_slice(&earlier.columns);
    }

    /// Whether `later` must wait for some earlier event folded into this summary.
    pub(crate) fn blocks(&self, later: &Footprint, couple_position: bool) -> bool {
        if !self.any {
            return false;
        }
        let later_retention = later.retention || (couple_position && later.positional);
        self.barrier
            || later.barrier
            || (later_retention && (self.terrain || self.retention))
            || (self.retention && !later.columns.is_empty())
            || (later.local_authority && self.mutation)
            || self.consumers.intersects(later.consumers)
            || later
                .columns
                .iter()
                .any(|column| self.columns.contains(column))
    }

    /// True once nothing later can commit past these events.
    pub(crate) fn is_barrier(&self) -> bool {
        self.barrier
    }
}
