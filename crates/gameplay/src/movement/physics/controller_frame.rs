use super::*;

/// Input edges and controller state retained alongside one completed simulation tick.
#[derive(Debug, Clone, Copy)]
pub(super) struct ControllerFrame {
    pub tick: u64,
    pub intent: ModeIntent,
    pub jump_edge: bool,
    pub fly_toggle: bool,
    pub requested_sneak: bool,
    pub requested_sprint: bool,
    pub mode_override: Option<sim::MovementMode>,
    pub sneak_override: Option<bool>,
    pub sprint_override: Option<bool>,
    pub forced_sneak: bool,
    pub grounded_before_tick: bool,
    pub jump_repeated: bool,
    pub ride_delta: Option<[f32; 3]>,
    pub input: MovementInput,
    pub modes: ModeTracker,
    pub environment: sim::MovementEnvironment,
}

impl ControllerFrame {
    /// Re-evaluates pose, jump edges and ride placement from the corrected pre-tick state.
    pub(super) fn prepare(
        &mut self,
        modes: &mut ModeTracker,
        environment: sim::MovementEnvironment,
        state: &mut PlayerState,
        input: &mut MovementInput,
        world: &dyn CollisionWorld,
    ) -> Result<(), SimulationError> {
        let previous_sprinting = input.sprinting;
        self.grounded_before_tick = state.on_ground;
        self.jump_repeated =
            !self.jump_edge && input.jumping && state.on_ground && state.jump_delay == 0;
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
        input.liquid_contact_height = Some(modes.contact_height());
        input.liquid_flow_enabled = Some(modes.mode() != sim::MovementMode::Flying);
        input.sprinting = self.sprint_override.unwrap_or(self.requested_sprint);
        if let Some(sprinting) = self.sprint_override {
            modes.restore_controls(sprinting, modes.sneaking());
        }
        let choice = modes.select(
            self.intent,
            self.fly_toggle,
            ModeObservation {
                feet: state.position,
                on_ground: state.on_ground,
                velocity_y: state.velocity.y,
                in_water: environment.in_water,
                in_lava: environment.in_lava,
                sprinting: input.sprinting,
                move_sideways: input.strafe as f32,
                move_forward: input.forward as f32,
                sneaking: self.requested_sneak,
                pitch: input.pitch_degrees as f32,
                yaw: input.yaw_degrees as f32,
                liquid_attach_height: input
                    .liquid_attach_height
                    .map_or(protocol::PLAYER_NETWORK_OFFSET, |height| height as f32),
                jumping: input.jumping,
                jump_edge: self.jump_edge,
            },
            world,
        )?;
        input.mode = self.mode_override.unwrap_or(choice.mode);
        input.sprinting = self.sprint_override.unwrap_or(choice.sprinting);
        modes.restore_mode(input.mode);
        self.forced_sneak = choice.forced_sneak;
        input.sneaking = self.sneak_override.unwrap_or(self.requested_sneak) || choice.forced_sneak;
        if matches!(
            input.mode,
            sim::MovementMode::Crawling | sim::MovementMode::Gliding | sim::MovementMode::Riding
        ) {
            input.sprinting = false;
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
        self.modes = *modes;
        self.input = *input;
        Ok(())
    }
}
