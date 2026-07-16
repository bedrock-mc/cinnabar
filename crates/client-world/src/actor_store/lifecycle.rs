use super::*;

impl ActorStore {
    pub(crate) fn new(session_id: u64, dimension: i32) -> Self {
        Self::with_capacity(
            session_id,
            dimension,
            MAX_TRACKED_ACTORS,
            MAX_TRACKED_PLAYERS,
        )
    }
    pub(crate) fn with_capacity(
        session_id: u64,
        dimension: i32,
        max_actors: usize,
        max_players: usize,
    ) -> Self {
        Self::with_limits(
            session_id,
            dimension,
            max_actors,
            max_players,
            MAX_TRACKED_PLAYER_SKIN_BYTES,
        )
    }
    pub(super) fn with_limits(
        session_id: u64,
        dimension: i32,
        max_actors: usize,
        max_players: usize,
        max_player_skin_bytes: usize,
    ) -> Self {
        Self {
            session_id,
            dimension,
            latest_sequence: 0,
            max_actors,
            max_players,
            max_player_skin_bytes,
            retained_player_skin_bytes: 0,
            actors: HashMap::new(),
            unique_to_runtime: HashMap::new(),
            players: HashMap::new(),
        }
    }
    #[cfg(test)]
    pub(crate) fn begin_session(&mut self, session_id: u64, dimension: i32) {
        self.session_id = session_id;
        self.dimension = dimension;
        self.latest_sequence = 0;
        self.actors.clear();
        self.unique_to_runtime.clear();
        self.players.clear();
        self.retained_player_skin_bytes = 0;
    }
    pub(crate) fn reset_dimension(
        &mut self,
        session_id: u64,
        sequence: u64,
        dimension: i32,
    ) -> ActorApplyResult {
        let guard = self.guard(session_id, sequence);
        if guard != ActorApplyResult::Updated {
            return guard;
        }
        self.dimension = dimension;
        self.actors.clear();
        self.unique_to_runtime.clear();
        ActorApplyResult::Reset
    }
    pub(crate) fn apply(
        &mut self,
        session_id: u64,
        sequence: u64,
        event: ActorEvent,
    ) -> ActorApplyResult {
        let guard = self.guard(session_id, sequence);
        if guard != ActorApplyResult::Updated {
            return guard;
        }
        if event_dimension(&event).is_some_and(|dimension| dimension != self.dimension) {
            return ActorApplyResult::StaleDimension;
        }
        match event {
            ActorEvent::Spawn(spawn) => self.apply_spawn(sequence, spawn),
            ActorEvent::Remove(remove) => self.remove_unique(remove.unique_id),
            ActorEvent::Move(movement) => {
                let Some(actor) = self.actors.get_mut(&movement.runtime_id) else {
                    return ActorApplyResult::MissingActor;
                };
                let mut received = actor.received_pose;
                let player_network_position = movement.position_origin
                    == ActorPositionOrigin::NetworkOffset
                    && matches!(actor.kind, ActorKind::Player { .. });
                for (axis, (target, source)) in received
                    .position
                    .iter_mut()
                    .zip(movement.position)
                    .enumerate()
                {
                    if let Some(source) = source {
                        *target = if player_network_position && axis == 1 {
                            source - PLAYER_NETWORK_OFFSET
                        } else {
                            source
                        };
                    }
                }
                if let Some(value) = movement.pitch {
                    received.pitch = value;
                }
                if let Some(value) = movement.yaw {
                    received.yaw = value;
                }
                if let Some(value) = movement.head_yaw {
                    received.head_yaw = value;
                }
                if let Some(value) = movement.on_ground {
                    actor.on_ground = Some(value);
                }
                actor.received_pose = received;
                if movement.teleported {
                    actor.previous_pose = received;
                    actor.set_current_pose(received);
                    actor.interpolation_ticks_remaining = 0;
                } else if matches!(actor.kind, ActorKind::Player { .. }) {
                    actor.interpolation_ticks_remaining = PLAYER_POSITION_INTERPOLATION_TICKS;
                } else {
                    actor.previous_pose = received;
                    actor.set_current_pose(received);
                    actor.interpolation_ticks_remaining = 0;
                }
                actor.movement_revision = sequence;
                actor.teleported = movement.teleported;
                actor.player_mode = movement.player_mode;
                actor.source_tick = movement.source_tick;
                ActorApplyResult::Updated
            }
            ActorEvent::Metadata(update) => {
                let Some(actor) = self.actors.get_mut(&update.runtime_id) else {
                    return ActorApplyResult::MissingActor;
                };
                let rejected = actor.apply_metadata(&update.metadata)
                    | actor.apply_properties(&update.properties);
                if rejected {
                    ActorApplyResult::CapacityRejected
                } else {
                    ActorApplyResult::Updated
                }
            }
            ActorEvent::Attributes(update) => {
                let Some(actor) = self.actors.get_mut(&update.runtime_id) else {
                    return ActorApplyResult::MissingActor;
                };
                if actor.apply_attributes(&update.attributes) {
                    ActorApplyResult::CapacityRejected
                } else {
                    ActorApplyResult::Updated
                }
            }
            ActorEvent::PlayerList(update) => {
                let mut capacity_rejected = false;
                for entry in update.entries.iter() {
                    match entry {
                        PlayerListEntry::Add {
                            uuid,
                            unique_id,
                            username,
                            verified,
                            skin,
                        } => {
                            if self.players.len() >= self.max_players
                                && !self.players.contains_key(uuid)
                            {
                                capacity_rejected = true;
                                continue;
                            }
                            let previous = self.players.get(uuid);
                            let previous_skin_bytes =
                                previous.map_or(0, |profile| retained_skin_bytes(&profile.skin));
                            let retained_without_previous = self
                                .retained_player_skin_bytes
                                .saturating_sub(previous_skin_bytes);
                            let requested_skin_bytes = retained_skin_bytes(skin);
                            let (skin, retained_player_skin_bytes) = retained_without_previous
                                .checked_add(requested_skin_bytes)
                                .filter(|total| *total <= self.max_player_skin_bytes)
                                .map_or_else(
                                    || {
                                        previous.map_or_else(
                                            || {
                                                (
                                                    PlayerSkin::Unavailable(
                                                        PlayerSkinUnavailable::RetainedBudgetExceeded,
                                                    ),
                                                    retained_without_previous,
                                                )
                                            },
                                            |profile| {
                                                (
                                                    profile.skin.clone(),
                                                    retained_without_previous
                                                        .saturating_add(previous_skin_bytes),
                                                )
                                            },
                                        )
                                    },
                                    |total| (skin.clone(), total),
                                );
                            self.retained_player_skin_bytes = retained_player_skin_bytes;
                            self.players.insert(
                                *uuid,
                                PlayerProfile {
                                    unique_id: *unique_id,
                                    username: username.clone(),
                                    verified: *verified,
                                    skin,
                                },
                            );
                        }
                        PlayerListEntry::Remove { uuid } => {
                            if let Some(profile) = self.players.remove(uuid) {
                                self.retained_player_skin_bytes = self
                                    .retained_player_skin_bytes
                                    .saturating_sub(retained_skin_bytes(&profile.skin));
                            }
                        }
                    }
                }
                if capacity_rejected {
                    ActorApplyResult::CapacityRejected
                } else {
                    ActorApplyResult::Updated
                }
            }
        }
    }
    pub(crate) fn advance_interpolation_ticks(&mut self, ticks: u32) {
        for actor in self.actors.values_mut() {
            let meaningful_ticks = if matches!(actor.kind, ActorKind::Player { .. }) {
                u32::from(actor.interpolation_ticks_remaining).saturating_add(1)
            } else {
                1
            };
            for _ in 0..ticks.min(meaningful_ticks) {
                let current = actor.current_pose();
                actor.previous_pose = current;
                let mut next = actor.received_pose;
                if matches!(actor.kind, ActorKind::Player { .. })
                    && actor.interpolation_ticks_remaining > 0
                {
                    let divisor = f32::from(actor.interpolation_ticks_remaining);
                    next.position = std::array::from_fn(|axis| {
                        current.position[axis]
                            + (actor.received_pose.position[axis] - current.position[axis])
                                / divisor
                    });
                    actor.interpolation_ticks_remaining -= 1;
                }
                actor.set_current_pose(next);
            }
        }
    }
    pub(crate) fn apply_player_move(
        &mut self,
        session_id: u64,
        sequence: u64,
        dimension: i32,
        movement: MovePlayerEvent,
    ) -> ActorApplyResult {
        self.apply(
            session_id,
            sequence,
            ActorEvent::Move(ActorMoveEvent {
                dimension,
                runtime_id: movement.runtime_id,
                position: movement.position.map(Some),
                position_origin: ActorPositionOrigin::NetworkOffset,
                pitch: Some(movement.pitch),
                yaw: Some(movement.yaw),
                head_yaw: Some(movement.head_yaw),
                on_ground: Some(movement.on_ground),
                teleported: movement.teleported,
                player_mode: Some(movement.mode),
                source_tick: Some(movement.source_tick),
            }),
        )
    }
    fn guard(&mut self, session_id: u64, sequence: u64) -> ActorApplyResult {
        if session_id != self.session_id {
            return ActorApplyResult::StaleSession;
        }
        if sequence <= self.latest_sequence {
            return ActorApplyResult::StaleSequence;
        }
        self.latest_sequence = sequence;
        ActorApplyResult::Updated
    }
    fn apply_spawn(&mut self, sequence: u64, spawn: ActorSpawnEvent) -> ActorApplyResult {
        let replaces_runtime = self.actors.contains_key(&spawn.runtime_id);
        let replaces_unique = self.unique_to_runtime.contains_key(&spawn.unique_id);
        if self.actors.len() >= self.max_actors && !replaces_runtime && !replaces_unique {
            return ActorApplyResult::CapacityRejected;
        }

        let mut replaced = false;
        if let Some(previous) = self.actors.remove(&spawn.runtime_id) {
            self.unique_to_runtime.remove(&previous.unique_id);
            replaced = true;
        }
        if let Some(previous_runtime) = self.unique_to_runtime.remove(&spawn.unique_id) {
            self.actors.remove(&previous_runtime);
            replaced = true;
        }
        let runtime_id = spawn.runtime_id;
        let unique_id = spawn.unique_id;
        self.actors
            .insert(runtime_id, ActorSnapshot::from_spawn(spawn, sequence));
        self.unique_to_runtime.insert(unique_id, runtime_id);
        if replaced {
            ActorApplyResult::Replaced
        } else {
            ActorApplyResult::Inserted
        }
    }
    fn remove_unique(&mut self, unique_id: i64) -> ActorApplyResult {
        let Some(runtime_id) = self.unique_to_runtime.remove(&unique_id) else {
            return ActorApplyResult::MissingActor;
        };
        self.actors.remove(&runtime_id);
        ActorApplyResult::Removed
    }
}
