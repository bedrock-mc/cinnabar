//! Versioned native frame wire contract for trusted WASI packet mods.

use serde::{Deserialize, Serialize};

/// JSON contract version, independent of the packet relay's existing WIT version.
pub const VERSION: u32 = 1;
/// A host can supply input-owned player frames and accept local camera deltas.
pub const CAMERA_PLAYERS: u32 = 1;
/// Maximum JSON input or output bytes in one native frame callback.
pub const MAX_BYTES: usize = 64 * 1024;

/// One nearby remote player's runtime identity and feet position.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Player {
    pub runtime_id: u64,
    pub feet: [f32; 3],
}

/// Captured client gameplay, using actor YXZ radians: yaw left, pitch up.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Frame {
    pub version: u32,
    pub session: u64,
    pub dimension: i32,
    pub eye: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub attack_held: bool,
    pub seconds: f32,
    pub players: Vec<Player>,
}

impl Frame {
    /// Rejects unsupported or non-finite observations before crossing the guest boundary.
    pub fn is_valid(&self) -> bool {
        self.version == VERSION
            && self.eye.iter().all(|value| value.is_finite())
            && self.yaw.is_finite()
            && self.pitch.is_finite()
            && self.seconds.is_finite()
            && (0.0..=1.0).contains(&self.seconds)
            && self.players.len() <= crate::MAX_GAMEPLAY_PLAYERS
            && self.players.iter().all(|player| {
                player.runtime_id != 0 && player.feet.iter().all(|value| value.is_finite())
            })
    }
}

/// Once-consumed local rotation; an absent output leaves the camera untouched.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Delta {
    pub yaw: f32,
    pub pitch: f32,
}

impl Delta {
    /// Uses the same finite accumulated camera bounds as regular player mods.
    pub fn is_valid(self) -> bool {
        self.yaw.is_finite()
            && self.pitch.is_finite()
            && self.yaw.abs() <= crate::MAX_CAMERA_DELTA_RADIANS
            && self.pitch.abs() <= crate::MAX_CAMERA_DELTA_RADIANS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_wire_fixture_matches_the_sdk() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../native-frame-v1.json")).unwrap();
        assert_eq!(fixture["version"], VERSION);
        assert_eq!(fixture["capabilities"], CAMERA_PLAYERS);
        assert_eq!(fixture["max_bytes"], MAX_BYTES);
        assert_eq!(fixture["max_players"], crate::MAX_GAMEPLAY_PLAYERS);
        assert_eq!(
            fixture["max_delta_radians"],
            crate::MAX_CAMERA_DELTA_RADIANS
        );
        let frame: Frame = serde_json::from_value(fixture["frame"].clone()).unwrap();
        assert!(frame.is_valid());
        let delta: Delta = serde_json::from_value(fixture["delta"].clone()).unwrap();
        assert!(delta.is_valid());
        assert_eq!(serde_json::to_value(frame).unwrap(), fixture["frame"]);
        assert_eq!(serde_json::to_value(delta).unwrap(), fixture["delta"]);
    }
}
