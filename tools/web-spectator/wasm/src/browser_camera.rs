//! Actual streamed movement and damage drive Cinnabar's native camera effects.
use bevy::{platform::time::Instant, prelude::*};
use render::camera::{
    CameraHurtState, FirstPersonHandMotion, HandSwayState, LocalHurtEvent, WalkBobState,
    walk_bob_effect,
};

use crate::browser_model::Fighter;

pub(super) struct PovMotion {
    player: Option<String>,
    hurt_at: Option<String>,
    last_update: Instant,
    bob: WalkBobState,
    sway: HandSwayState,
    hurt: CameraHurtState,
}

impl Default for PovMotion {
    fn default() -> Self {
        Self {
            player: None,
            hurt_at: None,
            last_update: Instant::now(),
            bob: WalkBobState::default(),
            sway: HandSwayState::default(),
            hurt: CameraHurtState::default(),
        }
    }
}

impl PovMotion {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(super) fn update(&mut self, fighter: &Fighter, base: Transform) -> (Transform, Mat4) {
        if self.player.as_deref() != Some(fighter.id.as_str()) {
            self.reset();
            self.player = Some(fighter.id.clone());
        }
        let now = Instant::now();
        let delta_seconds = now.duration_since(self.last_update).as_secs_f32().min(0.2);
        self.last_update = now;
        self.bob.advance(
            base.translation,
            fighter.on_ground,
            !fighter.dead,
            delta_seconds,
        );
        self.hurt.advance(delta_seconds);
        let hurt_identity = fighter.hurt_id.as_ref().or(fighter.hurt_at.as_ref());
        if self.hurt_at.as_ref() != hurt_identity {
            self.hurt_at = hurt_identity.cloned();
            if let Some(stamp) = fighter.hurt_at.as_deref() {
                let age_seconds =
                    ((js_sys::Date::now() - js_sys::Date::parse(stamp)) / 1000.0) as f32;
                // The stream has a committed hurt event but no damage-direction packet.
                // Native directionless tilt is the explicit fallback for that case.
                self.hurt
                    .register_at_age(LocalHurtEvent::default(), age_seconds);
            }
        }
        let (yaw, pitch, _) = base.rotation.to_euler(EulerRot::YXZ);
        self.sway.advance(pitch, yaw, delta_seconds);
        let (sway_pitch_radians, sway_yaw_radians) = self.sway.sway_radians();
        let motion = FirstPersonHandMotion {
            bob: walk_bob_effect(self.bob.walk_distance(), self.bob.bob()),
            hurt: self.hurt.view_matrix(yaw),
            sway_pitch_radians,
            sway_yaw_radians,
        };
        let camera = Transform::from_matrix(base.to_matrix() * motion.view_matrix().inverse());
        if camera.translation.is_finite() && camera.rotation.is_finite() {
            (camera, motion.matrix())
        } else {
            (base, Mat4::IDENTITY)
        }
    }
}
