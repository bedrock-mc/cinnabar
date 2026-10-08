//! Correction rewind and replay for the local physics controller.

use super::*;

impl LocalPhysicsController {
    pub(in crate::movement) fn apply_correction(
        &mut self,
        anchor: crate::movement::PhysicsAnchor,
        mode: PhysicsCorrectionMode,
        confirmation: Option<&PhysicsCorrectionConfirmation>,
        world: &impl CollisionWorld,
    ) -> Result<PhysicsCorrectionPlan, PhysicsCorrectionError> {
        let crate::movement::PhysicsAnchor {
            network_position,
            tick,
            on_ground,
            velocity,
        } = anchor;
        if !network_position.into_iter().all(f32::is_finite) {
            return Err(PhysicsCorrectionError::InvalidAnchor);
        }
        let velocity = velocity
            .filter(|velocity| super::timeline::motion_is_simulable(*velocity))
            .map(|velocity| {
                Vec3::new(
                    f64::from(velocity[0]),
                    f64::from(velocity[1]),
                    f64::from(velocity[2]),
                )
            });
        self.prediction_sync.arm();
        if matches!(mode, PhysicsCorrectionMode::Snap) {
            let jump_delay = self.state.as_ref().map_or(0, |state| state.jump_delay);
            let swim_amount = self.state.as_ref().map_or(0.0, |state| state.swim_amount);
            let swim_pose_active = self
                .state
                .as_ref()
                .is_some_and(|state| state.swim_pose_active);
            let previous_jump_held = self.previous_jump_held;
            let jump_edge_pending = self.jump_edge_pending;
            let input_edges = self.input_edges;
            let modes = self.modes;
            self.reanchor_network_position_before_advance(network_position, tick, on_ground);
            // As in vanilla, MovePlayer changes spatial state without resetting
            // jump input or movement abilities.
            self.previous_jump_held = previous_jump_held;
            self.jump_edge_pending = jump_edge_pending;
            self.input_edges = input_edges;
            self.modes = modes;
            if let Some(state) = self.state.as_mut() {
                state.jump_delay = jump_delay;
                state.swim_amount = swim_amount;
                state.swim_pose_active = swim_pose_active;
            }
            if let (Some(velocity), Some(state)) = (velocity, self.state.as_mut()) {
                state.velocity = velocity;
            }
            return Ok(PhysicsCorrectionPlan {
                outcome: PhysicsCorrectionOutcome::Snapped { tick },
                corrected_tick: tick,
                final_tick: tick,
                final_position: network_position,
                anchor_input: super::super::encoding::HeldInput::default(),
                corrected_sample: None,
                replayed_samples: Vec::new(),
            });
        }

        if self.state.is_none() {
            return Err(PhysicsCorrectionError::NotRetained { tick });
        }

        let current_tick = self
            .state
            .as_ref()
            .expect("active correction checked for local state")
            .tick;
        if tick > current_tick {
            return Err(PhysicsCorrectionError::NotRetained { tick });
        }
        let Some(mut corrected) = self.history.state_at(tick).cloned() else {
            return Err(PhysicsCorrectionError::NotRetained { tick });
        };
        if !self.sample_history.iter().any(|sample| sample.tick == tick) {
            return Err(PhysicsCorrectionError::NotRetained { tick });
        }

        let feet = Vec3::new(
            f64::from(network_position[0]),
            f64::from(network_position[1] - PLAYER_NETWORK_OFFSET),
            f64::from(network_position[2]),
        );
        // Vanilla's correction input writes both position and velocity
        // into the corrected frame before replaying later inputs.
        corrected.position = feet;
        corrected.on_ground = on_ground;
        // Axis collisions describe the motion that produced a position, so they
        // cannot be recomputed from a corrected anchor. They are retained only
        // when a bounded transport-success record shows that the correction
        // exactly matches the network position this client sent for that tick,
        // the retained sample used that same immutable collision identity, and
        // every chunk in that identity is still loaded at the same revision.
        // Cinnabar provisionally interprets that combination as confirmation of
        // the motion behind the position; this is a client replay policy, not
        // an established vanilla or protocol guarantee. Retaining the flags
        // avoids stuttering a legitimate wall climb on matching corrections.
        // Any missing proof or mismatch clears the flags and keeps the discrete
        // climb branch closed. An upward velocity produced while a stale
        // horizontal collision was retained is the same unconfirmed ladder
        // response, so it is cleared with those flags. Identity query failure
        // is semantic unavailability and does not disconnect. The position
        // comparison is exact in the sent `f32` network space because that is
        // the serialized position available to compare. The loss is bounded
        // to the corrected tick: `Simulator::tick` re-derives collisions.
        let retained_sample = self
            .sample_history
            .iter()
            .find(|sample| sample.tick == tick)
            .expect("retained correction sample was checked");
        let server_confirmed_prediction = confirmation.is_some_and(|confirmation| {
            confirmation.position == network_position
                && retained_sample.position == network_position
                && confirmation.world_identity == retained_sample.world_identity
                && self
                    .history
                    .world_at(tick)
                    .map_or_else(
                        || collision_identity_is_current(world, feet, &confirmation.world_identity),
                        |snapshot| {
                            collision_identity_is_current(
                                snapshot,
                                feet,
                                &confirmation.world_identity,
                            )
                        },
                    )
                    .unwrap_or(false)
        });
        if !server_confirmed_prediction {
            let was_climbing = self.controller_history.iter().any(|frame| {
                frame.tick == tick
                    && frame.environment.on_climbable
                    && frame.input.mode == sim::MovementMode::Walking
            });
            if was_climbing
                && (corrected.collisions.x || corrected.collisions.z)
                && corrected.velocity.y > 0.0
            {
                corrected.velocity.y = 0.0;
            }
            corrected.collisions = sim::AxisCollisions::default();
        }
        if let Some(velocity) = velocity {
            corrected.velocity = velocity;
        }
        self.deferred_corrections.supersede(tick);
        self.replay_from_corrected(tick, corrected, Some(network_position), world)
    }

    /// Re-simulates every retained tick after `tick` from its unchanged state so
    /// timeline edits recorded after it (motion, attributes, flags) take effect.
    pub(in crate::movement) fn replay_retained_from(
        &mut self,
        tick: u64,
        world: &impl CollisionWorld,
    ) -> Result<PhysicsCorrectionPlan, PhysicsCorrectionError> {
        let current_tick = self
            .state
            .as_ref()
            .ok_or(PhysicsCorrectionError::NotRetained { tick })?
            .tick;
        if tick > current_tick || !self.sample_history.iter().any(|sample| sample.tick == tick) {
            return Err(PhysicsCorrectionError::NotRetained { tick });
        }
        let Some(corrected) = self.history.state_at(tick).cloned() else {
            return Err(PhysicsCorrectionError::NotRetained { tick });
        };
        self.replay_from_corrected(tick, corrected, None, world)
    }

    fn replay_from_corrected(
        &mut self,
        tick: u64,
        corrected: PlayerState,
        corrected_network_position: Option<[f32; 3]>,
        world: &impl CollisionWorld,
    ) -> Result<PhysicsCorrectionPlan, PhysicsCorrectionError> {
        let prior_position = self.state.as_ref().map(|state| state.position);
        let prior_previous_position = self.previous_position;
        let on_ground = corrected.on_ground;
        let feet = corrected.position;
        let corrected_velocity = [
            corrected.velocity.x as f32,
            corrected.velocity.y as f32,
            corrected.velocity.z as f32,
        ];
        let corrected_collisions = corrected.collisions;
        let motion_overlays: Vec<sim::MotionOverlay> =
            self.server_motions.iter().copied().collect();
        let mut controller_frames = self.controller_history.clone();
        let immobility_edits: Vec<u64> = controller_frames
            .iter()
            .filter(|frame| {
                self.history
                    .input_at(frame.tick)
                    .is_some_and(|input| input.immobile != frame.input.immobile)
            })
            .map(|frame| frame.tick)
            .collect();
        let anchor_controller = controller_frames
            .iter()
            .find(|frame| frame.tick == tick)
            .copied()
            .ok_or(PhysicsCorrectionError::NotRetained { tick })?;
        let mut modes = anchor_controller.modes;
        let deferred_corrections = self.deferred_corrections.clone();
        let (replay, replayed_ticks) = self
            .history
            .rewind_and_replay_prepared(
                self.state
                    .as_mut()
                    .expect("active correction checked for local state"),
                corrected,
                &self.simulator,
                world,
                &motion_overlays,
                |state, input, world, previous| {
                    deferred_corrections.apply_before(state);
                    let frame = controller_frames
                        .iter_mut()
                        .find(|frame| frame.tick == state.tick + 1)
                        .expect("controller and prediction histories retain the same ticks");
                    let environment = previous.map_or(anchor_controller.environment, |output| {
                        output.tick_result.environment
                    });
                    frame.prepare(&mut modes, environment, state, input, world)
                },
            )
            .map_err(|_| PhysicsCorrectionError::ReplayFailed)?;

        self.deferred_corrections.mark_replayed();

        if replayed_ticks.len() != replay.replayed_ticks {
            return Err(PhysicsCorrectionError::ReplayFailed);
        }
        // Replay supplies actual jump initiations; grounded correction anchors close the old arc.
        let mut jump_fold = {
            let corrected_sample = self
                .sample_history
                .iter()
                .find(|sample| sample.tick == tick)
                .expect("retained correction sample was checked");
            ReplayJumpArcFold::seed(
                on_ground,
                corrected_sample.processed.jump_initiated,
                corrected_sample.processed.jump_arc_active,
            )
        };
        let mut replayed_samples = Vec::with_capacity(replayed_ticks.len());
        let anchor_input = super::super::encoding::HeldInput::from(
            self.sample_history
                .iter()
                .find(|sample| sample.tick == tick)
                .expect("retained correction sample was checked"),
        );
        for output in replayed_ticks {
            if let Some(frame) = controller_frames
                .iter_mut()
                .find(|frame| frame.tick == output.tick_result.tick)
            {
                frame.environment = output.tick_result.environment;
            }
            let result = output.tick_result;
            let Some(retained) = self
                .sample_history
                .iter_mut()
                .find(|sample| sample.tick == result.tick)
            else {
                return Err(PhysicsCorrectionError::NotRetained { tick: result.tick });
            };
            if self.history.world_at(result.tick).is_none()
                && retained.world_identity != result.world_identity
                && (immobility_edits.binary_search(&result.tick).is_err()
                    || retained.world_identity.registry != result.world_identity.registry)
            {
                return Err(PhysicsCorrectionError::WorldIdentityMismatch { tick: result.tick });
            }
            retained.world_identity = result.world_identity.clone();
            retained.position = [
                result.position.x as f32,
                result.position.y as f32 + PLAYER_NETWORK_OFFSET,
                result.position.z as f32,
            ];
            retained.movement = [
                result.movement.x as f32,
                result.movement.y as f32,
                result.movement.z as f32,
            ];
            retained.velocity = [
                result.velocity.x as f32,
                result.velocity.y as f32,
                result.velocity.z as f32,
            ];
            retained.move_vector = [
                -output.controls.move_vector[0] as f32,
                output.controls.move_vector[1] as f32,
            ];
            retained.horizontal_collision = result.collisions.x || result.collisions.z;
            retained.vertical_collision = result.collisions.y;
            retained.grounded_after_tick = result.on_ground;
            // The replay fed this input verbatim, mirroring the simulator's
            // own per-tick consumption.
            let Some(frame_input) = self.history.input_at(result.tick) else {
                return Err(PhysicsCorrectionError::NotRetained { tick: result.tick });
            };
            if frame_input.immobile || frame_input.mode == sim::MovementMode::Riding {
                jump_fold = ReplayJumpArcFold::seed(true, false, false);
            }
            let (initiated, arc_active) = jump_fold.step(output.jump_initiated, result.on_ground);
            retained.sneaking = frame_input.sneaking;
            retained.sprinting = frame_input.sprinting;
            retained.processed.sneaking = frame_input.sneaking;
            retained.processed.sprinting = frame_input.sprinting;
            retained.processed.mode = frame_input.mode;
            if let Some(frame) = controller_frames
                .iter()
                .find(|frame| frame.tick == result.tick)
            {
                retained.grounded_before_tick = frame.grounded_before_tick;
                retained.jump_repeated = frame.jump_repeated;
                retained.processed.forced_sneak = frame.forced_sneak;
                retained.processed.ride = frame.intent.ride;
                if let Some(delta) = frame.ride_delta {
                    retained.movement = delta;
                }
            }
            retained.processed.jump_initiated = initiated;
            retained.processed.jump_arc_active = arc_active;
            replayed_samples.push(retained.clone());
        }
        self.processed_jump_arc_active = jump_fold.arc_active();
        self.modes = modes;
        self.last_environment = controller_frames
            .back()
            .map_or(anchor_controller.environment, |frame| frame.environment);
        self.controller_history = controller_frames;
        let (corrected_world_identity, corrected_sample) = {
            let corrected_sample = self
                .sample_history
                .iter_mut()
                .find(|sample| sample.tick == tick)
                .expect("retained correction sample was checked");
            if let Some(position) = corrected_network_position {
                corrected_sample.position = position;
                corrected_sample.velocity = corrected_velocity;
                corrected_sample.grounded_after_tick = on_ground;
                corrected_sample.horizontal_collision =
                    corrected_collisions.x || corrected_collisions.z;
                corrected_sample.vertical_collision = corrected_collisions.y;
            }
            (
                corrected_sample.world_identity.clone(),
                corrected_sample.clone(),
            )
        };

        self.refresh_motion_ticks();
        let state = self
            .state
            .as_ref()
            .expect("successful replay retains local state");
        let final_tick = state.tick;
        let final_position = [
            state.position.x as f32,
            state.position.y as f32 + PLAYER_NETWORK_OFFSET,
            state.position.z as f32,
        ];
        self.previous_position = if final_tick == tick {
            feet
        } else {
            self.history
                .state_at(final_tick.saturating_sub(1))
                .map_or(feet, |previous| previous.position)
        };
        if let Some(prior_position) = prior_position {
            self.visual_correction.correct(
                prior_position - state.position,
                prior_previous_position - self.previous_position,
                state.velocity,
            );
        }
        self.last_world_identity = replayed_samples
            .last()
            .map(|sample| sample.world_identity.clone())
            .or(Some(corrected_world_identity));
        // A replay re-anchors the corrected tick and rebuilds later ticks from
        // it; that landing can sit inside solids just like a hard anchor. Re-arm
        // the depenetration probe so the next tick pushes the anchor out
        // positionally instead of streaming an embedded pose indefinitely.
        self.anchor_state.rearm();

        Ok(PhysicsCorrectionPlan {
            outcome: PhysicsCorrectionOutcome::Replayed {
                corrected_tick: replay.corrected_tick,
                replayed_ticks: replay.replayed_ticks,
            },
            corrected_tick: tick,
            final_tick,
            final_position,
            anchor_input,
            corrected_sample: Some(corrected_sample),
            replayed_samples,
        })
    }
}

fn collision_identity_is_current(
    world: &impl CollisionWorld,
    corrected_feet: Vec3,
    expected: &WorldCollisionIdentity,
) -> Result<bool, sim::WorldQueryError> {
    let y = checked_block_coordinate(corrected_feet.y)?;
    let mut current: Option<WorldCollisionIdentity> = None;
    if expected.chunks.is_empty() {
        let block = [
            checked_block_coordinate(corrected_feet.x)?,
            y,
            checked_block_coordinate(corrected_feet.z)?,
        ];
        current = Some(world.block_physics(block)?.identity);
    } else {
        for revision in &expected.chunks {
            let Some(x) = revision.chunk.x.checked_mul(16) else {
                return Err(sim::WorldQueryError::CoordinateOutOfRange);
            };
            let Some(z) = revision.chunk.z.checked_mul(16) else {
                return Err(sim::WorldQueryError::CoordinateOutOfRange);
            };
            let identity = world.block_physics([x, y, z])?.identity;
            current = Some(match current {
                None => identity,
                Some(previous) => previous.merge(&identity)?,
            });
        }
    }
    Ok(current.as_ref() == Some(expected))
}

fn checked_block_coordinate(value: f64) -> Result<i32, sim::WorldQueryError> {
    let value = value.floor();
    if value < f64::from(i32::MIN) || value > f64::from(i32::MAX) {
        return Err(sim::WorldQueryError::CoordinateOutOfRange);
    }
    Ok(value as i32)
}
