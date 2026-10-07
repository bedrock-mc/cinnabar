use super::*;

/// Raw block-space witness for a server publisher view.
///
/// The containing chunk and ceiling chunk radius remain on [`ViewCohort`] as a
/// bounded publisher/control envelope. This identity preserves values that
/// would otherwise be lost when an unaligned block centre or radius is
/// converted to chunks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublisherViewGeometry {
    pub center_blocks: [i32; 2],
    pub radius_blocks: u32,
}

/// One horizontal publisher view with independently tracked required columns.
///
/// `center` and `radius` define the enclosing Chebyshev publisher/control
/// envelope in chunk columns. Publisher-created cohorts additionally retain
/// the raw wire witness, but the protocol does not define an enumerable
/// universal column set from that witness. Required membership is recorded
/// separately from unique `LevelChunk` announcements — request-mode and inline
/// alike — in the publisher epoch, each only after its own decode and data
/// admission gates. The player grid's independent retention geometry is not
/// stored in this publisher identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewCohort {
    pub dimension: i32,
    pub center: [i32; 2],
    pub radius: i32,
    pub publisher_geometry: Option<PublisherViewGeometry>,
}

impl ViewCohort {
    #[must_use]
    pub fn from_publisher(dimension: i32, center: [i32; 3], radius_blocks: u32) -> Self {
        let chunks = radius_blocks.saturating_add(15) / 16;
        Self {
            dimension,
            center: [center[0].div_euclid(16), center[2].div_euclid(16)],
            radius: i32::try_from(chunks).unwrap_or(i32::MAX),
            publisher_geometry: Some(PublisherViewGeometry {
                center_blocks: [center[0], center[2]],
                radius_blocks,
            }),
        }
    }

    #[must_use]
    pub fn contains_column(self, dimension: i32, column: [i32; 2]) -> bool {
        dimension == self.dimension
            && i64::from(column[0]).abs_diff(i64::from(self.center[0])) <= self.radius.max(0) as u64
            && i64::from(column[1]).abs_diff(i64::from(self.center[1])) <= self.radius.max(0) as u64
    }

    /// Returns a static diagnostic classifier, never publisher readiness.
    ///
    /// Manually constructed cohorts use the legacy Euclidean chunk disk.
    /// Publisher witnesses use Dragonfly's attributable `distance < r - 0.5`
    /// policy; other servers may announce a different set.
    #[must_use]
    pub fn classifier_columns(self) -> BTreeSet<ChunkKey> {
        let radius = self.radius.max(0);
        let doubled_limit = self.publisher_geometry.map_or_else(
            || i64::from(radius).saturating_mul(2),
            |_| i64::from(radius).saturating_mul(2).saturating_sub(1),
        );
        (-radius..=radius)
            .flat_map(|x_offset| {
                (-radius..=radius)
                    .filter(move |z_offset| {
                        let x = i64::from(x_offset).unsigned_abs().saturating_mul(2);
                        let z = i64::from(*z_offset).unsigned_abs().saturating_mul(2);
                        x.saturating_mul(x).saturating_add(z.saturating_mul(z))
                            <= doubled_limit
                                .unsigned_abs()
                                .saturating_mul(doubled_limit.unsigned_abs())
                    })
                    .map(move |z_offset| {
                        ChunkKey::new(
                            self.dimension,
                            self.center[0].saturating_add(x_offset),
                            self.center[1].saturating_add(z_offset),
                        )
                    })
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CommittedControlEvent {
    /// An ordered server probe ready to echo after earlier controls are applied.
    NetworkStackLatency {
        sequence: u64,
        creation_time: u64,
    },
    /// A MobEffect change for the local movement simulator. Its packet tick is
    /// retained as correlation metadata, not used as a local expiry clock.
    /// The same event is also retained in [`CommittedUiEvent::LocalEffect`].
    LocalMovementEffect {
        sequence: u64,
        event: protocol::ActorEffectEvent,
    },
    /// Movement flags from one local-player SetActorData.
    LocalMovementFlags {
        sequence: u64,
        /// Local input tick the server stamped; zero when unstamped.
        tick: u64,
        flags: crate::MovementFlagUpdate,
    },
    /// Movement, underwater and lava speed attribute currents from one local
    /// attribute update, as one control; an absent attribute is `None`.
    LocalMovementSpeed {
        sequence: u64,
        dimension: i32,
        /// Effective `minecraft:movement` current.
        current: Option<f64>,
        /// Total/current factor for the vanilla sprint modifier.
        sprint_modifier: Option<f32>,
        underwater: Option<f64>,
        lava: Option<f64>,
        /// Local input tick the server stamped; zero when unstamped.
        tick: u64,
    },
    /// Finite `minecraft:air_drag_modifier` current for the local player.
    LocalAirDragModifier {
        sequence: u64,
        current: f32,
        /// Local input tick the server stamped; zero when unstamped.
        tick: u64,
    },
    MovePlayer {
        sequence: u64,
        movement: MovePlayerEvent,
        resolved: ResolvedServerPosition,
        source_cohort: Option<ViewCohort>,
    },
    PlayerMovementCorrection {
        sequence: u64,
        correction: PlayerMovementCorrectionEvent,
        resolved: ResolvedServerPosition,
    },
    /// A server-authoritative velocity impulse for the local player
    /// (knockback, explosion, launch). Other actors have no velocity consumer.
    LocalActorMotion {
        sequence: u64,
        event: protocol::ActorMotionEvent,
    },
    /// A server-predicted movement effect (firework glide boost and similar)
    /// for the local player, stamped with the input tick it starts after.
    LocalMovementBoost {
        sequence: u64,
        event: protocol::MovementEffectEvent,
    },
    /// The local player took damage; `source_direction` is the world-space horizontal `(x, z)`
    /// vector toward the damage source when a recent knockback impulse implies one.
    LocalHurt {
        sequence: u64,
        source_direction: Option<[f32; 2]>,
    },
    /// The retained authoritative player list changed and Tab/rawtext identity
    /// consumers must refresh even when no ordinary UI packet committed.
    PlayerListChanged {
        sequence: u64,
    },
    ChangeDimension {
        sequence: u64,
        change: ChangeDimensionEvent,
        resolved: ResolvedServerPosition,
    },
    DimensionChangeAck {
        sequence: u64,
        dimension_epoch: u64,
    },
    Respawn {
        sequence: u64,
        respawn: RespawnEvent,
        resolved: ResolvedServerPosition,
    },
    SetTime {
        sequence: u64,
        update: SetTimeEvent,
    },
    WorldClocks {
        sequence: u64,
        update: protocol::WorldClockUpdateEvent,
    },
    DaylightCycle {
        sequence: u64,
        update: DaylightCycleUpdateEvent,
    },
    WeatherCycle {
        sequence: u64,
        enabled: bool,
    },
    Weather {
        sequence: u64,
        update: WeatherUpdateEvent,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum CommittedUiEvent {
    Experience {
        sequence: u64,
        dimension_epoch: u64,
        event: protocol::ExperienceMessage,
    },
    /// Evidence addressed to the bootstrap's persistent local actor identity.
    LocalAbilities {
        sequence: u64,
        stream_identity: u64,
        event: protocol::AbilitiesUpdate,
    },
    /// Forms carry their committed dimension lifetime, even across a return
    /// to the same numeric dimension before the UI FIFO is drained.
    Form {
        sequence: u64,
        dimension_epoch: u64,
        event: protocol::FormRequestEvent,
    },
    Ui {
        sequence: u64,
        event: UiEvent,
    },
    BlockCrack {
        sequence: u64,
        dimension: i32,
        event: BlockCrackEvent,
    },
    LocalAttributes {
        sequence: u64,
        server_tick: u64,
        attributes: Arc<[ActorAttribute]>,
    },
    /// Local-player SetEntityData entries (air supply, freezing strength, ...).
    LocalMetadata {
        sequence: u64,
        server_tick: u64,
        metadata: Arc<[protocol::ActorMetadata]>,
    },
    /// A MobEffect change addressed to the local player.
    LocalEffect {
        sequence: u64,
        event: protocol::ActorEffectEvent,
    },
    /// The local player's authoritative mount after a link or actor-lifetime change.
    /// `None` means the player is no longer riding anything.
    LocalMount {
        sequence: u64,
        ridden_unique_id: Option<i64>,
    },
}

/// One audio command retaining its world commit and transport identity.
#[derive(Debug, Clone, PartialEq)]
pub struct CommittedAudioEvent {
    pub sequence: u64,
    pub dimension: i32,
    pub dimension_epoch: u64,
    /// Already-committed sound released by this actor's fixed-tick synchronization.
    pub actor_synchronization: Option<crate::ActorLifetimeId>,
    pub event: AudioEvent,
}

/// One packet-order-preserving particle trigger committed by the world stream.
#[derive(Debug, Clone, PartialEq)]
pub struct CommittedParticleEvent {
    pub sequence: u64,
    pub dimension: i32,
    pub event: protocol::ParticleEvent,
}

/// One packet-order-preserving server camera command committed by the world stream.
#[derive(Debug, Clone, PartialEq)]
pub struct CommittedCameraEvent {
    pub sequence: u64,
    pub event: protocol::CameraEvent,
}
