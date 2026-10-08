//! Standing and wall sign identities selected from one sign item.

use serde_json::{Map, Value, json};

use crate::placement_state::{PlacementInput, integer_in, only_keys, value};

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
    } else if let Some(prefix) = identifier.strip_suffix("wall_sign") {
        if !only_keys(&states, &["facing_direction"]) {
            return None;
        }
        integer_in(&states, "facing_direction", &[2, 3, 4, 5])?;
        prefix
    } else {
        return None;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standing_rotations_cover_sixteen_steps_and_strict_rounding_boundaries() {
        let canonical = r#"{"ground_sign_direction":{"type":"int","value":0}}"#;
        for rotation in 0..SIGN_ROTATIONS {
            let yaw = rotation as f32 * 22.5 - 180.0;
            let input = PlacementInput {
                face: 1,
                yaw,
                pitch: 0.0,
                click_position: [0.5; 3],
            };
            for (offset, expected) in [
                (0.0, rotation),
                (11.249, rotation),
                (11.25, (rotation + 1) & 15),
            ] {
                let (name, states) = sign_states(
                    "minecraft:standing_sign",
                    canonical,
                    PlacementInput {
                        yaw: yaw + offset,
                        ..input
                    },
                )
                .unwrap();
                assert_eq!(name, "minecraft:standing_sign");
                assert_eq!(states["ground_sign_direction"]["value"], expected);
                assert_eq!(support_face(&name, &states), Some(0));
            }
        }
    }

    #[test]
    fn side_faces_choose_matching_wall_signs_and_ceiling_clicks_defer() {
        let canonical = r#"{"ground_sign_direction":{"type":"int","value":7}}"#;
        for face in 0..=6 {
            let input = PlacementInput {
                face,
                yaw: 0.0,
                pitch: 0.0,
                click_position: [0.5; 3],
            };
            let result = sign_states("minecraft:spruce_standing_sign", canonical, input);
            if face == 0 || face == 6 {
                assert!(result.is_none());
                continue;
            }
            let (name, states) = result.unwrap();
            if face == 1 {
                assert_eq!(name, "minecraft:spruce_standing_sign");
            } else {
                assert_eq!(name, "minecraft:spruce_wall_sign");
                assert_eq!(states["facing_direction"]["value"], face);
                assert_eq!(support_face(&name, &states), Some(face ^ 1));
            }
        }
    }
}
