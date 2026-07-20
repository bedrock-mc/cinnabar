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
            default_game_mode: ActorGameMode::Survival,
            actors: HashMap::new(),
            unique_to_runtime: HashMap::new(),
            players: HashMap::new(),
            player_unique_ids: HashMap::new(),
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
    #[cfg(test)]
    pub(crate) fn begin_session(&mut self, session_id: u64, dimension: i32) {
        self.session_id = session_id;
        self.dimension = dimension;
        self.latest_sequence = 0;
        self.default_game_mode = ActorGameMode::Survival;
        self.actors.clear();
        self.unique_to_runtime.clear();
        self.players.clear();
        self.player_unique_ids.clear();
        self.retained_player_skin_bytes = 0;
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
                let network_position_offset =
                    if movement.position_origin == ActorPositionOrigin::NetworkOffset {
                        actor.network_position_offset()
                    } else {
                        0.0
                    };
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
                let elapsed_seconds = movement
                    .source_tick
                    .zip(actor.source_tick)
                    .and_then(|(current, previous)| current.checked_sub(previous))
                    .filter(|ticks| *ticks > 0)
                    .map_or(0.05, |ticks| ticks as f32 * 0.05);
                let derived_velocity = if movement.snap {
                    [0.0; 3]
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
                actor.received_pose = received;
                if movement.snap {
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
                    return ActorApplyResult::MissingActor;
                };
                let Some(actor) = self.actors.get_mut(&runtime_id) else {
                    return ActorApplyResult::MissingActor;
                };
                if !matches!(actor.kind, ActorKind::Player { .. }) {
                    return ActorApplyResult::MissingActor;
                }
                actor.game_mode = Some(update.game_mode);
                actor.resolved_game_mode =
                    Some(update.game_mode.resolve_fallback(self.default_game_mode));
                actor.game_mode_tick = Some(update.tick);
                ActorApplyResult::Updated
            }
            ActorEvent::DefaultGameMode(update) => {
                self.set_default_game_mode(update.game_mode);
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
                self.rebuild_player_unique_ids();
                self.rebind_player_rigs();
                if capacity_rejected {
                    ActorApplyResult::CapacityRejected
                } else {
                    ActorApplyResult::Updated
                }
            }
        }
    }

    fn rebuild_player_unique_ids(&mut self) {
        self.player_unique_ids.clear();
        for (uuid, profile) in &self.players {
            self.player_unique_ids
                .entry(profile.unique_id)
                .and_modify(|entry| *entry = None)
                .or_insert(Some(*uuid));
        }
    }

    fn rebind_player_rigs(&mut self) {
        let runtime_ids = self
            .actors
            .iter()
            .filter_map(|(&runtime_id, actor)| {
                matches!(actor.kind, ActorKind::Player { .. }).then_some(runtime_id)
            })
            .collect::<Vec<_>>();
        for runtime_id in runtime_ids {
            let geometry =
                self.player_profile(runtime_id)
                    .and_then(|profile| match &profile.skin {
                        PlayerSkin::Standard(skin) => Some(skin.geometry.clone()),
                        PlayerSkin::Unavailable(_) => None,
                    });
            let desired_identity = geometry.as_ref().map(|geometry| match geometry {
                protocol::PlayerSkinGeometry::Wide => ("geometry.humanoid.custom", None),
                protocol::PlayerSkinGeometry::Slim => ("geometry.humanoid.customSlim", None),
                protocol::PlayerSkinGeometry::Custom {
                    identifier,
                    data_sha256,
                } => (identifier.as_ref(), Some(data_sha256)),
            });
            let current_matches = self.animation.get(runtime_id).is_some_and(|rig| {
                desired_identity.is_some_and(|(identifier, expected_sha256)| {
                    rig.geometry_identifier == identifier
                        && expected_sha256.is_none_or(|expected| rig.geometry_sha256 == *expected)
                })
            });
            if current_matches
                || (desired_identity.is_none() && self.animation.get(runtime_id).is_none())
            {
                continue;
            }
            if let Some(actor) = self.actors.get(&runtime_id) {
                self.animation
                    .insert(self.session_id, self.dimension, actor, geometry.as_ref());
            }
        }
    }
    pub(crate) fn advance_interpolation_ticks(&mut self, ticks: u32) {
        for _ in 0..ticks {
            for actor in self.actors.values_mut() {
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
            self.animation.advance_tick(&self.actors);
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
                snap: movement.teleported,
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
            self.animation.remove_runtime(previous.runtime_id);
            self.items.remove(lifetime);
            self.actions.remove(lifetime);
            replaced = true;
        }
        if let Some(previous_runtime) = self.unique_to_runtime.remove(&spawn.unique_id) {
            if let Some(previous) = self.actors.remove(&previous_runtime) {
                let lifetime = self.lifetime_for(&previous);
                self.items.remove(lifetime);
                self.actions.remove(lifetime);
            }
            self.animation.remove_runtime(previous_runtime);
            replaced = true;
        }
        let runtime_id = spawn.runtime_id;
        let unique_id = spawn.unique_id;
        let held_item = spawn.held_item.clone();
        self.actors.insert(
            runtime_id,
            ActorSnapshot::from_spawn(spawn, sequence, self.default_game_mode),
        );
        self.unique_to_runtime.insert(unique_id, runtime_id);
        if let Some(actor) = self.actors.get(&runtime_id) {
            let geometry =
                self.player_profile(runtime_id)
                    .and_then(|profile| match &profile.skin {
                        PlayerSkin::Standard(skin) => Some(skin.geometry.clone()),
                        PlayerSkin::Unavailable(_) => None,
                    });
            self.animation
                .insert(self.session_id, self.dimension, actor, geometry.as_ref());
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
        let Some(runtime_id) = self.unique_to_runtime.remove(&unique_id) else {
            return ActorApplyResult::MissingActor;
        };
        if let Some(actor) = self.actors.remove(&runtime_id) {
            let lifetime = self.lifetime_for(&actor);
            self.items.remove(lifetime);
            self.actions.remove(lifetime);
        }
        self.animation.remove_runtime(runtime_id);
        ActorApplyResult::Removed
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
