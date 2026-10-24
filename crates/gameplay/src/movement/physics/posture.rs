//! Current posture for correction syncs, independent of uncompleted local input.

use super::*;

/// Minimum squared displacement that marks the completed actor tick as moving.
const MOVING_FLAG_DISTANCE_SQUARED: f32 = 0.001;

/// Accepted flag writes and size, independent of replay history and travel-mode selection.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct SyncState {
    pub flags: client_world::MovementFlagUpdate,
    pub size: Option<[f32; 2]>,
    explicit_size: bool,
    dimensions: client_world::ExplicitDimensions,
    last_pose: (bool, bool),
    previous_mode: sim::MovementMode,
}

/// A pose correction and the resize it caused when accepted by the live actor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct PoseWrite {
    pub correction: server_flags::FlagCorrection,
    pub resized: bool,
}

/// One accepted server edit retained in arrival order between completed ticks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum ServerWrite {
    Replacement(
        client_world::MovementFlagUpdate,
        client_world::ExplicitDimensions,
    ),
    Flags(client_world::MovementFlagUpdate),
    /// A live pose or resize whose flag bits replay copy-back can supersede.
    LivePose(PoseWrite),
    PoseCorrection(PoseWrite),
    Controls(client_world::MovementFlagUpdate),
    Dimensions(client_world::ExplicitDimensions),
}

/// Replays the control actions actually adopted between completed movement ticks.
pub(super) fn replay_actor_modes(writes: &[ServerWrite], modes: &mut ModeTracker) {
    for write in writes {
        match *write {
            ServerWrite::Controls(flags) => {
                modes.adopt_actor_flags(flags);
            }
            ServerWrite::Replacement(..)
            | ServerWrite::Flags(_)
            | ServerWrite::LivePose(_)
            | ServerWrite::PoseCorrection(_)
            | ServerWrite::Dimensions(_) => {}
        }
    }
}

impl SyncState {
    /// Replaces every lifetime-owned field before accepting the new actor's state.
    fn replace_actor(
        &mut self,
        flags: client_world::MovementFlagUpdate,
        dimensions: client_world::ExplicitDimensions,
    ) {
        *self = Self {
            flags,
            size: None,
            explicit_size: false,
            dimensions: Default::default(),
            last_pose: (false, false),
            previous_mode: sim::MovementMode::Walking,
        };
        self.resize();
        self.apply_dimensions(dimensions);
    }

    /// Updates flag and pose ownership without recording another replay edit.
    pub(super) fn apply_flags(&mut self, update: client_world::MovementFlagUpdate) {
        self.flags.merge(update);
        if self.explicit_size && update.sneaking.is_some() {
            // A matching local sneak does not request another resize.
            self.last_pose.0 = self.sneaking_pose();
        }
        if !self.explicit_size
            && [
                update.sneaking,
                update.gliding,
                update.swimming,
                update.crawling,
                update.spinning,
            ]
            .iter()
            .any(Option::is_some)
        {
            self.resize();
        }
    }

    /// Applies a size arrival and reports whether it changed component ownership or lifetime.
    pub(super) fn apply_dimensions(
        &mut self,
        dimensions: client_world::ExplicitDimensions,
    ) -> bool {
        let mut changed = false;
        if let Some(revision) = dimensions.spawn_revision
            && self.dimensions.spawn_revision != Some(revision)
        {
            if self
                .dimensions
                .spawn_revision
                .is_some_and(|current| revision < current)
            {
                return false;
            }
            if self.dimensions.spawn_revision.is_some() {
                self.size = None;
                self.explicit_size = false;
            }
            self.dimensions = client_world::ExplicitDimensions::for_spawn(revision);
            changed = true;
        }
        let mut size = self
            .size
            .unwrap_or([sim::PLAYER_WIDTH as f32, sim::PLAYER_HEIGHT as f32]);
        let mut resized = false;
        for ((previous, received), value) in
            [&mut self.dimensions.width, &mut self.dimensions.height]
                .into_iter()
                .zip([dimensions.width, dimensions.height])
                .zip(&mut size)
        {
            if let Some(write) = received.filter(|write| Some(*write) != *previous) {
                *previous = Some(write);
                *value = write.value;
                resized = true;
            }
        }
        if resized {
            self.size = Some(size);
            self.explicit_size = true;
        }
        changed || resized
    }

    /// Replays accepted flag and dimension writes in their original order.
    pub(super) fn replay_server<'a>(&mut self, writes: impl IntoIterator<Item = &'a ServerWrite>) {
        for write in writes {
            match *write {
                ServerWrite::Replacement(flags, dimensions) => {
                    self.replace_actor(flags, dimensions)
                }
                ServerWrite::Flags(flags) => self.apply_flags(flags),
                ServerWrite::LivePose(write) | ServerWrite::PoseCorrection(write) => {
                    self.flags.merge(write.correction.flags());
                    self.apply_pose_resize(write);
                }
                ServerWrite::Dimensions(dimensions) => {
                    self.apply_dimensions(dimensions);
                }
                ServerWrite::Controls(_) => {}
            }
        }
    }

    /// Keeps pending resize effects in arrival order under the replayed pose word.
    pub(super) fn replay_pending(&mut self, writes: &mut [ServerWrite]) {
        for write in writes {
            match write {
                ServerWrite::LivePose(pose) => {
                    // Copy-back replaced this pose, but its accepted resize still
                    // precedes any dimensions received after it.
                    pose.correction = Default::default();
                    self.apply_pose_resize(*pose);
                }
                ServerWrite::PoseCorrection(pose) => self.apply_pose_resize(*pose),
                _ => self.replay_server(std::iter::once(&*write)),
            }
        }
    }

    /// Preserves an accepted resize even when replay moved the flag correction earlier.
    fn apply_pose_resize(&mut self, write: PoseWrite) {
        if write.resized {
            self.explicit_size = false;
        }
        self.resize();
    }

    /// Records producer writes after a successful movement tick.
    pub(super) fn complete_tick(&mut self, frame: &ControllerFrame, state: &PlayerState) {
        self.flags.sneaking = Some(frame.input.sneaking);
        self.flags.sprinting = Some(frame.input.sprinting);
        self.flags.has_collision = Some(!frame.input.no_clip);
        if !frame.intent.vehicle_controlled_locally || frame.input.mode != sim::MovementMode::Riding
        {
            self.flags.moving = Some(moving_between(frame.position_before_tick, state.position));
        }
        self.previous_mode = frame.input.mode;
        self.accept_poses(frame.modes.pose_flags(), state.spin_attack_ticks.is_some());
    }

    /// Sizes the accepted pose word and reports whether it changed the sizing pose.
    pub(super) fn accept_poses(&mut self, poses: super::super::PoseFlags, spinning: bool) -> bool {
        self.flags.gliding = Some(poses.gliding);
        self.flags.swimming = Some(poses.swimming);
        self.flags.crawling = Some(poses.crawling);
        self.flags.spinning = Some(spinning);
        self.resize()
    }

    /// Whether the accepted sneak bit requests a crouching walking size.
    fn sneaking_pose(&self) -> bool {
        self.previous_mode == sim::MovementMode::Walking && self.flags.sneaking.unwrap_or(false)
    }

    /// A pose change replaces an explicit request; unchanged poses retain its dimensions.
    fn resize(&mut self) -> bool {
        let horizontal = self.flags.spinning == Some(true)
            || [self.flags.gliding, self.flags.swimming, self.flags.crawling].contains(&Some(true));
        let pose = (self.sneaking_pose(), horizontal);
        let changed = pose != self.last_pose;
        if changed {
            self.explicit_size = false;
        }
        self.last_pose = pose;
        if !self.explicit_size {
            self.size = Some([
                sim::PLAYER_WIDTH as f32,
                if horizontal {
                    sim::PLAYER_WIDTH as f32
                } else {
                    sim::MovementMode::Walking.hitbox_height(pose.0) as f32
                },
            ]);
        }
        changed
    }
}

impl LocalPhysicsController {
    /// Returns accepted pose flags independently of flight and mount travel.
    pub(in crate::movement) fn pose_flags(&self) -> super::super::PoseFlags {
        self.modes.pose_flags()
    }

    /// Starts accepted sync and control state for a replacement actor without rewinding old inputs.
    pub(crate) fn replace_actor_authority(
        &mut self,
        flags: client_world::MovementFlagUpdate,
        dimensions: client_world::ExplicitDimensions,
    ) {
        self.sync_state.replace_actor(flags, dimensions);
        self.sync_pending
            .push(ServerWrite::Replacement(flags, dimensions));
        self.live_posture = Default::default();
        self.server_control_flags = None;
        self.server_flags.reset_history();
        if let Some(state) = self.state.as_mut() {
            state.spin_attack_ticks = flags.spinning.unwrap_or(false).then_some(0);
            state.swim_pose_active = [flags.swimming, flags.crawling].contains(&Some(true));
        }
        self.modes.adopt_actor_flags(flags);
        self.adopt_live_flags(flags);
    }

    /// Records one sparse server flag write without replacing prior replay actions.
    pub(super) fn note_server_flags(&mut self, flags: client_world::MovementFlagUpdate) {
        if flags != client_world::MovementFlagUpdate::default() {
            self.sync_state.apply_flags(flags);
            self.sync_pending.push(ServerWrite::Flags(flags));
        }
    }

    /// Returns effective flight independently of the current travel mode.
    pub(in crate::movement) fn prediction_sync_flying(&self) -> bool {
        self.modes.flying()
    }

    /// Overlays server posture accepted after the latest completed movement sample.
    #[cfg(test)]
    pub(in crate::movement) fn prediction_sync_posture(&self) -> Option<(bool, bool)> {
        let (sneaking, sprinting) = self.latest_sneak_sprint()?;
        Some((
            self.live_posture.sneaking.unwrap_or(sneaking),
            self.live_posture.sprinting.unwrap_or(sprinting),
        ))
    }

    /// Returns the current accepted writes, including newer server changes.
    pub(in crate::movement) fn prediction_sync_flags(&self) -> client_world::MovementFlagUpdate {
        let mut flags = self.sync_state.flags;
        if let Some(state) = self.state.as_ref() {
            let poses = self.pose_flags();
            flags.gliding = Some(poses.gliding);
            flags.swimming = Some(poses.swimming);
            flags.crawling = Some(poses.crawling);
            flags.spinning = Some(state.spin_attack_ticks.is_some());
        }
        flags
    }

    /// Records a server dimension write before any later local resize is evaluated.
    pub(crate) fn note_server_dimensions(&mut self, dimensions: client_world::ExplicitDimensions) {
        if self.sync_state.apply_dimensions(dimensions) {
            self.sync_pending.push(ServerWrite::Dimensions(dimensions));
        }
    }

    /// Whether a newer explicit dimension request still owns the reported size.
    pub(in crate::movement) fn prediction_sync_explicit_size(&self) -> bool {
        self.sync_state.explicit_size
    }

    /// Returns the unscaled size after accepted server and completed local writes.
    pub(in crate::movement) fn prediction_sync_size(&self) -> Option<[f32; 2]> {
        self.sync_state.size
    }

    /// Uses the current accepted local size for walking posture observations.
    #[cfg(test)]
    pub(in crate::movement) fn prediction_sync_walking_height(&self) -> Option<f32> {
        self.sync_state.size.map(|size| size[1])
    }
}

/// Tests completed position displacement with the actor flag's f32 threshold.
fn moving_between(before: Vec3, after: Vec3) -> bool {
    let dx = after.x as f32 - before.x as f32;
    let dy = after.y as f32 - before.y as f32;
    let dz = after.z as f32 - before.z as f32;
    dz * dz + (dy * dy + dx * dx) > MOVING_FLAG_DISTANCE_SQUARED
}

#[cfg(test)]
mod tests {
    use super::*;

    struct OpenWorld;

    impl CollisionWorld for OpenWorld {
        /// Supplies empty terrain for the local pose and correction ticks.
        fn collision_boxes(
            &self,
            _: sim::Aabb,
        ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
            Ok(sim::CollisionQuery::synthetic(Vec::new()))
        }
    }

    /// Starts and immediately stops a glide, applying its stamped start live or by replay.
    fn stopped_glide(replay: bool) -> LocalPhysicsController {
        let mut physics = LocalPhysicsController::default();
        physics.reanchor_network_position([0.5, 72.0, 0.5], 100, false);
        let mut dimensions = client_world::ExplicitDimensions::for_spawn(1);
        dimensions.record(
            &protocol::ActorMetadata {
                key: protocol_ids::ActorDataKey::Height.wire_value(),
                value: protocol::ActorMetadataValue::Float(0.7),
            },
            1,
        );
        physics.note_server_dimensions(dimensions);
        let anchor = physics
            .advance(
                simulation_clock::TICK_DURATION,
                MovementInput::default(),
                &OpenWorld,
            )
            .samples
            .remove(0);
        assert!(physics.sync_state.explicit_size);
        let start = client_world::MovementFlagUpdate {
            gliding: Some(true),
            ..Default::default()
        };
        if !replay {
            assert_eq!(
                physics.apply_server_movement_flags(anchor.tick, start),
                None
            );
        }
        let sample = physics
            .advance(
                simulation_clock::TICK_DURATION,
                MovementInput::default(),
                &OpenWorld,
            )
            .samples
            .remove(0);
        assert_eq!(sample.processed.mode, sim::MovementMode::Walking);
        if replay {
            assert_eq!(
                physics.apply_server_movement_flags(anchor.tick, start),
                Some(anchor.tick)
            );
            let plan = physics
                .apply_correction(
                    crate::movement::PhysicsAnchor {
                        network_position: anchor.position,
                        tick: anchor.tick,
                        on_ground: false,
                        velocity: Some(anchor.velocity),
                    },
                    crate::movement::PhysicsCorrectionMode::ReplayIfRetained,
                    None,
                    &OpenWorld,
                )
                .unwrap();
            assert_eq!(plan.replayed_samples.len(), 1);
            assert_eq!(
                plan.replayed_samples[0].processed.mode,
                sim::MovementMode::Walking
            );
        }
        physics
    }

    #[test]
    fn replayed_glide_start_and_immediate_stop_report_standing_height() {
        let physics = stopped_glide(true);
        assert_eq!(
            physics.prediction_sync_size(),
            Some([sim::PLAYER_WIDTH as f32, sim::PLAYER_HEIGHT as f32])
        );
        assert!(!physics.prediction_sync_explicit_size());
    }

    #[test]
    fn replayed_glide_start_and_immediate_stop_match_live_sync_state() {
        assert_eq!(
            stopped_glide(true).sync_state,
            stopped_glide(false).sync_state
        );
    }

    #[test]
    fn future_pose_resize_keeps_the_intervening_local_stop() {
        let mut physics = LocalPhysicsController::default();
        physics.reanchor_network_position([0.5, 72.0, 0.5], 100, false);
        let anchor = physics
            .advance(
                simulation_clock::TICK_DURATION,
                MovementInput::default(),
                &OpenWorld,
            )
            .samples
            .remove(0);
        physics.apply_server_movement_flags(
            anchor.tick,
            client_world::MovementFlagUpdate {
                gliding: Some(true),
                ..Default::default()
            },
        );
        let mut dimensions = client_world::ExplicitDimensions::for_spawn(1);
        dimensions.record(
            &protocol::ActorMetadata {
                key: protocol_ids::ActorDataKey::Height.wire_value(),
                value: protocol::ActorMetadataValue::Float(0.7),
            },
            1,
        );
        physics.note_server_dimensions(dimensions);
        physics.advance(
            simulation_clock::TICK_DURATION,
            MovementInput::default(),
            &OpenWorld,
        );
        assert_eq!(
            physics.prediction_sync_size().unwrap()[1],
            sim::PLAYER_HEIGHT as f32
        );
        physics.apply_server_movement_flags(
            physics.state().unwrap().tick + 1,
            client_world::MovementFlagUpdate {
                spinning: Some(true),
                ..Default::default()
            },
        );
        let live = physics.sync_state;
        assert_eq!(live.size, Some([sim::PLAYER_WIDTH as f32; 2]));
        physics
            .apply_correction(
                crate::movement::PhysicsAnchor {
                    network_position: anchor.position,
                    tick: anchor.tick,
                    on_ground: false,
                    velocity: Some(anchor.velocity),
                },
                crate::movement::PhysicsCorrectionMode::ReplayIfRetained,
                None,
                &OpenWorld,
            )
            .unwrap();
        assert_eq!(physics.sync_state, live);
    }

    #[test]
    fn pending_dimensions_keep_their_order_with_retained_and_future_resizes() {
        for retained in [false, true] {
            for dimensions_first in [false, true] {
                let mut physics = stopped_glide(false);
                let anchor = physics.sample_at(101).unwrap().clone();
                let mut dimensions = client_world::ExplicitDimensions::for_spawn(1);
                dimensions.record(
                    &protocol::ActorMetadata {
                        key: protocol_ids::ActorDataKey::Height.wire_value(),
                        value: protocol::ActorMetadataValue::Float(0.7),
                    },
                    2,
                );
                if dimensions_first {
                    physics.note_server_dimensions(dimensions);
                }
                physics.apply_server_movement_flags(
                    if retained {
                        anchor.tick
                    } else {
                        physics.state().unwrap().tick + 1
                    },
                    client_world::MovementFlagUpdate {
                        spinning: Some(true),
                        ..Default::default()
                    },
                );
                if !dimensions_first {
                    physics.note_server_dimensions(dimensions);
                }
                let live = physics.sync_state;
                assert_eq!(live.explicit_size, !dimensions_first);
                for _ in 0..2 {
                    physics
                        .apply_correction(
                            crate::movement::PhysicsAnchor {
                                network_position: anchor.position,
                                tick: anchor.tick,
                                on_ground: false,
                                velocity: Some(anchor.velocity),
                            },
                            crate::movement::PhysicsCorrectionMode::ReplayIfRetained,
                            None,
                            &OpenWorld,
                        )
                        .unwrap();
                    assert_eq!(
                        physics.sync_state, live,
                        "retained {retained}, dimensions first {dimensions_first}"
                    );
                }
            }
        }
    }

    #[test]
    fn pending_pose_sizes_follow_replay_copy_back_and_newer_dimensions() {
        for stamped in [false, true] {
            for spinning in [false, true] {
                let mut physics = stopped_glide(false);
                let anchor = physics.sample_at(101).unwrap().clone();
                physics.apply_server_movement_flags(
                    if stamped {
                        physics.state().unwrap().tick
                    } else {
                        0
                    },
                    client_world::MovementFlagUpdate {
                        gliding: (!spinning).then_some(true),
                        spinning: spinning.then_some(true),
                        ..Default::default()
                    },
                );
                let mut dimensions = client_world::ExplicitDimensions::for_spawn(1);
                dimensions.record(
                    &protocol::ActorMetadata {
                        key: protocol_ids::ActorDataKey::Width.wire_value(),
                        value: protocol::ActorMetadataValue::Float(0.8),
                    },
                    2,
                );
                physics.note_server_dimensions(dimensions);
                physics
                    .apply_correction(
                        crate::movement::PhysicsAnchor {
                            network_position: anchor.position,
                            tick: anchor.tick,
                            on_ground: false,
                            velocity: Some(anchor.velocity),
                        },
                        crate::movement::PhysicsCorrectionMode::ReplayIfRetained,
                        None,
                        &OpenWorld,
                    )
                    .unwrap();
                assert_eq!(physics.prediction_sync_flags().gliding, Some(false));
                assert_eq!(physics.prediction_sync_flags().spinning, Some(false));
                assert_eq!(
                    physics.prediction_sync_size(),
                    Some([0.8, sim::PLAYER_HEIGHT as f32])
                );
                assert!(physics.prediction_sync_explicit_size());
                if !stamped {
                    let anchor = physics.sample_at(102).unwrap().clone();
                    physics.advance(
                        simulation_clock::TICK_DURATION,
                        MovementInput::default(),
                        &OpenWorld,
                    );
                    let live = physics.sync_state;
                    physics
                        .apply_correction(
                            crate::movement::PhysicsAnchor {
                                network_position: anchor.position,
                                tick: anchor.tick,
                                on_ground: false,
                                velocity: Some(anchor.velocity),
                            },
                            crate::movement::PhysicsCorrectionMode::ReplayIfRetained,
                            None,
                            &OpenWorld,
                        )
                        .unwrap();
                    assert_eq!(physics.sync_state, live);
                }
            }
        }
    }

    #[test]
    fn pose_correction_and_dimension_order_survives_replay() {
        for dimensions_first in [false, true] {
            let mut physics = LocalPhysicsController::default();
            physics.reanchor_network_position([0.5, 72.0, 0.5], 100, false);
            let anchor = physics
                .advance(
                    simulation_clock::TICK_DURATION,
                    MovementInput::default(),
                    &OpenWorld,
                )
                .samples
                .remove(0);
            let mut dimensions = client_world::ExplicitDimensions::for_spawn(1);
            dimensions.record(
                &protocol::ActorMetadata {
                    key: protocol_ids::ActorDataKey::Width.wire_value(),
                    value: protocol::ActorMetadataValue::Float(0.8),
                },
                1,
            );
            if dimensions_first {
                physics.note_server_dimensions(dimensions);
            }
            physics.apply_server_movement_flags(
                0,
                client_world::MovementFlagUpdate {
                    gliding: Some(true),
                    ..Default::default()
                },
            );
            if !dimensions_first {
                physics.note_server_dimensions(dimensions);
            }
            physics.advance_with_context(
                simulation_clock::TICK_DURATION,
                MovementInput::default(),
                PhysicsSampleContext {
                    mode_intent: ModeIntent {
                        elytra_ready: true,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &OpenWorld,
            );
            let live = physics.sync_state;
            assert_eq!(live.explicit_size, !dimensions_first);
            assert_eq!(
                live.size,
                Some([
                    if dimensions_first {
                        sim::PLAYER_WIDTH as f32
                    } else {
                        0.8
                    },
                    sim::PLAYER_WIDTH as f32,
                ])
            );
            physics
                .apply_correction(
                    crate::movement::PhysicsAnchor {
                        network_position: anchor.position,
                        tick: anchor.tick,
                        on_ground: false,
                        velocity: Some(anchor.velocity),
                    },
                    crate::movement::PhysicsCorrectionMode::ReplayIfRetained,
                    None,
                    &OpenWorld,
                )
                .unwrap();
            assert_eq!(
                physics.sync_state, live,
                "dimensions first: {dimensions_first}"
            );
        }
    }

    #[test]
    fn replacement_resets_every_sync_field_and_replays_as_one_write() {
        let flags = client_world::MovementFlagUpdate::default();
        let dimensions = client_world::ExplicitDimensions::for_spawn(2);
        let mut dirty = SyncState {
            flags: client_world::MovementFlagUpdate {
                moving: Some(true),
                spinning: Some(true),
                ..Default::default()
            },
            size: Some([0.9, 0.7]),
            explicit_size: true,
            dimensions: client_world::ExplicitDimensions::for_spawn(1),
            last_pose: (true, true),
            previous_mode: sim::MovementMode::Gliding,
        };
        let mut fresh = SyncState::default();
        fresh.replace_actor(flags, dimensions);
        dirty.replay_server(&[ServerWrite::Replacement(flags, dimensions)]);
        assert_eq!(dirty, fresh);
        assert_eq!(
            dirty.size,
            Some([sim::PLAYER_WIDTH as f32, sim::PLAYER_HEIGHT as f32])
        );
    }

    #[test]
    fn moving_threshold_keeps_the_small_vertical_and_horizontal_contributions() {
        assert!(moving_between(
            Vec3::ZERO,
            Vec3::new(0.005, 0.025, 0.01870828866958618),
        ));
    }
}
