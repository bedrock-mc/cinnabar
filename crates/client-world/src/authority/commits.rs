use super::*;
use protocol_ids::Attribute;

impl WorldAuthority {
    /// Applies a non-terrain event at its current ordered commit position.
    // Keep a routing failure intact without adding an allocation to every commit.
    #[allow(clippy::result_large_err)]
    pub fn apply_ordered_event(
        &mut self,
        event: WorldEvent,
        sequence: Option<u64>,
    ) -> Result<(), WorldEvent> {
        match event {
            WorldEvent::DimensionHeights(heights) => self.apply_dimension_heights(&heights),
            WorldEvent::DimensionChangeAck { .. } => {
                self.push_committed_control(CommittedControlEvent::DimensionChangeAck {
                    sequence: sequence.expect("dimension acknowledgement commits through submit"),
                    dimension_epoch: self.form_dimension_epoch,
                });
            }
            WorldEvent::NetworkStackLatency(creation_time) => {
                let sequence = sequence.expect("latency probes commit through submit");
                self.push_committed_control(CommittedControlEvent::NetworkStackLatency {
                    sequence,
                    creation_time,
                });
            }
            WorldEvent::ActorMotion(motion) => {
                let sequence = sequence.expect("sequenced actor motion commits through submit");
                if motion.actor_runtime_id != self.local_player_runtime_id {
                    self.actors.apply_motion(sequence, motion);
                    return Ok(());
                }
                self.actors.note_local_knockback(sequence, motion.motion);
                self.push_committed_control(CommittedControlEvent::LocalActorMotion {
                    sequence,
                    event: motion,
                });
            }
            WorldEvent::MovementEffect(event) => {
                let sequence = sequence.expect("sequenced movement effects commit through submit");
                if event.actor_runtime_id == self.local_player_runtime_id {
                    self.push_committed_control(CommittedControlEvent::LocalMovementBoost {
                        sequence,
                        event,
                    });
                }
            }
            WorldEvent::WorldSpawn(update) => self.world_spawn = update.position,
            WorldEvent::SetTime(update) => {
                let sequence = sequence.expect("sequenced SetTime commits through submit");
                self.push_committed_control(CommittedControlEvent::SetTime { sequence, update });
            }
            WorldEvent::WorldClocks(updates) => {
                let sequence = sequence.expect("sequenced world clocks commit through submit");
                for update in updates {
                    self.push_committed_control(CommittedControlEvent::WorldClocks {
                        sequence,
                        update,
                    });
                }
            }
            WorldEvent::GameRules(rules) => {
                let sequence = sequence.expect("sequenced game rules commit through submit");
                if let Some(update) = rules.daylight_cycle {
                    self.push_committed_control(CommittedControlEvent::DaylightCycle {
                        sequence,
                        update,
                    });
                }
                if let Some(enabled) = rules.weather_cycle {
                    self.push_committed_control(CommittedControlEvent::WeatherCycle {
                        sequence,
                        enabled,
                    });
                }
                if !rules.hud.is_empty() || !rules.death.is_empty() {
                    self.push_committed_ui(CommittedUiEvent::Ui {
                        sequence,
                        event: UiEvent::GameRules {
                            hud: rules.hud,
                            death: rules.death,
                        },
                    });
                }
            }
            WorldEvent::Weather(update) => {
                let sequence = sequence.expect("sequenced weather commits through submit");
                self.push_committed_control(CommittedControlEvent::Weather { sequence, update });
            }
            WorldEvent::Audio(event) => {
                let sequence = sequence.expect("sequenced audio events commit through submit");
                let committed = CommittedAudioEvent {
                    sequence,
                    dimension: self.current_dimension,
                    dimension_epoch: self.form_dimension_epoch,
                    client_generated: false,
                    actor_synchronization: None,
                    event,
                };
                if matches!(&committed.event, AudioEvent::Level(level) if level.fire_at_position.is_some())
                {
                    self.actors.queue_synchronized_audio(committed);
                } else {
                    self.push_committed_audio(committed);
                }
            }
            WorldEvent::PrimitiveShapes(event) => {
                assert!(self.committed_primitive_shapes.len() < MAX_ADMITTED_WORLD_EVENTS);
                self.committed_primitive_shapes.push_back(event);
            }
            WorldEvent::Camera(protocol::CameraEvent::InputLocks(locks)) => {
                let sequence = sequence.expect("input locks commit through submit");
                self.push_committed_control(CommittedControlEvent::ClientInputLocks {
                    sequence,
                    locks,
                });
            }
            WorldEvent::Camera(event) => {
                self.audio_nondefault_camera_observed = true;
                let sequence = sequence.expect("sequenced camera events commit through submit");
                self.push_committed_camera(CommittedCameraEvent { sequence, event });
            }
            WorldEvent::Actor(mut event) => {
                let sequence = sequence.expect("sequenced actor events commit through submit");
                if let ActorEvent::Move(movement) = &mut event {
                    self.commit_local_actor_move(sequence, *movement);
                    self.resolve_actor_teleport(movement);
                }
                let local_death = match &event {
                    ActorEvent::Attributes(update)
                        if update.runtime_id == self.local_player_runtime_id
                            && update.dimension == self.current_dimension =>
                    {
                        update
                            .attributes
                            .iter()
                            .filter(|attribute| {
                                attribute.name.as_ref() == Attribute::Health.wire_name()
                            })
                            .filter_map(|attribute| {
                                crate::LocalPlayerStat::from_attribute(attribute)
                                    .map(|_| attribute.current <= 0.0)
                            })
                            .next_back()
                    }
                    ActorEvent::Status(status)
                        if status.runtime_id == self.local_player_runtime_id =>
                    {
                        match status.kind {
                            protocol::ActorStatusKind::Death => Some(true),
                            _ => None,
                        }
                    }
                    _ => None,
                };
                let local_death_status = matches!(&event, ActorEvent::Status(status)
                    if status.runtime_id == self.local_player_runtime_id
                        && status.kind == protocol::ActorStatusKind::Death);
                let player_list_changed =
                    matches!(&event, ActorEvent::PlayerList(_) | ActorEvent::Skin { .. });
                let previous_mount = self.actors.ridden_unique_id(self.local_player_unique_id);
                if let ActorEvent::Attributes(update) = &event
                    && update.runtime_id == self.local_player_runtime_id
                    && update.dimension == self.current_dimension
                {
                    self.push_committed_ui(CommittedUiEvent::LocalAttributes {
                        sequence,
                        server_tick: update.tick,
                        attributes: Arc::clone(&update.attributes),
                    });
                    let movement = update
                        .attributes
                        .iter()
                        .rev()
                        .filter(|attribute| {
                            attribute.name.as_ref() == Attribute::Movement.wire_name()
                        })
                        .find_map(MovementSpeedAttribute::from_attribute);
                    let liquid = |name: &str| {
                        update
                            .attributes
                            .iter()
                            .rev()
                            .find(|attribute| attribute.name.as_ref() == name)
                            .map(|attribute| f64::from(attribute.current))
                    };
                    let underwater = liquid(Attribute::UnderwaterMovement.wire_name());
                    let lava = liquid(Attribute::LavaMovement.wire_name());
                    let air_drag_modifier = update
                        .attributes
                        .iter()
                        .rev()
                        .find(|attribute| {
                            attribute.name.as_ref() == Attribute::AirDragModifier.wire_name()
                        })
                        .map(|attribute| attribute.current)
                        .filter(|current| current.is_finite());
                    if let Some(attribute) = movement {
                        self.local_movement_speed = Some(attribute.current);
                    }
                    if movement.is_some()
                        || underwater.is_some()
                        || lava.is_some()
                        || air_drag_modifier.is_some()
                    {
                        self.push_committed_control(CommittedControlEvent::LocalMovementSpeed {
                            sequence,
                            dimension: update.dimension,
                            movement,
                            underwater,
                            lava,
                            air_drag_modifier,
                            tick: update.tick,
                        });
                    }
                }
                if let ActorEvent::Metadata(update) = &event
                    && update.runtime_id == self.local_player_runtime_id
                    && update.dimension == self.current_dimension
                {
                    self.push_committed_ui(CommittedUiEvent::LocalMetadata {
                        sequence,
                        server_tick: update.tick,
                        metadata: Arc::clone(&update.metadata),
                    });
                    if let Some(flags) = crate::MovementFlagUpdate::from_metadata(&update.metadata)
                    {
                        self.push_committed_control(CommittedControlEvent::LocalMovementFlags {
                            sequence,
                            tick: update.tick,
                            flags,
                        });
                    }
                }
                let local_hurt = matches!(
                    &event,
                    ActorEvent::Status(status)
                        if matches!(
                            status.kind,
                            protocol::ActorStatusKind::Hurt
                                | protocol::ActorStatusKind::HurtWithoutDamage
                        ) && status.runtime_id == self.local_player_runtime_id
                );
                let pickup_position = if let ActorEvent::TakeItem(take) = &event {
                    self.actors.pickup_sound_position(take.item_runtime_id)
                } else {
                    None
                };
                if matches!(&event, ActorEvent::Status(status)
                    if status.runtime_id == self.local_player_runtime_id
                        && status.kind == protocol::ActorStatusKind::TotemActivate)
                {
                    self.push_committed_ui(CommittedUiEvent::LocalTotemActivation { sequence });
                }
                let combat = match &event {
                    ActorEvent::Status(status)
                        if status.kind == protocol::ActorStatusKind::Hurt =>
                    {
                        Some((status.runtime_id, super::ActorCombatKind::Hurt))
                    }
                    ActorEvent::Status(status)
                        if status.kind == protocol::ActorStatusKind::Death =>
                    {
                        Some((status.runtime_id, super::ActorCombatKind::Death))
                    }
                    _ => None,
                };
                let totem = match &event {
                    ActorEvent::Status(status)
                        if status.kind == protocol::ActorStatusKind::TotemActivate =>
                    {
                        Some((status.runtime_id, true))
                    }
                    ActorEvent::Status(status)
                        if status.kind == protocol::ActorStatusKind::Death =>
                    {
                        Some((status.runtime_id, false))
                    }
                    _ => None,
                };
                let attributes_runtime = match &event {
                    ActorEvent::Attributes(update) => Some(update.runtime_id),
                    _ => None,
                };
                let result = self.actors.apply(self.actor_session_id, sequence, event);
                if result == crate::actor_store::ActorApplyResult::Updated
                    && let Some(runtime_id) = attributes_runtime
                    && self
                        .actors
                        .get(runtime_id)
                        .is_some_and(|actor| actor.status.dead)
                {
                    self.observe_totem_status(runtime_id, false);
                }
                if result == crate::actor_store::ActorApplyResult::Updated
                    && let Some((runtime_id, activated)) = totem
                {
                    self.observe_totem_status(runtime_id, activated);
                }
                if result == crate::actor_store::ActorApplyResult::Updated
                    && let Some((runtime_id, kind)) = combat
                {
                    self.record_combat_evidence(sequence, runtime_id, kind);
                }

                if result == crate::actor_store::ActorApplyResult::Updated
                    && let Some(position) = pickup_position
                {
                    self.push_committed_audio(CommittedAudioEvent {
                        sequence,
                        dimension: self.current_dimension,
                        dimension_epoch: self.form_dimension_epoch,
                        client_generated: false,
                        actor_synchronization: None,
                        event: AudioEvent::Level(protocol::LevelAudioEvent {
                            sound_event: "pop".into(),
                            position,
                            data: -1,
                            actor_identifier: "".into(),
                            is_baby: false,
                            is_global: false,
                            actor_unique_id: -1,
                            fire_at_position: None,
                        }),
                    });
                }
                if let Some(dead) = local_death
                    && matches!(
                        result,
                        crate::actor_store::ActorApplyResult::Updated
                            | crate::actor_store::ActorApplyResult::MissingActor
                            | crate::actor_store::ActorApplyResult::CapacityRejected
                    )
                {
                    self.publish_local_life(sequence, dead);
                    if local_death_status {
                        self.push_committed_ui(CommittedUiEvent::Ui {
                            sequence,
                            event: UiEvent::Hud(protocol::HudEvent::Health { health: 0 }),
                        });
                    }
                }
                if local_hurt {
                    self.push_committed_control(CommittedControlEvent::LocalHurt {
                        sequence,
                        source_direction: self.actors.hurt_source_direction(sequence),
                    });
                }
                if player_list_changed {
                    self.push_committed_control(CommittedControlEvent::PlayerListChanged {
                        sequence,
                    });
                }
                self.publish_local_mount_change(sequence, previous_mount);
            }
            WorldEvent::ActorEffect(event) => {
                let sequence = sequence.expect("sequenced effect events commit through submit");
                if event.actor_runtime_id == self.local_player_runtime_id
                    && event.dimension == self.current_dimension
                {
                    self.push_committed_control(CommittedControlEvent::LocalMovementEffect {
                        sequence,
                        event,
                    });
                    self.push_committed_ui(CommittedUiEvent::LocalEffect { sequence, event });
                }
                self.actors.apply_effect(&event);
            }
            WorldEvent::Abilities(event) => {
                let sequence = sequence.expect("sequenced abilities commit through submit");
                if event.actor_unique_id == self.local_player_unique_id {
                    self.push_committed_ui(CommittedUiEvent::LocalAbilities {
                        sequence,
                        stream_identity: self.biome_tint_stream_id,
                        event,
                    });
                }
            }
            // The local player's armor is its armor container (window 120);
            // vanilla ignores MobArmorEquipment addressed to itself.
            WorldEvent::ArmorEquipment(event)
                if event.actor_runtime_id != self.local_player_runtime_id =>
            {
                let sequence = sequence.expect("sequenced armor events commit through submit");
                let _ = self
                    .actors
                    .apply_armor(self.actor_session_id, sequence, &event);
            }
            WorldEvent::ArmorEquipment(_) => {}
            WorldEvent::ActorPropertySync(event) => {
                let _ = self.actors.apply_property_sync(&event);
            }
            WorldEvent::ActorLink(event) => {
                let sequence = sequence.expect("sequenced link events commit through submit");
                let previous_mount = self.actors.ridden_unique_id(self.local_player_unique_id);
                let _ = self
                    .actors
                    .apply_link(self.actor_session_id, sequence, event);
                self.publish_local_mount_change(sequence, previous_mount);
            }
            WorldEvent::Experience(event) => {
                let sequence = sequence.expect("extension events commit through submit");
                self.push_committed_ui(CommittedUiEvent::Experience {
                    sequence,
                    dimension_epoch: self.form_dimension_epoch,
                    event,
                });
            }
            WorldEvent::Ui(event) => {
                let sequence = sequence.expect("sequenced UI events commit through submit");
                // A game-mode update changes the UI only when its unique ID matches
                // the local player.
                let event = match event {
                    UiEvent::Hud(protocol::HudEvent::PlayerStatus(
                        protocol::PlayerStatus::PlayerSpawn,
                    )) => {
                        self.actors
                            .mark_local_player_spawned(self.local_player_runtime_id);
                        UiEvent::Hud(protocol::HudEvent::PlayerStatus(
                            protocol::PlayerStatus::PlayerSpawn,
                        ))
                    }
                    UiEvent::Hud(protocol::HudEvent::Health { health }) => {
                        self.actors
                            .set_local_health(self.local_player_runtime_id, health);
                        if let Ok(health) = u16::try_from(health) {
                            self.publish_local_life(sequence, health == 0);
                        }
                        if health <= 0 {
                            self.observe_totem_status(self.local_player_runtime_id, false);
                        }
                        UiEvent::Hud(protocol::HudEvent::Health { health })
                    }
                    UiEvent::ShowCredits(event)
                        if event.runtime_id != self.local_player_runtime_id =>
                    {
                        return Ok(());
                    }
                    UiEvent::PlayerGameMode {
                        actor_unique_id,
                        event,
                        ..
                    } => {
                        self.actors
                            .apply_player_game_mode(actor_unique_id, event.update);
                        if actor_unique_id != self.local_player_unique_id {
                            return Ok(());
                        }
                        UiEvent::GameMode(event)
                    }
                    UiEvent::DefaultGameMode(event) => {
                        self.actors.apply_world_game_mode(event.update);
                        UiEvent::DefaultGameMode(event)
                    }
                    event => event,
                };
                let committed = match event {
                    UiEvent::Form(event) => CommittedUiEvent::Form {
                        sequence,
                        dimension_epoch: self.form_dimension_epoch,
                        event,
                    },
                    event => CommittedUiEvent::Ui { sequence, event },
                };
                self.push_committed_ui(committed);
            }
            WorldEvent::Equipment(event) => {
                let sequence = sequence.expect("sequenced equipment commits through submit");
                let _ = self
                    .actors
                    .apply_equipment(self.actor_session_id, sequence, event);
            }
            WorldEvent::Inventory(_) => {
                // Inventory normalization lands in Task 10. Task 11's app-owned
                // authoritative store consumes this event, so WorldStream
                // deliberately owns no duplicate inventory state.
            }
            WorldEvent::ItemActor(event) => {
                let sequence = sequence.expect("sequenced item/actor events commit through submit");
                if let protocol::ItemActorEvent::Action(action) = &event
                    && matches!(
                        action.kind,
                        protocol::ActorActionKind::CriticalHit
                            | protocol::ActorActionKind::MagicCriticalHit
                    )
                {
                    let magic = matches!(action.kind, protocol::ActorActionKind::MagicCriticalHit);
                    for &actor_runtime_id in action.actor_runtime_ids.iter() {
                        self.push_committed_particle(CommittedParticleEvent {
                            sequence,
                            dimension: self.current_dimension,
                            event: protocol::ParticleEvent::ActorCritical {
                                actor_runtime_id,
                                magic,
                                particle_count: action.data,
                            },
                        });
                    }
                }
                let _ = self
                    .actors
                    .apply_item_actor(self.actor_session_id, sequence, event);
            }
            event => return Err(event),
        }
        Ok(())
    }
    /// Retains a committed control within its admission-backed queue bound.
    pub fn push_committed_control(&mut self, event: CommittedControlEvent) {
        assert!(
            self.committed_controls.len() < COMMITTED_CONTROL_CAPACITY,
            "control admission invariant exceeded bounded commit-delta capacity"
        );
        self.committed_controls.push_back(event);
    }
    /// Retains a committed UI event within its admission-backed queue bound.
    pub fn push_committed_ui(&mut self, event: CommittedUiEvent) {
        assert!(
            self.committed_ui.len() < COMMITTED_UI_CAPACITY,
            "UI admission invariant exceeded bounded commit-delta capacity"
        );
        self.committed_ui.push_back(event);
    }
    /// Retains committed audio within its admission-backed queue bound.
    pub fn push_committed_audio(&mut self, event: CommittedAudioEvent) {
        assert!(
            self.committed_audio.len() < COMMITTED_AUDIO_CAPACITY,
            "audio admission invariant exceeded bounded commit-delta capacity"
        );
        self.committed_audio.push_back(event);
    }
    /// Particle triggers are visual-only: under backpressure the oldest is dropped.
    pub fn push_committed_particle(&mut self, event: CommittedParticleEvent) {
        if self.committed_particles.len() >= COMMITTED_PARTICLE_CAPACITY {
            self.committed_particles.pop_front();
        }
        self.committed_particles.push_back(event);
    }
    /// Retains a committed camera event within its admission-backed queue bound.
    pub fn push_committed_camera(&mut self, event: CommittedCameraEvent) {
        assert!(
            self.committed_camera.len() < COMMITTED_CAMERA_CAPACITY,
            "camera admission invariant exceeded bounded commit-delta capacity"
        );
        self.committed_camera.push_back(event);
    }

    /// Publishes a local mount change after the actor mutation that caused it.
    pub fn publish_local_mount_change(&mut self, sequence: u64, previous: Option<i64>) {
        let current = self.actors.ridden_unique_id(self.local_player_unique_id);
        if current != previous {
            self.push_committed_ui(CommittedUiEvent::LocalMount {
                sequence,
                ridden_unique_id: current,
            });
        }
    }
}
