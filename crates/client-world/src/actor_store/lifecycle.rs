use super::*;
use protocol_ids::Attribute;
mod spawn;

mod equipment;
mod interpolation;

impl ActorStore {
    pub(crate) fn set_server_animation_compiler(
        &mut self,
        compiler: actor_animation::ServerAnimationCompiler,
    ) {
        self.animation.set_server_animation_compiler(compiler);
    }

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
            actor_animation::ActorAnimationStore::diagnostic(),
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
            actor_animation::ActorAnimationStore::with_assets(std::sync::Arc::clone(
                &entity_assets,
            )),
        );
        store.entity_assets = Some(std::sync::Arc::clone(&entity_assets));
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
        animation: actor_animation::ActorAnimationStore,
    ) -> Self {
        Self {
            session_id,
            dimension,
            latest_sequence: 0,
            max_actors,
            max_players,
            max_player_skin_bytes,
            retained_player_skin_bytes: 0,
            ignored_movement_components: 0,
            actor_identifier_skips: 0,
            painting_motive_fallbacks: 0,
            player_game_mode_skips: 0,
            world_default_game_mode: None,
            entity_assets: None,
            custom_actor_facts: HashMap::new(),
            actors: HashMap::new(),
            hitbox_budget: Default::default(),
            effect_members: Default::default(),
            unique_to_runtime: FastHashMap::new(),
            rider_to_ridden: FastHashMap::new(),
            max_actor_links: max_actors.min(MAX_TRACKED_ACTOR_LINKS),
            players: HashMap::new(),
            unlisted_players: HashMap::new(),
            animation,
            ready_appearances: Default::default(),
            items: crate::item::ItemStateStore::diagnostic(),
            actions: crate::action::RemoteActionStore::diagnostic(),
            remote_state_excluded_runtime_id: None,
            pending_local_health: None,
            local_server_swings: Vec::new(),
            pending_local_damage: None,
            local_health_skips: 0,
            pending_local_death: Vec::new(),
            local_player_spawned: false,
            synthetic_local_uuid: None,
            synthetic_local_skin: None,
            local_skin_fingerprints: Default::default(),
            synthetic_local_skin_pending: false,
            synthetic_local_revision: 0,
            local_first_person: false,
            local_view_dirty: false,
            pick_states: Vec::new(),
            picks_ahead: false,
            local_view_bobbing: true,
            local_flying: false,
            local_hands: [None, None],
            local_equipment_count: 0,
            local_held_network_ids: [0; 2],
            local_main_metadata: 0,
            local_held_variants: Default::default(),
            local_main_stack_id: None,
            local_main_slot: 0,
            local_item_use: LocalItemUse::Unpredicted,
            local_bedrock_swing_ticks: actor_animation::ACTOR_SWING_TICKS,
            local_java_swing_ticks: actor_animation::ACTOR_SWING_TICKS,
            camera_rotation: [0.0; 2],
            camera_position: [0.0; 3],
            animation_view: None,
            seat_bindings: FastHashMap::new(),
            seat_bases: HashMap::new(),
            property_registry: Default::default(),
            local_knockback: None,
            status_notices: Vec::new(),
            pickup_visuals: Vec::new(),
            particle_effects: Default::default(),
            synchronized_audio: Default::default(),
        }
    }

    /// Arm swings the server sent for the local player since the last call.
    pub(crate) fn take_local_server_swings(&mut self) -> Vec<protocol::SwingSource> {
        std::mem::take(&mut self.local_server_swings)
    }

    pub(crate) fn exclude_remote_state_for(&mut self, runtime_id: u64) {
        self.remote_state_excluded_runtime_id = Some(runtime_id);
        self.items.set_persistent_armor_runtime(runtime_id);
        if let Some(lifetime) = self.lifetime(runtime_id) {
            self.items.remove(lifetime);
            self.actions.remove(lifetime);
        }
    }

    /// Starts an actor's arm swing from a local cause rather than a server action.
    pub(crate) fn start_swing(&mut self, runtime_id: u64, ticks: i32) {
        self.animation.start_swing(runtime_id, ticks, ticks);
    }

    /// Retains a remote actor's swing-length effects until the server removes them; the local
    /// player's come from its own feed.
    pub(crate) fn apply_effect(&mut self, event: &protocol::ActorEffectEvent) {
        if event.dimension != self.dimension
            || self.remote_state_excluded_runtime_id == Some(event.actor_runtime_id)
        {
            return;
        }
        let Some(actor) = self.actors.get_mut(&event.actor_runtime_id) else {
            return;
        };
        let amplifier = match event.action {
            protocol::ActorEffectAction::Add | protocol::ActorEffectAction::Update => {
                Some(event.amplifier)
            }
            protocol::ActorEffectAction::Remove => None,
            protocol::ActorEffectAction::Unknown(_) => return,
        };
        actor.status.swing_effects.apply(event.effect_id, amplifier);
    }

    pub(crate) fn reset_java_equip(&mut self, runtime_id: u64) {
        self.animation.reset_java_equip(runtime_id);
    }

    /// Feeds the client-authored local-player pose into the shared actor rig, spawning the
    /// synthetic actor on the first call so `actor_rigs()` drives its third-person body.
    /// Items and actions stay client-owned via `exclude_remote_state_for`.
    pub(crate) fn sync_local_player(
        &mut self,
        runtime_id: u64,
        unique_id: i64,
        feed: &LocalPlayerFeed,
    ) {
        if runtime_id == 0 {
            return;
        }
        self.local_view_dirty |= self.local_first_person != feed.first_person;
        self.local_first_person = feed.first_person;
        self.local_view_bobbing = feed.view_bobbing;
        self.local_flying = feed.flying;
        self.local_hands = [feed.main_hand.clone(), feed.off_hand.clone()];
        self.local_view_dirty |= self.local_equipment_count != feed.equipment_count;
        self.local_equipment_count = feed.equipment_count;
        self.local_held_network_ids = feed.held_network_ids;
        self.local_main_metadata = feed.main_hand_metadata;
        self.local_held_variants = feed.held_variants;
        self.local_main_stack_id = feed.main_hand_stack_id.filter(|id| *id > 0);
        self.local_main_slot = feed.main_hand_slot;
        self.local_view_dirty |= self.local_item_use != feed.item_use;
        self.local_item_use = feed.item_use;
        self.local_bedrock_swing_ticks = feed.bedrock_swing_ticks;
        self.local_java_swing_ticks = feed.java_swing_ticks;
        let pose = ActorPose {
            y_origin: Default::default(),
            position: feed.position,
            pitch: feed.pitch,
            yaw: feed.yaw,
            head_yaw: feed.head_yaw,
        };
        self.synthetic_local_revision = self.synthetic_local_revision.saturating_add(1);
        let revision = self.synthetic_local_revision.max(1);
        let (uuid, username) = self.resolve_local_identity(unique_id, feed);
        if let Some(actor) = self.actors.get_mut(&runtime_id) {
            if let Some(mode) = feed.game_mode {
                let mode = Some(protocol::GameModeUpdate::Explicit(mode));
                self.local_view_dirty |= actor.player_game_mode != mode;
                actor.player_game_mode = mode;
            }
            // Adopt the player-list identity once it arrives so the skin resolves by uuid.
            if let ActorKind::Player {
                uuid: current_uuid,
                username: current_username,
            } = &mut actor.kind
                && *current_uuid != uuid
            {
                *current_uuid = uuid;
                *current_username = username;
            }
            actor.received_pose = pose;
            actor.status.movement_interpolation = Default::default();
            actor.velocity = feed.velocity;
            actor.status.native_velocity = feed.velocity;
            actor.status.fall_fly_ticks = feed.fall_fly_ticks;
            actor.on_ground = Some(feed.on_ground);
            actor.movement_revision = revision;
            actor.teleported = feed.teleported;
            actor.apply_local_flags(feed);
            // A zero remaining count lands each tick exactly on the fed pose (no server-style
            // easing), so the body tracks local physics without lag.
            actor.interpolation_ticks_remaining = 0;
            if feed.teleported {
                actor.previous_pose = pose;
                actor.set_current_pose(pose);
                self.animation.mark_reset(runtime_id);
            }
            self.effect_members.collidable.update(actor);
            return;
        }
        let mut actor =
            ActorSnapshot::local_player(unique_id, runtime_id, revision, uuid, username, feed);
        actor.player_game_mode = feed.game_mode.map(protocol::GameModeUpdate::Explicit);
        self.unique_to_runtime.insert(unique_id, runtime_id);
        self.effect_members.update(&mut actor);
        self.bind_actor_seat(&actor);
        self.actors.insert(runtime_id, actor);
        self.apply_pending_local_health(runtime_id);
        self.apply_pending_local_death(runtime_id);
        if let Some(actor) = self.actors.get(&runtime_id) {
            self.animation
                .insert(self.session_id, self.dimension, actor);
        }
    }
    /// Retains server appearances while an explicit client preference selects a synthetic profile.
    /// Unchanged default feeds preserve server skin updates between pose samples.
    fn resolve_local_identity(
        &mut self,
        unique_id: i64,
        feed: &LocalPlayerFeed,
    ) -> ([u8; 16], std::sync::Arc<str>) {
        let synthetic = self.synthetic_local_uuid;
        if !feed.prefer_client_skin
            && let Some((uuid, username)) = self
                .players
                .iter()
                .chain(self.unlisted_players.iter())
                .find(|(uuid, profile)| Some(**uuid) != synthetic && profile.unique_id == unique_id)
                .map(|(uuid, profile)| (*uuid, std::sync::Arc::clone(&profile.username)))
        {
            if let Some(stale) = self.synthetic_local_uuid.take()
                && stale != uuid
            {
                self.remove_profile(&stale);
            }
            self.synthetic_local_skin = None;
            self.local_skin_fingerprints.clear();
            self.synthetic_local_skin_pending = false;
            return (uuid, username);
        }
        let uuid = if feed.prefer_client_skin {
            synthetic.unwrap_or_else(|| self.available_profile_uuid(feed.uuid))
        } else {
            feed.uuid
        };
        let skin_fingerprint = self.local_skin_fingerprints.fingerprint(&feed.skin);
        let stale = match self
            .players
            .get(&uuid)
            .or_else(|| self.unlisted_players.get(&uuid))
        {
            Some(profile) => {
                profile.unique_id != unique_id
                    || synthetic != Some(uuid)
                    || self.synthetic_local_skin != Some(skin_fingerprint)
                    || self.synthetic_local_skin_pending
            }
            None => true,
        };
        if stale {
            self.upsert_profile(
                uuid,
                PlayerProfile {
                    unique_id,
                    username: std::sync::Arc::clone(&feed.username),
                    verified: false,
                    xuid: "".into(),
                    skin: feed.skin.clone(),
                },
                None,
            );
            self.synthetic_local_skin_pending = self
                .players
                .get(&uuid)
                .or_else(|| self.unlisted_players.get(&uuid))
                .is_none_or(|profile| profile.skin != feed.skin);
        }
        self.synthetic_local_uuid = Some(uuid);
        self.synthetic_local_skin = Some(skin_fingerprint);
        (uuid, std::sync::Arc::clone(&feed.username))
    }

    #[cfg(test)]
    pub(crate) fn begin_session(&mut self, session_id: u64, dimension: i32) {
        self.session_id = session_id;
        self.pending_local_health = None;
        self.pending_local_death.clear();
        self.pending_local_damage = None;
        self.local_health_skips = 0;
        self.local_player_spawned = false;
        self.local_flying = false;
        self.picks_ahead = false;
        self.dimension = dimension;
        self.latest_sequence = 0;
        self.custom_actor_facts.clear();
        self.actor_identifier_skips = 0;
        self.player_game_mode_skips = 0;
        self.world_default_game_mode = None;
        self.actors.clear();
        self.seat_bindings.clear();
        self.seat_bases.clear();
        self.effect_members = Default::default();
        self.unique_to_runtime.clear();
        self.rider_to_ridden.clear();
        self.players.clear();
        self.unlisted_players.clear();
        self.synthetic_local_uuid = None;
        self.synthetic_local_skin = None;
        self.local_skin_fingerprints.clear();
        self.synthetic_local_skin_pending = false;
        self.retained_player_skin_bytes = 0;
        self.animation.clear();
        self.ready_appearances = Default::default();
        self.items.clear();
        self.actions.clear();
        self.local_server_swings.clear();
        self.status_notices.clear();
        self.pickup_visuals.clear();
        self.particle_effects.clear();
        self.synchronized_audio.clear();
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
        self.local_server_swings.clear();
        self.local_flying = false;
        self.retain_local_death();
        self.pending_local_health = self
            .remote_state_excluded_runtime_id
            .and_then(|runtime_id| self.actors.get(&runtime_id))
            .and_then(|actor| actor.attributes.get(Attribute::Health.wire_name()))
            .cloned()
            .or_else(|| self.pending_local_health.take());
        self.pending_local_damage = self
            .remote_state_excluded_runtime_id
            .and_then(|runtime_id| self.actors.get(&runtime_id))
            .map(|actor| super::local_health::PendingLocalDamage {
                damage: actor.status.damage,
                hurt_time: actor.status.hurt_time,
            })
            .or_else(|| self.pending_local_damage.take());
        self.actors.clear();
        self.seat_bindings.clear();
        self.effect_members = Default::default();
        self.unique_to_runtime.clear();
        self.rider_to_ridden.clear();
        // The real player list survives a dimension change, but the synthetic local profile is
        // tied to the cleared actor and is re-inserted on the next pose feed.
        if let Some(uuid) = self.synthetic_local_uuid.take() {
            self.remove_profile(&uuid);
        }
        self.synthetic_local_skin = None;
        self.local_skin_fingerprints.clear();
        self.synthetic_local_skin_pending = false;
        self.prune_unlisted_players();
        self.animation.clear();
        self.ready_appearances = Default::default();
        self.items.clear_actor_state();
        self.actions.clear();
        self.status_notices.clear();
        self.pickup_visuals.clear();
        self.particle_effects.clear();
        self.synchronized_audio.clear();
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
            ActorEvent::Identifiers(registry) => {
                self.apply_seat_bases(&registry);
                self.apply_actor_interactions(registry)
            }
            ActorEvent::Spawn(spawn) => self.apply_spawn(sequence, spawn),
            ActorEvent::PaintingSpawn { spawn, painting } => {
                self.apply_painting_spawn(sequence, spawn, painting)
            }
            ActorEvent::PlayerSpawn { spawn, game_mode } => {
                let unique_id = spawn.unique_id;
                let result = self.apply_spawn(sequence, spawn);
                if matches!(
                    result,
                    ActorApplyResult::Inserted | ActorApplyResult::Replaced
                ) {
                    self.apply_player_game_mode(unique_id, game_mode);
                }
                result
            }
            ActorEvent::Remove(remove) => self.remove_unique(remove.unique_id),
            ActorEvent::Move(movement) => {
                // The local player's pose is client-fed each tick; server movement
                // (authoritative reconciliation) must not fight that feed.
                if self.remote_state_excluded_runtime_id == Some(movement.runtime_id) {
                    return ActorApplyResult::MissingActor;
                }
                let Some(actor) = self.actors.get_mut(&movement.runtime_id) else {
                    return ActorApplyResult::MissingActor;
                };
                let Some(duration) =
                    super::movement_interpolation::duration(movement.interpolation)
                else {
                    let previous = self.ignored_movement_components;
                    self.ignored_movement_components = previous.saturating_add(1);
                    if previous == 0 || self.ignored_movement_components / 64 > previous / 64 {
                        tracing::warn!(
                            "ignored unsupported actor movement duration (total {})",
                            self.ignored_movement_components
                        );
                    }
                    return ActorApplyResult::Updated;
                };
                let previous_received = actor.last_received_pose();
                let mut received = previous_received;
                let network_position_offset =
                    if movement.position_origin == ActorPositionOrigin::NetworkOffset {
                        actor.network_position_offset()
                    } else {
                        0.0
                    };
                let mut ignored = 0_u64;
                let mut merge = |target: &mut f32, source: Option<f32>| {
                    if let Some(value) = source {
                        if value.is_finite() {
                            *target = value;
                        } else {
                            ignored += 1;
                        }
                    }
                };
                for (axis, (target, source)) in received
                    .position
                    .iter_mut()
                    .zip(movement.position)
                    .enumerate()
                {
                    merge(
                        target,
                        source.map(|value| {
                            if axis == 1 {
                                value - network_position_offset
                            } else {
                                value
                            }
                        }),
                    );
                }
                if movement.position[1].is_some_and(f32::is_finite) {
                    received.y_origin =
                        if movement.position_origin == ActorPositionOrigin::NetworkOffset {
                            PoseYOrigin::NETWORK
                        } else {
                            PoseYOrigin::default()
                        };
                }
                merge(&mut received.pitch, movement.pitch);
                merge(&mut received.yaw, movement.yaw);
                merge(&mut received.head_yaw, movement.head_yaw);
                if ignored > 0 {
                    let previous = self.ignored_movement_components;
                    self.ignored_movement_components = previous.saturating_add(ignored);
                    if previous == 0 || self.ignored_movement_components / 64 > previous / 64 {
                        tracing::warn!(
                            "ignored non-finite actor movement components: {}",
                            self.ignored_movement_components
                        );
                    }
                }
                if let Some(value) = movement.on_ground {
                    actor.on_ground = Some(value);
                }
                let tick_seconds = simulation_clock::TICK_DURATION.as_secs_f32();
                let elapsed_seconds = movement
                    .source_tick
                    .zip(actor.source_tick)
                    .and_then(|(current, previous)| current.checked_sub(previous))
                    .filter(|ticks| *ticks > 0)
                    .map_or(tick_seconds, |ticks| ticks as f32 * tick_seconds);
                let derived_velocity = if movement.teleported {
                    [0.0; 3]
                } else {
                    std::array::from_fn(|axis| {
                        (received.position[axis] - previous_received.position[axis])
                            / elapsed_seconds
                    })
                };
                // A rotation-only move carries no displacement to derive from.
                if movement.position.iter().any(Option::is_some) {
                    actor.velocity = if derived_velocity.iter().all(|value| value.is_finite()) {
                        derived_velocity
                    } else {
                        [0.0; 3]
                    };
                }
                actor.start_movement_interpolation(
                    received,
                    duration,
                    movement.interpolation.force_completion,
                    movement.teleported,
                );
                actor.movement_revision = sequence;
                actor.teleported = movement.teleported;
                actor.player_mode = movement.player_mode;
                actor.source_tick = movement.source_tick;
                self.effect_members.collidable.update(actor);
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
                    if self.remote_state_excluded_runtime_id == Some(update.runtime_id) {
                        self.merge_pending_local_death(&update.metadata);
                    }
                    return ActorApplyResult::MissingActor;
                };
                let incompatible = update.metadata.iter().any(|metadata| {
                    actor.metadata.get(&metadata.key).is_some_and(|previous| {
                        std::mem::discriminant(previous) != std::mem::discriminant(&metadata.value)
                    })
                });
                let rejected = actor.apply_metadata(&update.metadata, &mut self.hitbox_budget)
                    | actor.apply_properties(&update.properties);
                self.effect_members.update(actor);
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
                self.retain_unspawned_local_health(update.runtime_id, &update.attributes);
                let Some(actor) = self.actors.get_mut(&update.runtime_id) else {
                    return ActorApplyResult::MissingActor;
                };
                let rejected = actor.apply_attributes(&update.attributes);
                actor.sync_status_from_health();
                if rejected {
                    ActorApplyResult::CapacityRejected
                } else {
                    ActorApplyResult::Updated
                }
            }
            ActorEvent::Status(status) => self.apply_status(status),
            ActorEvent::TakeItem(take) => self.apply_take_item(take),
            ActorEvent::Skin { uuid, skin } => self.apply_skin_update(uuid, skin),
            ActorEvent::PlayerList(update) => {
                let mut capacity_rejected = false;
                // Several additions share one actor pass; upserts never change the actors.
                let spawned = update
                    .entries
                    .iter()
                    .filter(|entry| matches!(entry, PlayerListEntry::Add { .. }))
                    .nth(1)
                    .is_some()
                    .then(|| self.spawned_player_unique_ids());
                for entry in update.entries.iter() {
                    match entry {
                        PlayerListEntry::Add {
                            uuid,
                            unique_id,
                            username,
                            verified,
                            xuid,
                            skin,
                        } => {
                            let admitted = self.upsert_profile(
                                *uuid,
                                PlayerProfile {
                                    unique_id: *unique_id,
                                    username: username.clone(),
                                    verified: *verified,
                                    xuid: xuid.clone(),
                                    skin: skin.clone(),
                                },
                                spawned.as_ref(),
                            );
                            capacity_rejected |= !admitted;
                            if admitted && self.synthetic_local_uuid == Some(*uuid) {
                                self.synthetic_local_uuid = None;
                            }
                        }
                        PlayerListEntry::Remove { uuid } => {
                            self.unlist_player(uuid);
                        }
                    }
                }
                if capacity_rejected {
                    tracing::debug!(
                        players = self.players.len(),
                        "Cinnabar world roster capacity rejected"
                    );
                    ActorApplyResult::CapacityRejected
                } else {
                    tracing::debug!(
                        players = self.players.len(),
                        identities = self
                            .players
                            .values()
                            .filter(|profile| !profile.xuid.is_empty())
                            .count(),
                        "Cinnabar world roster committed"
                    );
                    ActorApplyResult::Updated
                }
            }
        }
    }

    pub(crate) fn apply_link(
        &mut self,
        session_id: u64,
        sequence: u64,
        event: ActorLinkEvent,
    ) -> ActorApplyResult {
        let guard = self.guard(session_id, sequence);
        if guard != ActorApplyResult::Updated {
            return guard;
        }
        if event.dimension != self.dimension {
            return ActorApplyResult::StaleDimension;
        }
        self.apply_link_inner(event)
    }
    pub(crate) fn apply_player_move(
        &mut self,
        session_id: u64,
        sequence: u64,
        dimension: i32,
        movement: MovePlayerEvent,
    ) -> ActorApplyResult {
        // As in vanilla, Reset sets the position directly and
        // Rotation turns the player without any position request.
        let rotation_only = movement.mode == protocol::MovePlayerMode::Rotation;
        self.apply(
            session_id,
            sequence,
            ActorEvent::Move(ActorMoveEvent {
                dimension,
                runtime_id: movement.runtime_id,
                position: if rotation_only {
                    [None; 3]
                } else {
                    movement.position.map(Some)
                },
                position_origin: ActorPositionOrigin::NetworkOffset,
                pitch: Some(movement.pitch),
                yaw: Some(movement.yaw),
                head_yaw: Some(movement.head_yaw),
                on_ground: Some(movement.on_ground),
                teleported: movement.teleported || movement.mode == protocol::MovePlayerMode::Reset,
                force_local_move: false,
                player_mode: Some(movement.mode),
                source_tick: Some(movement.source_tick),
                interpolation: Default::default(),
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
    fn remove_unique(&mut self, unique_id: i64) -> ActorApplyResult {
        let Some(runtime_id) = self.unique_to_runtime.remove(&unique_id) else {
            self.remove_links_for(unique_id);
            return ActorApplyResult::MissingActor;
        };
        self.seat_bindings.remove(&runtime_id);
        if let Some(actor) = self.actors.remove(&runtime_id) {
            self.effect_members.remove(runtime_id);
            self.synchronized_audio.remove_runtime(runtime_id);
            let lifetime = self.lifetime_for(&actor);
            self.items.remove(lifetime);
            self.actions.remove(lifetime);
        }
        self.remove_links_for(unique_id);
        self.animation.remove_runtime(runtime_id);
        self.prune_unlisted_players();
        ActorApplyResult::Removed
    }

    fn apply_link_inner(&mut self, event: ActorLinkEvent) -> ActorApplyResult {
        Self::apply_link_to(&mut self.rider_to_ridden, self.max_actor_links, event)
    }

    fn apply_link_to(
        rider_to_ridden: &mut FastHashMap<i64, i64>,
        max_actor_links: usize,
        event: ActorLinkEvent,
    ) -> ActorApplyResult {
        match event.link_type {
            ActorLinkType::Unknown(_) => ActorApplyResult::Updated,
            ActorLinkType::Remove => {
                if rider_to_ridden.get(&event.rider_unique_id) == Some(&event.ridden_unique_id) {
                    rider_to_ridden.remove(&event.rider_unique_id);
                }
                ActorApplyResult::Updated
            }
            ActorLinkType::Rider | ActorLinkType::Passenger => {
                if !rider_to_ridden.contains_key(&event.rider_unique_id)
                    && rider_to_ridden.len() >= max_actor_links
                {
                    return ActorApplyResult::CapacityRejected;
                }
                rider_to_ridden.insert(event.rider_unique_id, event.ridden_unique_id);
                ActorApplyResult::Updated
            }
        }
    }

    fn remove_links_for(&mut self, unique_id: i64) {
        self.rider_to_ridden.remove(&unique_id);
        self.rider_to_ridden
            .retain(|_, ridden_unique_id| *ridden_unique_id != unique_id);
    }

    /// Layers a session's server-pack entity catalog over the vanilla one.
    pub(crate) fn set_pack_entities(
        &mut self,
        assets: Option<(std::sync::Arc<assets::RuntimeEntityAssets>, Vec<u32>)>,
    ) {
        self.actions.set_pack(
            assets
                .as_ref()
                .map(|(catalog, _)| std::sync::Arc::clone(catalog)),
        );
        self.animation.set_pack(assets);
        // Cinnabar live reload also rebinds actors already present in the world.
        for actor in self.actors.values() {
            self.animation
                .insert(self.session_id, self.dimension, actor);
        }
    }

    /// Installs compiled item-use overlays before numerical admission.
    pub(crate) fn set_item_use_overlays(
        &mut self,
        overlays: std::sync::Arc<
            std::collections::BTreeMap<Box<str>, assets::equipment::CompiledItemUseOverlay>,
        >,
    ) {
        self.items.set_use_overlays(overlays)
    }

    pub(crate) fn set_item_use_durations(
        &mut self,
        durations: std::sync::Arc<std::collections::BTreeMap<Box<str>, u32>>,
    ) {
        self.items.set_use_durations(durations);
    }

    /// Installs effective item-directed attack and kinetic presentation facts.
    pub(crate) fn set_item_attack_timings(
        &mut self,
        timings: std::sync::Arc<std::collections::BTreeMap<Box<str>, protocol::ItemAttackTiming>>,
    ) {
        self.items.set_attack_timings(timings);
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
                    // The native handler swings whichever mob the packet names, the local player
                    // included; its swing plays through the local swing tracker.
                    if self.remote_state_excluded_runtime_id == Some(runtime_id)
                        && matches!(action.kind, protocol::ActorActionKind::SwingArm)
                    {
                        self.local_server_swings
                            .push(protocol::SwingSource::from_wire_name(
                                action.swing_source.as_deref(),
                            ));
                    }
                    if (self.remote_state_excluded_runtime_id == Some(runtime_id)
                        && !matches!(action.kind, protocol::ActorActionKind::Custom { .. }))
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
                let prepared = self.animation.prepare_server_animation(&action);
                let mut accepted = false;
                for (lifetime, rig) in targets {
                    let source_tick = ActorSourceTick::IngressSequence(sequence);
                    let applied = self
                        .actions
                        .apply(lifetime, rig, sequence, source_tick, &action);
                    if applied && matches!(action.kind, protocol::ActorActionKind::SwingArm) {
                        let effects = self
                            .actors
                            .get(&lifetime.runtime_id)
                            .map(|actor| actor.status.swing_effects)
                            .unwrap_or_default();
                        self.animation.start_swing(
                            lifetime.runtime_id,
                            effects.bedrock_ticks(render_api::DEFAULT_COMBAT_VISUALS),
                            effects.ticks(),
                        );
                    }
                    if applied && let Some(request) = prepared.as_ref() {
                        self.animation.start_server_animation(
                            lifetime.runtime_id,
                            std::sync::Arc::clone(request),
                        );
                    }
                    accepted |= applied;
                }
                if accepted {
                    ActorApplyResult::Updated
                } else {
                    ActorApplyResult::MissingActor
                }
            }
        }
    }

    /// Resolves the full identity of the actor currently using this runtime ID.
    pub(crate) fn lifetime(&self, runtime_id: u64) -> Option<ActorLifetimeId> {
        self.actors
            .get(&runtime_id)
            .map(|actor| self.lifetime_for(actor))
    }

    pub(super) const fn lifetime_for(&self, actor: &ActorSnapshot) -> ActorLifetimeId {
        ActorLifetimeId {
            session_id: self.session_id,
            dimension: self.dimension,
            runtime_id: actor.runtime_id,
            spawn_revision: actor.spawn_revision,
        }
    }
}
