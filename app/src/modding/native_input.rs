//! Applies negotiated packet intents to the client's ordinary movement pipeline.
use bevy::prelude::*;
use mod_api::native_camera::{Effects, LookIntent};

/// Synthesized buttons are released through the same finalized input as physical controls.
#[derive(Default, Resource)]
pub(super) struct NativeInput {
    pub(super) scope: Option<(u64, i32)>,
    pub(super) movement: semantic_input::finalized::MovementOverlay,
}

/// Computes an absolute target's bounded step from the current host pose and frame duration.
pub(super) fn look_delta(
    look: LookIntent,
    yaw: f32,
    pitch: f32,
    seconds: f32,
) -> mod_host::CameraDelta {
    let error = Vec2::new(
        (look.yaw - yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI,
        look.pitch - pitch,
    );
    let step = error.clamp_length_max(look.speed * seconds.clamp(0.0, 0.1));
    mod_host::CameraDelta {
        yaw: step.x,
        pitch: step.y,
    }
}

impl super::gameplay::GameplayContext<'_> {
    /// Native walking never takes a mounted, detached, UI-owned or manually moving body.
    pub(super) fn apply_native_input(
        &mut self,
        effects: Option<&Effects>,
        ui: &client_ui::ui_runtime::UiRuntime,
    ) {
        let scope = self.walking_scope(ui);
        let Some(state) = self.native_input.as_mut() else {
            return;
        };
        if state.scope != scope {
            state.movement = Default::default();
            state.scope = scope;
        }
        let Some(input) = self.input.as_mut() else {
            return;
        };
        let manual = input.raw_movement() != [0.0; 2]
            || input.phase(semantic_input::Action::Jump).held
            || input.phase(semantic_input::Action::Sneak).held
            || input
                .snapshot()
                .is_none_or(|input| input.authority_released);
        let movement = effects
            .and_then(|effects| effects.movement)
            .filter(|_| scope.is_some() && !manual);
        let intent = movement.zip(self.view.as_ref()).map(|(movement, view)| {
            let (yaw, _, _) = view.rotation().to_euler(EulerRot::YXZ);
            let angle = movement.yaw - yaw;
            let (sin, cos) = angle.sin_cos();
            let [x, z] = movement.axes;
            (
                [cos * x - sin * z, sin * x + cos * z],
                [movement.jump, movement.sneak, movement.sprint],
            )
        });
        state.movement.apply(input, intent);
    }

    /// Applies absolute look at render cadence, rather than consuming asynchronous relative steps.
    pub(super) fn apply_native_look(&mut self, effects: Option<&Effects>) {
        if let Some(look) = effects.and_then(|effects| effects.look) {
            let Some(view) = self.view.as_ref() else {
                return;
            };
            let (yaw, pitch, _) = view.rotation().to_euler(EulerRot::YXZ);
            let seconds = self
                .real_time
                .as_ref()
                .map_or(0.0, |time| time.delta_secs());
            self.apply(look_delta(look, yaw, pitch, seconds));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_look_advances_on_every_host_frame_without_overshoot() {
        let target = LookIntent {
            yaw: 0.5,
            pitch: -0.2,
            speed: 2.0,
        };
        for fps in [30, 60, 144] {
            let (mut yaw, mut pitch) = (0.0, 0.0);
            for _ in 0..fps {
                let delta = look_delta(target, yaw, pitch, 1.0 / fps as f32);
                assert!(delta.yaw.hypot(delta.pitch) <= 2.0 / fps as f32 + 1e-6);
                yaw += delta.yaw;
                pitch += delta.pitch;
            }
            assert!((yaw - target.yaw).abs() < 1e-5);
            assert!((pitch - target.pitch).abs() < 1e-5);
        }
        let delta = look_delta(
            LookIntent {
                yaw: -3.13,
                pitch: 0.0,
                speed: 2.0,
            },
            3.13,
            0.0,
            1.0 / 60.0,
        );
        assert!(delta.yaw > 0.0 && delta.yaw < 0.03);
    }
}
