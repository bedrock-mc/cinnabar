//! Local camera effects shared by native packet callbacks and the client adapter.

use serde::{Deserialize, Serialize};

/// The optional `camera-frame` export receives the same bounded frame observations.
pub const CAPABILITY: u32 = 4;
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

/// A callback's effects are applied only within the current host-owned camera lease.
/// Rotation is consumed once; presentation remains valid until the next frame result.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Effects {
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
        self.rotation
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
}
