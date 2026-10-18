use super::*;

impl PhysicsSampleContext {
    /// Expires release slowdown and sprint blocking together before the next fixed tick.
    pub(super) fn for_next_tick(mut self, completed_tick: u64) -> Self {
        self.release_use_modifier = self
            .release_use_modifier
            .filter(|(through_tick, _)| completed_tick < *through_tick);
        self.mode_intent.sprint_start_blocked |= self.release_use_modifier.is_some();
        self
    }
}

/// Input edges and controller state retained alongside one completed simulation tick.
#[derive(Debug, Clone, Copy)]
pub(super) struct ControllerFrame {
    pub tick: u64,
    /// Recovery state after this tick, including freezes that defer the first probe.
    pub anchor_state: AnchorProbeState,
    /// Velocity before this tick applies liquid travel and drag.
    pub entry_velocity: [f32; 3],
    pub eye_offset: eye::LocalEyeOffset,
    pub intent: ModeIntent,
    pub jump_edge: bool,
    pub jump_down: bool,
    pub requested_sneak: bool,
    pub requested_sprint: bool,
    pub sprint_down: bool,
    pub server_sprinting: Option<bool>,
    pub input_mode: PlayerInputMode,
    pub mode_override: Option<sim::MovementMode>,
    pub sneak_override: Option<bool>,
    pub sprint_override: Option<bool>,
    pub forced_sneak: bool,
    pub flight_request: Option<bool>,
    pub grounded_before_tick: bool,
    pub jump_repeated: bool,
    pub ride_delta: Option<[f32; 3]>,
    pub input: MovementInput,
    pub modes: ModeTracker,
    pub environment: sim::MovementEnvironment,
}

impl ControllerFrame {
    /// Advances the pose eye once after a successful tick and retains its interpolation state.
    pub(super) fn finish_eye(&mut self, eye: &mut eye::LocalEyeOffset, state: &PlayerState) {
        eye.tick(
            self.input.mode,
            self.input.sneaking,
            state.spin_attack_ticks.is_some(),
        );
        self.eye_offset = *eye;
    }

    /// Re-evaluates pose, jump edges and ride placement from the corrected pre-tick state.
    pub(super) fn prepare(
        &mut self,
        modes: &mut ModeTracker,
        anchor_state: &mut AnchorProbeState,
        environment: sim::MovementEnvironment,
        state: &mut PlayerState,
        input: &mut MovementInput,
        world: &dyn CollisionWorld,
    ) -> Result<(), SimulationError> {
        input.no_clip = self.intent.no_clip;
        let previous_sprinting = input.sprinting;
        if let Some(sprinting) = self.server_sprinting {
            modes.restore_controls(sprinting, modes.sneaking());
        }
        self.grounded_before_tick = state.on_ground;
        self.jump_repeated = !input.immobile
            && !self.jump_edge
            && input.jumping
            && state.on_ground
            && state.jump_delay == 0;
        input.jump_pressed = self.jump_edge || self.jump_repeated;
        if input.mode != self.input.mode {
            self.mode_override = Some(input.mode);
        }
        if input.sneaking != self.input.sneaking {
            self.sneak_override = Some(input.sneaking);
        }
        if input.sprinting != self.input.sprinting {
            self.sprint_override = Some(input.sprinting);
        }
        input.liquid_contact_height =
            Some(modes.liquid_contact_height(state.spin_attack_ticks.is_some()));
        input.liquid_flow_enabled = Some(modes.mode() != sim::MovementMode::Flying);
        input.sprinting = self.sprint_override.unwrap_or(self.requested_sprint);
        if let Some(sprinting) = self.sprint_override {
            modes.restore_controls(sprinting, modes.sneaking());
        }
        let previous_modes = *modes;
        let [move_sideways, move_forward] = ModeObservation::input_vector(*input);
        let mut observed = ModeObservation {
            feet: state.position,
            on_ground: state.on_ground,
            in_water: environment.in_water,
            sprinting: input.sprinting,
            sprint_blinded: input.effects.blindness,
            sprint_down: self.sprint_down,
            input_mode: self.input_mode,
            requested_movement: state.requested_movement,
            move_sideways,
            move_forward,
            sneaking: self.requested_sneak,
            pitch: input.pitch_degrees as f32,
            yaw: input.yaw_degrees as f32,
            liquid_attach_height: input
                .liquid_attach_height
                .map_or(protocol::PLAYER_NETWORK_OFFSET, |height| height as f32),
            liquid_contact_height: input.liquid_contact_height,
            jump_edge: self.jump_edge,
            jump_down: self.jump_down,
        };
        anchor_state.prepare_tick(world, *modes, self.intent, &mut observed, *input);
        state.position = observed.feet;
        let choice = modes.select(self.intent, observed, world);
        let choice = match choice {
            Ok(choice) => choice,
            Err(_) if input.immobile => {
                *modes = previous_modes;
                super::super::locomotion::ModeChoice {
                    mode: modes.mode(),
                    forced_sneak: self.forced_sneak,
                    sprinting: input.sprinting,
                    flight_request: None,
                }
            }
            Err(error) => return Err(error.into()),
        };
        input.mode = self.mode_override.unwrap_or(choice.mode);
        input.sprinting = self.sprint_override.unwrap_or(choice.sprinting);
        modes.restore_mode(input.mode);
        self.forced_sneak = choice.forced_sneak;
        self.flight_request = choice.flight_request;
        input.sneaking = self.sneak_override.unwrap_or(self.requested_sneak) || choice.forced_sneak;
        if matches!(
            input.mode,
            sim::MovementMode::Crawling | sim::MovementMode::Gliding | sim::MovementMode::Riding
        ) {
            input.sprinting = false;
        }
        if input.mode == sim::MovementMode::Riding {
            input.sneaking = false;
        }
        modes.restore_controls(input.sprinting, input.sneaking);
        if self.sprint_override.is_some() || self.mode_override.is_some() {
            crate::movement::speed_authority::preserve_effective_speed(input, previous_sprinting);
        }
        self.ride_delta = None;
        if input.mode == sim::MovementMode::Riding
            && let Some(seat) = self
                .intent
                .ride_seat
                .filter(|seat| seat.iter().all(|v| v.is_finite()))
        {
            let seat = Vec3::new(f64::from(seat[0]), f64::from(seat[1]), f64::from(seat[2]));
            self.ride_delta = Some([
                (seat.x - state.position.x) as f32,
                (seat.y - state.position.y) as f32,
                (seat.z - state.position.z) as f32,
            ]);
            state.position = seat;
        }
        modes.record_controls(*input, self.requested_sneak);
        self.entry_velocity = [
            state.velocity.x as f32,
            state.velocity.y as f32,
            state.velocity.z as f32,
        ];
        self.modes = *modes;
        self.anchor_state = *anchor_state;
        self.input = *input;
        Ok(())
    }
}
