//! Native frame admission for the shared movement simulator. Session authority,
//! abilities, prediction reconciliation and non-walking modes remain to be wired.

use std::time::Duration;

use semantic_input::{Action, ActionSnapshot};
use sim::{CollisionWorld, MovementInput, PlayerState, SimulationError, Simulator, Vec3};
use simulation_clock::{MAX_FRAME_ELAPSED, TICK_DURATION};

pub struct PlayerController {
    state: PlayerState,
    previous_position: Vec3,
    pending: Duration,
    jump_pressed: bool,
    yaw: f32,
    pitch: f32,
}

impl PlayerController {
    pub fn new(state: PlayerState) -> Self {
        Self {
            previous_position: state.position,
            state,
            pending: Duration::ZERO,
            jump_pressed: false,
            yaw: 0.,
            pitch: 0.,
        }
    }

    pub fn state(&self) -> &PlayerState {
        &self.state
    }

    pub fn rotation(&self) -> [f32; 2] {
        [self.pitch, self.yaw]
    }

    /// Camera pose is host policy; callers supply the active mode's eye height.
    pub fn eye(&self, height: f64) -> Vec3 {
        let alpha = (self.pending.as_secs_f64() / TICK_DURATION.as_secs_f64()).min(1.);
        self.previous_position
            + (self.state.position - self.previous_position) * alpha
            + Vec3::new(0., height, 0.)
    }

    /// Retains press edges until a completed fixed tick. Menu pauses clear pending
    /// time and edges; resuming cannot run a backlog or replay a quarantined jump.
    pub fn advance(
        &mut self,
        actions: &ActionSnapshot,
        elapsed: Duration,
        active: bool,
        look_degrees_per_second: f32,
        world: &impl CollisionWorld,
    ) -> Result<u32, SimulationError> {
        if !active || actions.authority_released {
            self.pending = Duration::ZERO;
            self.jump_pressed = false;
            self.previous_position = self.state.position;
            return Ok(0);
        }
        let elapsed = elapsed.min(MAX_FRAME_ELAPSED);
        let look = look_degrees_per_second * elapsed.as_secs_f32();
        let yaw = self.yaw + actions.look_delta[0] * look;
        // Semantic look Y and the shared simulator's pitch are both down-positive.
        let pitch = self.pitch + actions.look_delta[1] * look;
        if !yaw.is_finite() || !pitch.is_finite() {
            return Err(SimulationError::NonFiniteInput {
                field: "camera rotation",
            });
        }
        self.yaw = yaw.rem_euclid(360.);
        self.pitch = pitch.clamp(-89.9, 89.9);
        self.jump_pressed |= actions.phases[Action::Jump as usize].pressed;
        self.pending = self.pending.saturating_add(elapsed).min(MAX_FRAME_ELAPSED);
        let mut ticks = 0;
        while self.pending >= TICK_DURATION {
            let before = self.state.position;
            Simulator::default().tick(
                &mut self.state,
                MovementInput {
                    // The semantic router is right-positive; the simulator is left-positive.
                    strafe: -f64::from(actions.raw_movement[0]),
                    forward: f64::from(actions.raw_movement[1]),
                    yaw_degrees: f64::from(self.yaw),
                    pitch_degrees: f64::from(self.pitch),
                    jumping: actions.phases[Action::Jump as usize].held,
                    jump_pressed: self.jump_pressed,
                    sneaking: actions.phases[Action::Sneak as usize].held,
                    sprinting: actions.phases[Action::Sprint as usize].held
                        && actions.raw_movement[1] > 0.,
                    move_vector_is_raw: true,
                    ..MovementInput::default()
                },
                world,
            )?;
            self.previous_position = before;
            self.jump_pressed = false;
            self.pending -= TICK_DURATION;
            ticks += 1;
        }
        Ok(ticks)
    }

    /// Column-major view matrix matching the simulator's pitch/yaw convention.
    pub fn view_matrix(&self, eye_height: f64) -> [f32; 16] {
        let eye = self.eye(eye_height);
        let f = sim::view_direction(self.pitch, self.yaw);
        let horizontal = (f.x * f.x + f.z * f.z).sqrt();
        let right = Vec3::new(-f.z / horizontal, 0., f.x / horizontal);
        let up = Vec3::new(-right.z * f.y, right.z * f.x - right.x * f.z, right.x * f.y);
        let dot = |a: Vec3, b: Vec3| (a.x * b.x + a.y * b.y + a.z * b.z) as f32;
        [
            right.x as f32,
            up.x as f32,
            -f.x as f32,
            0.,
            right.y as f32,
            up.y as f32,
            -f.y as f32,
            0.,
            right.z as f32,
            up.z as f32,
            -f.z as f32,
            0.,
            -dot(right, eye),
            -dot(up, eye),
            dot(f, eye),
            1.,
        ]
    }
}

#[cfg(test)]
mod tests;
