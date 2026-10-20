//! Applies negotiated packet intents to the client's ordinary movement pipeline.
use bevy::prelude::*;
use mod_api::native_camera::{Effects, LookIntent};

/// Synthesized buttons are released through the same finalized input as physical controls.
#[derive(Default, Resource)]
pub(super) struct NativeInput {
    pub(super) scope: Option<(u64, i32)>,
    pub(super) movement: semantic_input::finalized::MovementOverlay,
}

impl NativeInput {
    /// Retires controls through the current world's finalized frame before releasing their owner.
    fn apply(
        &mut self,
        input: &mut semantic_input::finalized::FinalizedInput,
        scope: Option<(u64, i32)>,
        intent: Option<([f32; 2], [bool; 3])>,
    ) {
        if self.scope.zip(scope).is_some_and(|(old, new)| old != new) {
            self.movement = Default::default();
        }
        self.scope = scope;
        self.movement.apply(input, intent);
    }
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
    /// Both native callback adapters finish camera rotation before resolving world-relative walking.
    pub(super) fn apply_native_controls(
        &mut self,
        effects: Option<&Effects>,
        delta: Option<mod_host::CameraDelta>,
        ui: &client_ui::ui_runtime::UiRuntime,
    ) {
        self.apply_native_look(effects);
        if let Some(delta) = delta {
            self.apply(delta);
        }
        self.apply_native_input(effects, ui);
    }

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
        state.apply(input, scope, intent);
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
    #[test]
    fn losing_walking_eligibility_publishes_the_retiring_buttons() {
        let fresh = || {
            let mut router = semantic_input::SemanticInputRouter::default();
            router
                .route(semantic_input::DeviceFrame::default())
                .unwrap();
            semantic_input::finalized::FinalizedInput::from_finalized(router.finalize().unwrap())
        };
        let mut state = NativeInput::default();
        state.apply(
            &mut fresh(),
            Some((1, 0)),
            Some(([0.0, 1.0], [true, true, false])),
        );
        let mut released = fresh();
        state.apply(&mut released, None, None);
        assert!(released.phase(semantic_input::Action::Jump).released);
        assert!(released.phase(semantic_input::Action::Sneak).released);
        let mut next_world = fresh();
        state.apply(&mut next_world, Some((2, 0)), None);
        assert!(!next_world.phase(semantic_input::Action::Jump).released);
    }
}
