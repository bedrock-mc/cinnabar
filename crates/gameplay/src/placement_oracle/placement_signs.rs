//! Retained oracle: Standing and wall sign identities selected from one sign item.

use serde_json::{Map, Value, json};

use crate::placement_oracle::placement_state::{integer_in, only_keys, value};
use crate::placement_state::PlacementInput;

const SIGN_ROTATIONS: u8 = 16;

/// Chooses the wood-matched sign and its standing rotation or wall attachment face.
pub(crate) fn sign_states(
    identifier: &str,
    canonical: &str,
    input: PlacementInput,
) -> Option<(String, Map<String, Value>)> {
    if !identifier.starts_with("minecraft:") || !(1..=5).contains(&input.face) {
        return None;
    }
    let states = serde_json::from_str(canonical).ok()?;
    let prefix = if let Some(prefix) = identifier.strip_suffix("standing_sign") {
        if !only_keys(&states, &["ground_sign_direction"]) {
            return None;
        }
        if value(&states, "ground_sign_direction")?.as_u64()? >= u64::from(SIGN_ROTATIONS) {
            return None;
        }
        prefix
    } else {
        let prefix = identifier.strip_suffix("wall_sign")?;
        if !only_keys(&states, &["facing_direction"]) {
            return None;
        }
        integer_in(&states, "facing_direction", &[2, 3, 4, 5])?;
        prefix
    };
    let (suffix, key, direction) = if input.face == 1 {
        if !input.yaw.is_finite() {
            return None;
        }
        let rotation = ((input.yaw + 180.0) * f32::from(SIGN_ROTATIONS) / 360.0 + 0.5).floor();
        if !rotation.is_finite() {
            return None;
        }
        (
            "standing_sign",
            "ground_sign_direction",
            rotation.rem_euclid(f32::from(SIGN_ROTATIONS)) as u8,
        )
    } else {
        ("wall_sign", "facing_direction", input.face)
    };
    Some((
        format!("{prefix}{suffix}"),
        serde_json::from_value(json!({key:{"type":"int","value":direction}})).ok()?,
    ))
}

/// Maps a resolved ordinary sign to the cell that supports it.
pub(crate) fn support_face(identifier: &str, states: &Map<String, Value>) -> Option<u8> {
    if identifier.ends_with("standing_sign") && only_keys(states, &["ground_sign_direction"]) {
        return Some(0);
    }
    if identifier.ends_with("wall_sign") && only_keys(states, &["facing_direction"]) {
        integer_in(states, "facing_direction", &[2, 3, 4, 5])?;
        return Some(value(states, "facing_direction")?.as_u64()? as u8 ^ 1);
    }
    None
}
