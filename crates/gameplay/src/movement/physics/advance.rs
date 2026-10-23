use super::*;

impl LocalPhysicsController {
    pub fn advance(
        &mut self,
        elapsed: Duration,
        input: MovementInput,
        world: &impl CollisionWorld,
    ) -> LocalPhysicsFrame {
        self.advance_with_context(elapsed, input, PhysicsSampleContext::default(), world)
    }

    pub fn advance_with_context(
        &mut self,
        elapsed: Duration,
        input: MovementInput,
        context: PhysicsSampleContext,
        world: &impl CollisionWorld,
    ) -> LocalPhysicsFrame {
        self.advance_with_context_and_effects(
            elapsed,
            input,
            context,
            world,
            &mut NoMovementEffects,
        )
    }

    pub fn advance_with_context_and_effects(
        &mut self,
        elapsed: Duration,
        input: MovementInput,
        context: PhysicsSampleContext,
        world: &impl CollisionWorld,
        effects: &mut impl MovementEffectSource,
    ) -> LocalPhysicsFrame {
        self.advance_with_speed_updates(elapsed, input, context, &[], world, effects)
    }

    /// Records accepted speed operations on the first successful tick of this frame.
    pub(in crate::movement) fn advance_with_speed_updates(
        &mut self,
        elapsed: Duration,
        mut input: MovementInput,
        mut context: PhysicsSampleContext,
        updates: &[super::super::speed_authority::MovementSpeedUpdate],
        world: &impl CollisionWorld,
        effects: &mut impl MovementEffectSource,
    ) -> LocalPhysicsFrame {
        self.filter_input_requests(&mut input, &mut context);
        self.dimension_waiting = false;
        let Some(state) = self.state.as_mut() else {
            return LocalPhysicsFrame::default();
        };
        self.input_edges.observe(context.input);
        if context.input.jump.pressed || (input.jumping && !self.previous_jump_held) {
            self.jump_edge_pending = true;
        }
        self.previous_jump_held = input.jumping;
        input.jump_pressed = self.jump_edge_pending;

        let mut frame = fixed_ticks::frame(
            elapsed,
            &mut self.accumulated_seconds,
            &mut self.discard_next_elapsed,
        );
        let allowed = frame
            .due_ticks
            .min(MAX_LOCAL_PHYSICS_TICKS_PER_FRAME as u64) as usize;

        let physical_jump_held = input.jumping;
        let sprint_request = input.sprinting;
        let requested_movement_speed = input.movement_speed;
        let mut movement_speed = self
            .controller_history
            .back()
            .map_or(Default::default(), |frame| frame.speed);
        let mut speed_updates: Vec<_> = context
            .movement_speed_attribute
            .map(super::super::speed_authority::MovementSpeedUpdate::Attribute)
            .into_iter()
            .chain(updates.iter().copied())
            .collect();
        for update in &speed_updates {
            update.apply(&mut movement_speed);
        }
        movement_speed.adopt_server_sprinting(Some(self.modes.sprinting()));
        let sneak_request = input.sneaking;
        input.pitch_degrees = f64::from(context.pitch);
        input.fly_speed = context.mode_intent.fly_speed;
        input.vertical_fly_speed = context.mode_intent.vertical_fly_speed;
        input.creative_flight = context.mode_intent.creative_flight;
        input.spectator = context.mode_intent.spectator;
        input.no_clip = context.mode_intent.no_clip;
        input.depth_strider = context.mode_intent.depth_strider;
        input.soul_speed = context.mode_intent.soul_speed;
        input.swift_sneak = context.mode_intent.swift_sneak;
        let item_use_modifier = input.item_use_movement_modifier;
        for tick_index in 0..allowed {
            let context = context.for_next_tick(state.tick);
            input.item_use_movement_modifier = context
                .release_use_modifier
                .map(|(_, modifier)| item_use_factor(modifier))
                .or(item_use_modifier);
            input.riptide = self
                .riptide_pending
                .filter(|request| state.tick >= request.after_tick)
                .map(|request| request.launch);
            input.skip_travel = self.skip_travel_pending;
            let mut tick_input = self.input_edges.sample(context.input);
            // Bedrock auto-jump semantics treat a held jump as a fresh request
            // once the player is grounded again. Preserve the render-frame edge
            // latch for taps shorter than one fixed tick, but never inject the
            // repeated edge while airborne or during the jump-delay window.
            input.jumping = physical_jump_held || self.jump_pulse_pending;
            let grounded_before_tick = state.on_ground;
            let jump_repeated = !input.immobile
                && input.jumping
                && grounded_before_tick
                && state.jump_delay == 0
                && !self.jump_edge_pending;
            input.jump_pressed = self.jump_edge_pending || self.jump_pulse_pending || jump_repeated;
            input.effects = effects.snapshot();
            self.boosts.apply(state.tick + 1, &mut input.effects);
            input.sprinting = sprint_request;
            input.movement_speed = requested_movement_speed;
            input.liquid_contact_height = Some(
                self.modes
                    .liquid_contact_height(state.spin_attack_ticks.is_some()),
            );
            input.liquid_flow_enabled = Some(self.modes.mode() != sim::MovementMode::Flying);
            let mut forced_sneak = false;
            let mut start_sprinting = false;
            let mut flight_request = None;
            let mut mode_error = None;
            let previous_modes = self.modes;
            let [move_sideways, move_forward] = ModeObservation::input_vector(input);
            let mut observed = ModeObservation {
                feet: state.position,
                on_ground: state.on_ground,
                in_water: self.last_environment.in_water,
                sprinting: sprint_request,
                sprint_blinded: input.effects.blindness,
                sprint_down: context.input.sprint_down,
                input_mode: context.input_mode,
                requested_movement: state.requested_movement,
                move_sideways,
                move_forward,
                sneaking: sneak_request,
                pitch: context.pitch,
                yaw: input.yaw_degrees as f32,
                liquid_attach_height: self.eye_offset.height(1.0),
                liquid_contact_height: input.liquid_contact_height,
                jump_edge: self.jump_edge_pending,
                jump_down: context.input.jump.held || self.jump_pulse_pending,
            };
            self.anchor_state.prepare_tick(
                world,
                self.modes,
                context.mode_intent,
                &mut observed,
                input,
            );
            state.position = observed.feet;
            match self.modes.select(context.mode_intent, observed, world) {
                Ok(choice) => {
                    input.mode = choice.mode;
                    input.sprinting = choice.sprinting;
                    start_sprinting = choice.start_sprinting;
                    input.sneaking = sneak_request || choice.forced_sneak;
                    forced_sneak = choice.forced_sneak;
                    flight_request = choice.flight_request;
                }
                Err(_) if input.immobile => {
                    // Missing terrain cannot turn an authoritative freeze into a blocked tick.
                    self.modes = previous_modes;
                    input.mode = self.modes.mode();
                    input.sneaking = sneak_request;
                }
                Err(error) => mode_error = Some(error),
            }
            if input.mode == sim::MovementMode::Riding {
                input.sneaking = false;
            }
            speed::apply_sprint_speed(&mut movement_speed, &mut input, start_sprinting);
            self.modes.record_controls(input, context.input.sneak_down);
            // A rider's position is its seat on the mount, not a simulated result.
            let mut ride_delta = None;
            if input.mode == sim::MovementMode::Riding
                && let Some(seat) = context
                    .mode_intent
                    .ride_seat
                    .filter(|seat| seat.iter().all(|axis| axis.is_finite()))
            {
                let seat = Vec3::new(f64::from(seat[0]), f64::from(seat[1]), f64::from(seat[2]));
                ride_delta = Some([
                    (seat.x - state.position.x) as f32,
                    (seat.y - state.position.y) as f32,
                    (seat.z - state.position.z) as f32,
                ]);
                state.position = seat;
            }
            // A queued server impulse replaces this tick's starting velocity,
            // mirroring how Bedrock applies knockback as an absolute velocity.
            // The overlay is retained after application so a correction
            // rewind covering its tick re-applies it deterministically;
            // capacity bounds evict the oldest entries.
            let next_tick = state.tick.saturating_add(1);
            for overlay in self
                .server_motions
                .iter()
                .filter(|overlay| overlay.tick == next_tick)
            {
                state.velocity = overlay.velocity;
            }
            let before = state.position;
            // The liquid probe uses the prior tick's pose-adjusted eye anchor.
            // Retain the height with this input so correction replay samples
            // the same material cell instead of the rendered interpolation.
            input.liquid_attach_height = Some(f64::from(self.eye_offset.height(1.0)));
            let entry_velocity = [
                state.velocity.x as f32,
                state.velocity.y as f32,
                state.velocity.z as f32,
            ];
            let predicted = match mode_error {
                Some(error) => Err(sim::PredictionError::Simulation(SimulationError::World(
                    error,
                ))),
                None => {
                    let predicted =
                        self.history
                            .predict_with_controls(state, input, &self.simulator, world);
                    if matches!(
                        predicted,
                        Err(sim::PredictionError::Simulation(SimulationError::World(
                            sim::WorldQueryError::QueryExtentExceeded
                        )))
                    ) && let Some(launch) = input.riptide.take()
                    {
                        // Failed ticks commit nothing. Retire the release and retry ordinary
                        // movement when its impulse plus existing motion cannot fit the query.
                        self.riptide_pending = None;
                        crate::movement::diagnostics::note_skipped_authority(
                            "riptide",
                            f64::from(launch.level()),
                        );
                        self.history
                            .predict_with_controls(state, input, &self.simulator, world)
                    } else {
                        predicted
                    }
                }
            };
            match predicted {
                Ok(output) => {
                    self.skip_travel_pending = false;
                    let result = output.tick_result;
                    self.last_environment = result.environment;
                    while self.controller_history.len() >= self.history_capacity {
                        self.controller_history.pop_front();
                    }
                    let mut controller = ControllerFrame {
                        tick: state.tick,
                        speed: movement_speed,
                        start_sprinting,
                        speed_updates: std::mem::take(&mut speed_updates),
                        anchor_state: self.anchor_state,
                        entry_velocity,
                        eye_offset: self.eye_offset,
                        intent: context.mode_intent,
                        jump_edge: self.jump_edge_pending,
                        jump_down: observed.jump_down,
                        requested_sneak: sneak_request,
                        requested_sprint: sprint_request,
                        sprint_down: context.input.sprint_down,
                        server_sprinting: tick_input.server_sprinting,
                        input_mode: context.input_mode,
                        mode_override: None,
                        sneak_override: None,
                        sprint_override: None,
                        forced_sneak,
                        flight_request,
                        grounded_before_tick,
                        jump_repeated,
                        ride_delta,
                        actor_position: self.actor_position_pending.take(),
                        input,
                        modes: self.modes,
                        environment: result.environment,
                    };
                    controller.finish_eye(&mut self.eye_offset, state);
                    self.controller_history.push_back(controller);
                    effects.commit_successful_tick();
                    self.previous_position = before;
                    self.visual_correction.tick();
                    let world_identity = result.world_identity;
                    self.last_world_identity = Some(world_identity.clone());
                    frame.completed_ticks += 1;
                    self.prediction_sync.tick();
                    // The simulator owns initiation; held inputs alone cannot prove a jump.
                    let mut processed = ProcessedMovementState::next(
                        self.processed_jump_arc_active,
                        output.jump_initiated,
                        state.on_ground,
                        input.sneaking,
                        input.sprinting,
                    );
                    processed.mode = input.mode;
                    processed.spinning = state.spin_attack_ticks.is_some();
                    processed.spin_started = input.riptide.is_some()
                        && input.mode != sim::MovementMode::Riding
                        && !input.spectator;
                    processed.ride = context.mode_intent.ride;
                    processed.forced_sneak = forced_sneak;
                    processed.flight_request = flight_request;
                    if input.immobile || input.mode == sim::MovementMode::Riding {
                        // Frozen travel and mount-owned jumping cannot continue a local jump arc.
                        processed.jump_initiated = false;
                        processed.jump_arc_active = false;
                    }
                    self.processed_jump_arc_active = processed.jump_arc_active;
                    tick_input.jump.held |= self.jump_pulse_pending;
                    tick_input.jump.pressed |= self.jump_pulse_pending;
                    tick_input.jump.released |= self.jump_pulse_held && !tick_input.jump.held;
                    frame.samples.push(PhysicsMovementSample {
                        tick: state.tick,
                        position: [
                            state.position.x as f32,
                            state.position.y as f32 + PLAYER_NETWORK_OFFSET,
                            state.position.z as f32,
                        ],
                        movement: [
                            result.movement.x as f32,
                            result.movement.y as f32,
                            result.movement.z as f32,
                        ],
                        velocity: [
                            result.velocity.x as f32,
                            result.velocity.y as f32,
                            result.velocity.z as f32,
                        ],
                        move_vector: [
                            -output.controls.move_vector[0] as f32,
                            output.controls.move_vector[1] as f32,
                        ],
                        raw_move_vector: context.raw_move_vector,
                        analogue_move_vector: context.analogue_move_vector,
                        pitch: context.pitch,
                        yaw: input.yaw_degrees as f32,
                        head_yaw: context.head_yaw,
                        camera_orientation: context.camera_orientation,
                        jumping: input.jumping,
                        sneaking: input.sneaking,
                        input: tick_input,
                        sprinting: input.sprinting,
                        input_mode: context.input_mode,
                        grounded_before_tick,
                        grounded_after_tick: state.on_ground,
                        horizontal_collision: result.collisions.x || result.collisions.z,
                        vertical_collision: result.collisions.y,
                        jump_repeated,
                        processed,
                        world_identity,
                    });
                    if let (Some(delta), Some(sample)) = (ride_delta, frame.samples.last_mut()) {
                        sample.movement = delta;
                    }
                    while self.sample_history.len() >= self.history_capacity {
                        self.sample_history.pop_front();
                    }
                    self.sample_history.push_back(
                        frame
                            .samples
                            .last()
                            .expect("completed tick appended a movement sample")
                            .clone(),
                    );
                    self.live_posture = Default::default();
                    motion_ticks::retain(
                        &mut self.motion_ticks,
                        &mut self.motion_anchor,
                        self.sample_history
                            .back()
                            .expect("completed sample retained"),
                        self.controller_history
                            .back()
                            .expect("completed controller retained"),
                    );
                    self.jump_edge_pending = false;
                    if input.riptide.is_some() {
                        self.riptide_pending = None;
                    }
                    self.input_edges = Default::default();
                    self.jump_pulse_held = self.jump_pulse_pending;
                    self.jump_pulse_pending = false;
                    input.jump_pressed = false;
                }
                Err(error) => {
                    self.modes = previous_modes;
                    let retryable_collision_blocked = matches!(
                        &error,
                        sim::PredictionError::Simulation(error)
                            if is_retryable_collision_block(error)
                    );
                    if retryable_collision_blocked {
                        // Prediction is transactional: the failed tick did
                        // not mutate state or history. Discard all elapsed
                        // time in this blocked frame instead of retaining a
                        // retry backlog that could become a false overflow
                        // or a large catch-up burst when collision data
                        // returns or motion fits the query limit. New elapsed
                        // time starts a fresh tick.
                        self.previous_position = state.position;
                        self.eye_offset.finish_interpolation();
                        self.visual_correction.finish_interpolation();
                        self.accumulated_seconds = 0.0;
                        frame.dropped_ticks = 0;
                    } else {
                        // Keep a failed non-transient tick pending. The
                        // existing overflow count remains authoritative for
                        // genuine overload with usable collision data.
                        let unconsumed_ticks = allowed - tick_index;
                        self.accumulated_seconds +=
                            unconsumed_ticks as f64 * LOCAL_PHYSICS_TICK_SECONDS;
                    }
                    frame.blocked_tick_index = Some(tick_index);
                    frame.blocked = Some(match error {
                        sim::PredictionError::Simulation(error) => error,
                        sim::PredictionError::ZeroCapacity
                        | sim::PredictionError::StateHistoryDiverged { .. }
                        | sim::PredictionError::CorrectionNotRetained { .. } => {
                            unreachable!(
                                "local prediction uses a fresh non-zero sequential history"
                            )
                        }
                    });
                    break;
                }
            }
        }
        self.dropped_tick_count = self.dropped_tick_count.saturating_add(frame.dropped_ticks);
        frame
    }
}
