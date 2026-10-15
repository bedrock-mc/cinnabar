//! Sleep headings compiled once from admitted block states.

use assets::NetworkIdMode;
use serde_json::{Map, Value};

use super::PhysicsCollisionRegistries;

impl PhysicsCollisionRegistries {
    /// Resolves the compiled sleep heading in the requested runtime identity space.
    pub fn bed_rotation_degrees(&self, mode: NetworkIdMode, runtime: u32) -> Option<f32> {
        let blocks = match mode {
            NetworkIdMode::Sequential => &self.interaction_blocks,
            NetworkIdMode::Hashed => &self.hashed_interaction_blocks,
        };
        blocks.get(&runtime)?.bed_rotation
    }
}

/// Preserves tagged and plain integer directions, including their modulo-four wrapping.
pub(super) fn rotation(states: &Map<String, Value>) -> Option<f32> {
    let value = states.get("direction")?;
    let direction = value.get("value").unwrap_or(value).as_i64()?;
    Some((90 - direction.rem_euclid(4) * 90).rem_euclid(360) as f32)
}

#[cfg(test)]
mod tests {
    use super::rotation;
    use serde_json::json;

    #[test]
    fn directions_preserve_plain_tagged_wrapped_and_missing_states() {
        for (direction, expected) in [
            (0, 90.0),
            (1, 0.0),
            (2, 270.0),
            (3, 180.0),
            (-1, 180.0),
            (4, 90.0),
            (i64::MIN, 90.0),
            (i64::MAX, 180.0),
        ] {
            for state in [
                json!({"direction": direction}),
                json!({"direction": {"type": "int", "value": direction}}),
            ] {
                assert_eq!(rotation(state.as_object().unwrap()), Some(expected));
            }
        }
        for state in [
            json!({}),
            json!({"head_piece_bit": true}),
            json!({"direction": null}),
            json!({"direction": "north"}),
            json!({"direction": 1.5}),
            json!({"direction": {"value": false}}),
        ] {
            assert_eq!(rotation(state.as_object().unwrap()), None);
        }
    }
}
