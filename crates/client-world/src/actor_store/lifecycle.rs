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
        Self::with_limits_and_animation(
            session_id,
            dimension,
            max_actors,
            max_players,
            max_player_skin_bytes,
            crate::actor_animation::ActorAnimationStore::diagnostic(),
        )
    }
    pub(crate) fn new_with_entity_assets(
        session_id: u64,
        dimension: i32,
        entity_assets: std::sync::Arc<assets::RuntimeEntityAssets>,
    ) -> Self {
        let mut store = Self::with_limits_and_animation(
            session_id,
            dimension,
            MAX_TRACKED_ACTORS,
            MAX_TRACKED_PLAYERS,
            MAX_TRACKED_PLAYER_SKIN_BYTES,
            crate::actor_animation::ActorAnimationStore::with_assets(std::sync::Arc::clone(
                &entity_assets,
            )),
        );
        store.items =
            crate::item::ItemStateStore::with_assets(std::sync::Arc::clone(&entity_assets));
        store.actions = crate::action::RemoteActionStore::with_assets(entity_assets);
        store
    }
    fn with_limits_and_animation(
        session_id: u64,
        dimension: i32,
        max_actors: usize,
        max_players: usize,
        max_player_skin_bytes: usize,
        animation: crate::actor_animation::ActorAnimationStore,
    ) -> Self {
        Self {
            session_id,
            dimension,
            latest_sequence: 0,
            max_actors,
            max_players,
            max_player_skin_bytes,
            retained_player_skin_bytes: 0,
            retained_player_skin_geometry_bytes: 0,
            default_game_mode: ActorGameMode::Survival,
            actors: HashMap::new(),
            unique_to_runtime: HashMap::new(),
            pending_game_modes: HashMap::new(),
            pending_actor_links: HashMap::new(),
            position_revisions: HashMap::new(),
            velocity_revisions: HashMap::new(),
            players: HashMap::new(),
            animation,
            items: crate::item::ItemStateStore::diagnostic(),
            actions: crate::action::RemoteActionStore::diagnostic(),
            remote_state_excluded_runtime_id: None,
        }
    }

    pub(crate) fn exclude_remote_state_for(&mut self, runtime_id: u64) {
        self.remote_state_excluded_runtime_id = Some(runtime_id);
        if let Some(lifetime) = self.lifetime(runtime_id) {
            self.items.remove(lifetime);
            self.actions.remove(lifetime);
        }
    }

    pub(crate) fn set_default_game_mode(&mut self, game_mode: ActorGameMode) {
        self.default_game_mode = game_mode;
        for actor in self.actors.values_mut() {
            actor.resolved_game_mode = actor
                .game_mode
                .map(|raw| raw.resolve_fallback(self.default_game_mode));
        }
    }

    pub(crate) fn apply_player_game_mode_update(
        &mut self,
        unique_id: i64,
        update: protocol::GameModeUpdate,
    ) {
        self.set_actor_game_mode(unique_id, actor_game_mode_from_update(update), None);
    }

    pub(crate) fn apply_default_game_mode_update(&mut self, update: protocol::GameModeUpdate) {
        self.set_default_game_mode(actor_game_mode_from_update(update));
    }

    fn set_actor_game_mode(&mut self, unique_id: i64, game_mode: ActorGameMode, tick: Option<u64>) {
        let Some(runtime_id) = self.unique_to_runtime.get(&unique_id).copied() else {
            return;
        };
        let Some(actor) = self.actors.get_mut(&runtime_id) else {
            return;
        };
        if !matches!(actor.kind, ActorKind::Player { .. }) {
            return;
        }
        actor.game_mode = Some(game_mode);
        actor.resolved_game_mode = Some(game_mode.resolve_fallback(self.default_game_mode));
        if let Some(tick) = tick {
            actor.game_mode_tick = Some(tick);
        }
    }

    /// StartGame identifies the local player but Bedrock does not send an
    /// AddPlayer event for that same actor. Keep a local actor snapshot in the
    /// canonical actor/rig store when entity assets are available, while the
    /// remote-state exclusion continues to keep local inventory/equipment
    /// authority in the app-owned local-player path.
    pub(crate) fn install_local_player(
        &mut self,
        runtime_id: u64,
        unique_id: i64,
        position: [f32; 3],
    ) {
        if !self.animation.has_assets() || runtime_id == 0 {
            return;
        }
        let spawn = ActorSpawnEvent {
            dimension: self.dimension,
            unique_id,
            runtime_id,
            kind: ActorKind::Player {
                uuid: [0; 16],
                username: std::sync::Arc::from(""),
            },
            game_mode: None,
            position,
            velocity: [0.0; 3],
            pitch: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
            body_yaw: 0.0,
            held_item: NetworkItemStack::empty(),
            metadata: std::sync::Arc::from([]),
            attributes: std::sync::Arc::from([]),
            properties: std::sync::Arc::from([]),
        };
        self.velocity_revisions.remove(&runtime_id);
        let mut snapshot =
            ActorSnapshot::from_spawn(spawn, self.session_id.max(1), self.default_game_mode);
        self.apply_pending_game_mode(&mut snapshot);
        self.actors.insert(runtime_id, snapshot);
        self.unique_to_runtime.insert(unique_id, runtime_id);
        if let Some(link) = self.pending_actor_links.remove(&unique_id)
            && matches!(
                link.link_type,
                protocol::ActorLinkType::Rider | protocol::ActorLinkType::Passenger
            )
            && let Some(actor) = self.actors.get_mut(&runtime_id)
        {
            actor.mount_unique_id = Some(link.ridden_unique_id);
        }
        if let Some(actor) = self.actors.get(&runtime_id) {
            self.animation
                .insert(self.session_id, self.dimension, actor);
        }
    }
    #[cfg(test)]
    pub(crate) fn begin_session(&mut self, session_id: u64, dimension: i32) {
        self.session_id = session_id;
        self.dimension = dimension;
        self.latest_sequence = 0;
        self.default_game_mode = ActorGameMode::Survival;
        self.actors.clear();
        self.unique_to_runtime.clear();
        self.pending_game_modes.clear();
        self.pending_actor_links.clear();
        self.position_revisions.clear();
        self.velocity_revisions.clear();
        self.players.clear();
        self.retained_player_skin_bytes = 0;
        self.retained_player_skin_geometry_bytes = 0;
        self.animation.clear();
        self.items.clear();
        self.actions.clear();
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
        self.pending_game_modes.clear();
        self.pending_actor_links.clear();
        self.position_revisions.clear();
        self.velocity_revisions.clear();
        self.animation.clear();
        self.items.clear_actor_state();
        self.actions.clear();
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
                // MovePlayer's rotation mode carries a position-shaped field
                // on the wire, but its authority is orientation-only. Treat
                // it like a delta rotation so it cannot reset a moving
                // actor's positional velocity or gait phase.
                let has_position_update = movement.position.iter().any(Option::is_some)
                    && movement.player_mode != Some(MovePlayerMode::Rotation);
                let network_position_offset =
                    if movement.position_origin == ActorPositionOrigin::NetworkOffset {
                        actor.network_position_offset()
                    } else {
                        0.0
                    };
                let immediate = movement.teleported
                    || movement.position_origin == ActorPositionOrigin::FeetImmediate;
                if has_position_update {
                    for (axis, (target, source)) in received
                        .position
                        .iter_mut()
                        .zip(movement.position)
                        .enumerate()
                    {
                        if let Some(source) = source {
                            *target = if axis == 1 {
                                source - network_position_offset
                            } else {
                                source
                            };
                        }
                    }
                }
                if let Some(value) = movement.pitch {
                    received.pitch = value;
                }
                if let Some(value) = movement.yaw {
                    received.yaw = value;
                    actor.body_yaw = value;
                }
                if let Some(value) = movement.head_yaw {
                    received.head_yaw = value;
                }
                if let Some(value) = movement.on_ground {
                    actor.on_ground = Some(value);
                }
                let elapsed_seconds = movement
                    .source_tick
                    .zip(actor.source_tick)
                    .and_then(|(current, previous)| current.checked_sub(previous))
                    .filter(|ticks| *ticks > 0)
                    .map_or(0.05, |ticks| ticks as f32 * 0.05);
                let derived_velocity = if movement.teleported {
                    [0.0; 3]
                } else if !has_position_update {
                    actor.velocity
                } else {
                    std::array::from_fn(|axis| {
                        (received.position[axis] - actor.received_pose.position[axis])
                            / elapsed_seconds
                    })
                };
                actor.velocity = if derived_velocity.iter().all(|value| value.is_finite()) {
                    derived_velocity
                } else {
                    [0.0; 3]
                };
                if has_position_update {
                    self.position_revisions
                        .insert(movement.runtime_id, sequence);
                }
                actor.received_pose = received;
                if immediate {
                    actor.previous_pose = received;
                    actor.set_current_pose(received);
                    actor.interpolation_ticks_remaining = 0;
                } else {
                    actor.interpolation_ticks_remaining =
                        if matches!(actor.kind, ActorKind::Player { .. }) {
                            PLAYER_POSITION_INTERPOLATION_TICKS
                        } else {
                            ACTOR_POSITION_INTERPOLATION_TICKS
                        };
                }
                actor.movement_revision = sequence;
                actor.teleported = movement.teleported;
                actor.player_mode = movement.player_mode;
                actor.source_tick = movement.source_tick;
                if movement.teleported {
                    self.animation.mark_reset(movement.runtime_id);
                    if let Some(lifetime) = self.lifetime(movement.runtime_id) {
                        self.actions.reset_on_teleport(lifetime);
                    }
                }
                ActorApplyResult::Updated
            }
            ActorEvent::Metadata(update) => {
                let Some(actor) = self.actors.get_mut(&update.runtime_id) else {
                    return ActorApplyResult::MissingActor;
                };
                let incompatible = update.metadata.iter().any(|metadata| {
                    actor.metadata.get(&metadata.key).is_some_and(|previous| {
                        std::mem::discriminant(previous) != std::mem::discriminant(&metadata.value)
                    })
                });
                let rejected = actor.apply_metadata(&update.metadata)
                    | actor.apply_properties(&update.properties);
                if incompatible {
                    self.animation.mark_reset(update.runtime_id);
                }
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
            ActorEvent::GameMode(update) => {
                let Some(runtime_id) = self.unique_to_runtime.get(&update.unique_id).copied()
                else {
                    let retain = self.pending_game_modes.get(&update.unique_id).map_or(
                        self.pending_game_modes.len() < self.max_actors,
                        |previous| update.tick >= previous.tick,
                    );
                    if retain {
                        self.pending_game_modes.insert(update.unique_id, update);
                        return ActorApplyResult::Updated;
                    }
                    return ActorApplyResult::CapacityRejected;
                };
                let Some(actor) = self.actors.get_mut(&runtime_id) else {
                    return ActorApplyResult::MissingActor;
                };
                if !matches!(actor.kind, ActorKind::Player { .. }) {
                    return ActorApplyResult::MissingActor;
                }
                if actor
                    .game_mode_tick
                    .is_some_and(|previous| update.tick < previous)
                {
                    return ActorApplyResult::StaleSequence;
                }
                actor.game_mode = Some(update.game_mode);
                actor.resolved_game_mode =
                    Some(update.game_mode.resolve_fallback(self.default_game_mode));
                actor.game_mode_tick = Some(update.tick);
                ActorApplyResult::Updated
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
                            let previous_skin_geometry_bytes = previous
                                .map_or(0, |profile| retained_skin_geometry_bytes(&profile.skin));
                            let retained_without_previous = self
                                .retained_player_skin_bytes
                                .saturating_sub(previous_skin_bytes);
                            let retained_without_previous_geometry = self
                                .retained_player_skin_geometry_bytes
                                .saturating_sub(previous_skin_geometry_bytes);
                            let requested_skin_bytes = retained_skin_bytes(skin);
                            let requested_skin_geometry_bytes = retained_skin_geometry_bytes(skin);
                            let accepted = retained_without_previous
                                .checked_add(requested_skin_bytes)
                                .filter(|total| *total <= self.max_player_skin_bytes)
                                .and_then(|skin_bytes| {
                                    retained_without_previous_geometry
                                        .checked_add(requested_skin_geometry_bytes)
                                        .filter(|total| {
                                            *total <= MAX_TRACKED_PLAYER_SKIN_GEOMETRY_BYTES
                                        })
                                        .map(|geometry_bytes| (skin_bytes, geometry_bytes))
                                });
                            let (skin, retained_player_skin_bytes, retained_geometry_bytes) =
                                accepted.map_or_else(
                                    || {
                                        previous.map_or_else(
                                            || {
                                                (
                                                    PlayerSkin::Unavailable(
                                                        PlayerSkinUnavailable::RetainedBudgetExceeded,
                                                    ),
                                                    retained_without_previous,
                                                    retained_without_previous_geometry,
                                                )
                                            },
                                            |profile| {
                                                (
                                                    profile.skin.clone(),
                                                    retained_without_previous
                                                        .saturating_add(previous_skin_bytes),
                                                    retained_without_previous_geometry
                                                        .saturating_add(previous_skin_geometry_bytes),
                                                )
                                            },
                                        )
                                    },
                                    |(skin_bytes, geometry_bytes)| {
                                        (skin.clone(), skin_bytes, geometry_bytes)
                                    },
                                );
                            self.retained_player_skin_bytes = retained_player_skin_bytes;
                            self.retained_player_skin_geometry_bytes = retained_geometry_bytes;
                            self.players.insert(
                                *uuid,
                                PlayerProfile {
                                    unique_id: *unique_id,
                                    username: username.clone(),
                                    verified: *verified,
                                    skin,
                                },
                            );
                            let geometry = self.player_skin_geometry(*unique_id);
                            if let Some(runtime_id) = self.unique_to_runtime.get(unique_id).copied()
                            {
                                if let Some(actor) = self.actors.get(&runtime_id).cloned() {
                                    self.animation.insert_with_skin(
                                        self.session_id,
                                        self.dimension,
                                        &actor,
                                        geometry.as_ref(),
                                    );
                                }
                            }
                        }
                        PlayerListEntry::Remove { uuid } => {
                            if let Some(profile) = self.players.remove(uuid) {
                                self.retained_player_skin_bytes = self
                                    .retained_player_skin_bytes
                                    .saturating_sub(retained_skin_bytes(&profile.skin));
                                self.retained_player_skin_geometry_bytes = self
                                    .retained_player_skin_geometry_bytes
                                    .saturating_sub(retained_skin_geometry_bytes(&profile.skin));
                                if let Some(runtime_id) =
                                    self.unique_to_runtime.get(&profile.unique_id).copied()
                                {
                                    if let Some(actor) = self.actors.get(&runtime_id).cloned() {
                                        self.animation.insert(
                                            self.session_id,
                                            self.dimension,
                                            &actor,
                                        );
                                    }
                                }
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

    /// Applies an ordered SetActorLink update to remote and local actor
    /// snapshots. Links may arrive before the rider spawn, so a bounded
    /// rider-keyed pending table preserves the relation until that snapshot
    /// exists without inventing a render transform.
    pub(crate) fn apply_actor_link(&mut self, event: protocol::ActorLinkEvent) {
        if event.dimension != self.dimension {
            return;
        }
        match event.link_type {
            protocol::ActorLinkType::Rider | protocol::ActorLinkType::Passenger => {
                if let Some(runtime_id) =
                    self.unique_to_runtime.get(&event.rider_unique_id).copied()
                {
                    if let Some(actor) = self.actors.get_mut(&runtime_id) {
                        actor.mount_unique_id = Some(event.ridden_unique_id);
                    }
                    self.pending_actor_links.remove(&event.rider_unique_id);
                } else if self.pending_actor_links.len() < MAX_PENDING_ACTOR_LINKS
                    || self
                        .pending_actor_links
                        .contains_key(&event.rider_unique_id)
                {
                    self.pending_actor_links
                        .insert(event.rider_unique_id, event);
                }
            }
            protocol::ActorLinkType::Remove => {
                self.pending_actor_links.remove(&event.rider_unique_id);
                if let Some(runtime_id) =
                    self.unique_to_runtime.get(&event.rider_unique_id).copied()
                    && let Some(actor) = self.actors.get_mut(&runtime_id)
                    && actor.mount_unique_id == Some(event.ridden_unique_id)
                {
                    actor.mount_unique_id = None;
                }
            }
            protocol::ActorLinkType::Unknown(_) => {}
        }
    }

    pub(crate) fn advance_interpolation_ticks(&mut self, ticks: u32) {
        for _ in 0..ticks {
            for actor in self.actors.values_mut() {
                let current = actor.current_pose();
                actor.previous_pose = current;
                let mut next = actor.received_pose;
                if actor.interpolation_ticks_remaining > 0 {
                    let divisor = f32::from(actor.interpolation_ticks_remaining);
                    let amount = (1.0 / divisor).clamp(0.0, 1.0);
                    next.position = std::array::from_fn(|axis| {
                        current.position[axis]
                            + (actor.received_pose.position[axis] - current.position[axis]) * amount
                    });
                    next.pitch = lerp_angle(current.pitch, actor.received_pose.pitch, amount);
                    next.yaw = lerp_angle(current.yaw, actor.received_pose.yaw, amount);
                    next.head_yaw =
                        lerp_angle(current.head_yaw, actor.received_pose.head_yaw, amount);
                    actor.interpolation_ticks_remaining -= 1;
                }
                let position_revision = self.position_revisions.get(&actor.runtime_id).copied();
                let applied_position_revision =
                    self.velocity_revisions.get(&actor.runtime_id).copied();
                if position_revision.is_none() && applied_position_revision.is_none() {
                    // Preserve the spawn-time velocity until the first
                    // positional authority sample replaces it. This is
                    // required for actors that begin with a launch or drift
                    // velocity and have not emitted a movement update yet.
                } else if position_revision != applied_position_revision {
                    let velocity = std::array::from_fn(|axis| {
                        (next.position[axis] - current.position[axis]) / 0.05
                    });
                    actor.velocity = if velocity.iter().all(|value| value.is_finite()) {
                        velocity
                    } else {
                        [0.0; 3]
                    };
                    if actor.interpolation_ticks_remaining == 0
                        && let Some(position_revision) = position_revision
                    {
                        self.velocity_revisions
                            .insert(actor.runtime_id, position_revision);
                    }
                } else {
                    actor.velocity = [0.0; 3];
                }
                actor.body_yaw = next.yaw;
                actor.set_current_pose(next);
            }
            self.animation
                .advance_tick(&self.actors, &self.actions, &self.items);
            self.actions.advance_tick();
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
            let lifetime = self.lifetime_for(&previous);
            self.unique_to_runtime.remove(&previous.unique_id);
            self.clear_mount_references(previous.unique_id);
            self.velocity_revisions.remove(&previous.runtime_id);
            self.position_revisions.remove(&previous.runtime_id);
            self.animation.remove_runtime(previous.runtime_id);
            self.items.remove(lifetime);
            self.actions.remove(lifetime);
            replaced = true;
        }
        if let Some(previous_runtime) = self.unique_to_runtime.remove(&spawn.unique_id) {
            if let Some(previous) = self.actors.remove(&previous_runtime) {
                let lifetime = self.lifetime_for(&previous);
                self.clear_mount_references(previous.unique_id);
                self.items.remove(lifetime);
                self.actions.remove(lifetime);
            }
            self.velocity_revisions.remove(&previous_runtime);
            self.position_revisions.remove(&previous_runtime);
            self.animation.remove_runtime(previous_runtime);
            replaced = true;
        }
        let runtime_id = spawn.runtime_id;
        let unique_id = spawn.unique_id;
        let held_item = spawn.held_item.clone();
        let geometry = self.player_skin_geometry(unique_id);
        let mut snapshot = ActorSnapshot::from_spawn(spawn, sequence, self.default_game_mode);
        self.apply_pending_game_mode(&mut snapshot);
        self.actors.insert(runtime_id, snapshot);
        self.unique_to_runtime.insert(unique_id, runtime_id);
        if let Some(actor) = self.actors.get(&runtime_id) {
            self.animation.insert_with_skin(
                self.session_id,
                self.dimension,
                actor,
                geometry.as_ref(),
            );
            if self.remote_state_excluded_runtime_id != Some(runtime_id) {
                self.items
                    .insert_spawn(self.lifetime_for(actor), sequence, held_item);
            }
        }
        if replaced {
            ActorApplyResult::Replaced
        } else {
            ActorApplyResult::Inserted
        }
    }
    fn remove_unique(&mut self, unique_id: i64) -> ActorApplyResult {
        self.pending_game_modes.remove(&unique_id);
        self.pending_actor_links
            .retain(|rider, link| *rider != unique_id && link.ridden_unique_id != unique_id);
        let Some(runtime_id) = self.unique_to_runtime.remove(&unique_id) else {
            return ActorApplyResult::MissingActor;
        };
        if let Some(actor) = self.actors.remove(&runtime_id) {
            let lifetime = self.lifetime_for(&actor);
            self.items.remove(lifetime);
            self.actions.remove(lifetime);
        }
        self.velocity_revisions.remove(&runtime_id);
        self.position_revisions.remove(&runtime_id);
        self.animation.remove_runtime(runtime_id);
        self.clear_mount_references(unique_id);
        ActorApplyResult::Removed
    }

    fn clear_mount_references(&mut self, unique_id: i64) {
        self.pending_actor_links
            .retain(|rider, link| *rider != unique_id && link.ridden_unique_id != unique_id);
        for actor in self.actors.values_mut() {
            if actor.mount_unique_id == Some(unique_id) {
                actor.mount_unique_id = None;
            }
        }
    }

    fn player_skin_geometry(&self, unique_id: i64) -> Option<PlayerSkinGeometry> {
        self.players
            .values()
            .find(|profile| profile.unique_id == unique_id)
            .and_then(|profile| match &profile.skin {
                PlayerSkin::Standard(skin) => Some(skin.geometry.clone()),
                PlayerSkin::Unavailable(_) => None,
            })
    }

    fn apply_pending_game_mode(&mut self, actor: &mut ActorSnapshot) {
        let Some(update) = self.pending_game_modes.remove(&actor.unique_id) else {
            return;
        };
        if !matches!(actor.kind, ActorKind::Player { .. }) {
            return;
        }
        if actor
            .game_mode_tick
            .is_some_and(|previous| update.tick < previous)
        {
            return;
        }
        actor.game_mode = Some(update.game_mode);
        actor.resolved_game_mode = Some(update.game_mode.resolve_fallback(self.default_game_mode));
        actor.game_mode_tick = Some(update.tick);
    }

    pub(crate) fn apply_equipment(
        &mut self,
        session_id: u64,
        sequence: u64,
        event: EquipmentEvent,
    ) -> ActorApplyResult {
        let guard = self.guard(session_id, sequence);
        if guard != ActorApplyResult::Updated {
            return guard;
        }
        if self.remote_state_excluded_runtime_id == Some(event.actor_runtime_id) {
            return ActorApplyResult::MissingActor;
        }
        let Some(lifetime) = self.lifetime(event.actor_runtime_id) else {
            return ActorApplyResult::MissingActor;
        };
        if self.items.apply_equipment(lifetime, sequence, event) {
            ActorApplyResult::Updated
        } else {
            ActorApplyResult::CapacityRejected
        }
    }

    pub(crate) fn apply_armor_equipment(
        &mut self,
        session_id: u64,
        sequence: u64,
        event: protocol::ArmorEquipmentEvent,
    ) -> ActorApplyResult {
        let guard = self.guard(session_id, sequence);
        if guard != ActorApplyResult::Updated {
            return guard;
        }
        if self.remote_state_excluded_runtime_id == Some(event.actor_runtime_id) {
            return ActorApplyResult::MissingActor;
        }
        let Some(lifetime) = self.lifetime(event.actor_runtime_id) else {
            return ActorApplyResult::MissingActor;
        };
        if self.items.apply_armor_equipment(lifetime, sequence, event) {
            ActorApplyResult::Updated
        } else {
            ActorApplyResult::CapacityRejected
        }
    }

    pub(crate) fn apply_item_actor(
        &mut self,
        session_id: u64,
        sequence: u64,
        event: ItemActorEvent,
    ) -> ActorApplyResult {
        let guard = self.guard(session_id, sequence);
        if guard != ActorApplyResult::Updated {
            return guard;
        }
        match event {
            ItemActorEvent::Registry(registry) => {
                if self.items.apply_registry(registry) {
                    ActorApplyResult::Updated
                } else {
                    ActorApplyResult::CapacityRejected
                }
            }
            ItemActorEvent::Action(action) => {
                if action.actor_runtime_ids.len() > MAX_ACTION_EVENTS_PER_TICK {
                    return ActorApplyResult::CapacityRejected;
                }
                if matches!(action.kind, protocol::ActorActionKind::Ignored { .. }) {
                    return ActorApplyResult::MissingActor;
                }
                let mut seen = HashSet::with_capacity(action.actor_runtime_ids.len());
                let mut targets = Vec::with_capacity(action.actor_runtime_ids.len());
                for runtime_id in action.actor_runtime_ids.iter().copied() {
                    if self.remote_state_excluded_runtime_id == Some(runtime_id)
                        || !seen.insert(runtime_id)
                    {
                        continue;
                    }
                    let Some(actor) = self.actors.get(&runtime_id) else {
                        continue;
                    };
                    let rig = self.animation.get(runtime_id).map(|snapshot| snapshot.rig);
                    targets.push((self.lifetime_for(actor), rig));
                }
                if targets.is_empty() {
                    return ActorApplyResult::MissingActor;
                }
                if !self.actions.can_accept(targets.len()) {
                    return ActorApplyResult::CapacityRejected;
                }
                let mut accepted = false;
                for (lifetime, rig) in targets {
                    let source_tick = ActorSourceTick::IngressSequence(sequence);
                    accepted |= self
                        .actions
                        .apply(lifetime, rig, sequence, source_tick, &action);
                }
                if accepted {
                    ActorApplyResult::Updated
                } else {
                    ActorApplyResult::MissingActor
                }
            }
        }
    }

    pub(super) fn lifetime(&self, runtime_id: u64) -> Option<ActorLifetimeId> {
        self.actors
            .get(&runtime_id)
            .map(|actor| self.lifetime_for(actor))
    }

    const fn lifetime_for(&self, actor: &ActorSnapshot) -> ActorLifetimeId {
        ActorLifetimeId {
            session_id: self.session_id,
            dimension: self.dimension,
            runtime_id: actor.runtime_id,
            spawn_revision: actor.spawn_revision,
        }
    }
}

fn lerp_angle(from: f32, to: f32, amount: f32) -> f32 {
    if !from.is_finite() || !to.is_finite() || !amount.is_finite() {
        return to;
    }
    let delta = (to - from + 180.0).rem_euclid(360.0) - 180.0;
    from + delta * amount.clamp(0.0, 1.0)
}

fn actor_game_mode_from_update(update: protocol::GameModeUpdate) -> ActorGameMode {
    match update {
        protocol::GameModeUpdate::Explicit(mode) => mode.into(),
        protocol::GameModeUpdate::WorldDefault => ActorGameMode::Fallback,
        protocol::GameModeUpdate::Unknown(value) => ActorGameMode::Unknown(value),
    }
}
