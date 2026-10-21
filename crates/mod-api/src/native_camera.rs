//! Local camera effects shared by native packet callbacks and the client adapter.

use serde::{Deserialize, Serialize};

/// The optional `camera-frame` export receives the same bounded frame observations.
pub const CAPABILITY: u32 = 4;
/// Optional retained look and walking intents on the camera-frame transport.
pub const INPUT_INTENTS: u32 = 64;
/// Maximum detached-view movement, in blocks per real second.
pub const MAX_FLY_SPEED: f32 = 100.0;
/// Maximum orbit boom length accepted by the detached camera API.
pub const MAX_ORBIT_DISTANCE: f32 = 32.0;

/// A detached view keeps native look separate from the body's network-facing pose.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DetachedView {
    Orbit {
        distance: f32,
    },
    Fly {
        horizontal_speed: f32,
        vertical_speed: f32,
    },
}

impl DetachedView {
    /// Bounds local presentation speed and orbit distance before applying an effect.
    pub fn is_valid(self) -> bool {
        match self {
            Self::Orbit { distance } => {
                distance.is_finite() && (0.0..=MAX_ORBIT_DISTANCE).contains(&distance)
            }
            Self::Fly {
                horizontal_speed,
                vertical_speed,
            } => [horizontal_speed, vertical_speed]
                .iter()
                .all(|speed| speed.is_finite() && (0.0..=MAX_FLY_SPEED).contains(speed)),
        }
    }
}

/// Absolute view intent, evaluated against the current pose on every host frame.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LookIntent {
    pub yaw: f32,
    pub pitch: f32,
    /// Radians per second; independent of callback delivery frequency.
    pub speed: f32,
}

impl LookIntent {
    /// Rejects non-finite headings and unbounded turning rates.
    pub fn is_valid(self) -> bool {
        self.yaw.is_finite()
            && self.pitch.is_finite()
            && self.pitch.abs() <= std::f32::consts::FRAC_PI_2
            && self.speed.is_finite()
            && (0.0..=20.0).contains(&self.speed)
    }
}

/// Ordinary walking controls in a supplied yaw frame, before client slowdown and physics.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MovementIntent {
    pub yaw: f32,
    /// Positive X is right, positive Y is forward.
    pub axes: [f32; 2],
    pub jump: bool,
    pub sneak: bool,
    pub sprint: bool,
}

impl MovementIntent {
    /// Accepts only finite, unit-bounded ordinary movement controls.
    pub fn is_valid(self) -> bool {
        self.yaw.is_finite()
            && self.axes.iter().all(|axis| axis.is_finite())
            && self.axes[0].hypot(self.axes[1]) <= 1.0001
    }
}

/// A callback's effects are applied only within the current host-owned camera lease.
/// Rotation is consumed once; retained look and movement require INPUT_INTENTS.
/// The host expires retained controls and revokes them at input/world ownership boundaries.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Effects {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub look: Option<LookIntent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub movement: Option<MovementIntent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<super::native_frame::Delta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detached: Option<DetachedView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hurt_intensity: Option<f32>,
    #[serde(default)]
    pub preserve_teleport_rotation: bool,
}

impl Effects {
    /// Uses the owning camera and damage-style bounds for every supported effect.
    pub fn is_valid(&self) -> bool {
        self.look.is_none_or(LookIntent::is_valid)
            && self.movement.is_none_or(MovementIntent::is_valid)
            && !(self.look.is_some() && (self.rotation.is_some() || self.detached.is_some()))
            && !(self.movement.is_some() && self.detached.is_some())
            && self
                .rotation
                .is_none_or(super::native_frame::Delta::is_valid)
            && self.detached.is_none_or(DetachedView::is_valid)
            && self
                .hurt_intensity
                .is_none_or(|value| render_api::HurtCameraStyle::new(value, false).is_some())
            && !(self.rotation.is_some() && self.detached.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_output_fixture_matches_the_sdk() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../native-camera-v1.json")).unwrap();
        assert_eq!(fixture["capabilities"], CAPABILITY);
        let effects: Effects = serde_json::from_value(fixture["effects"].clone()).unwrap();
        assert!(effects.is_valid());
        assert_eq!(serde_json::to_value(effects).unwrap(), fixture["effects"]);
    }

    #[test]
    fn effects_reject_conflicting_or_unbounded_camera_requests() {
        assert!(Effects::default().is_valid());
        assert!(
            !Effects {
                detached: Some(DetachedView::Orbit { distance: f32::NAN }),
                ..Default::default()
            }
            .is_valid()
        );
        assert!(
            !Effects {
                detached: Some(DetachedView::Fly {
                    horizontal_speed: 101.0,
                    vertical_speed: 1.0
                }),
                ..Default::default()
            }
            .is_valid()
        );
        assert!(
            !Effects {
                hurt_intensity: Some(f32::INFINITY),
                ..Default::default()
            }
            .is_valid()
        );
        assert!(
            !Effects {
                rotation: Some(super::super::native_frame::Delta {
                    yaw: 0.0,
                    pitch: 0.0
                }),
                detached: Some(DetachedView::Orbit { distance: 4.0 }),
                ..Default::default()
            }
            .is_valid()
        );
        assert!(serde_json::from_str::<Effects>(r#"{"unknown":true}"#).is_err());
    }
    #[test]
    fn input_intents_reject_invalid_axes_and_conflicting_look_owners() {
        let look = LookIntent {
            yaw: 1.0,
            pitch: 0.0,
            speed: 3.0,
        };
        let movement = MovementIntent {
            yaw: 1.0,
            axes: [0.0, 1.0],
            jump: true,
            sneak: false,
            sprint: false,
        };
        assert!(
            Effects {
                look: Some(look),
                movement: Some(movement),
                ..Default::default()
            }
            .is_valid()
        );
        assert!(
            !MovementIntent {
                axes: [1.0, 1.0],
                ..movement
            }
            .is_valid()
        );
        assert!(
            !MovementIntent {
                yaw: f32::NAN,
                ..movement
            }
            .is_valid()
        );
        assert!(
            !LookIntent {
                speed: f32::INFINITY,
                ..look
            }
            .is_valid()
        );
        assert!(
            !Effects {
                look: Some(look),
                detached: Some(DetachedView::Orbit { distance: 4.0 }),
                ..Default::default()
            }
            .is_valid()
        );
        assert!(
            !Effects {
                look: Some(look),
                rotation: Some(super::super::native_frame::Delta {
                    yaw: 0.1,
                    pitch: 0.0
                }),
                ..Default::default()
            }
            .is_valid()
        );
    }
}
