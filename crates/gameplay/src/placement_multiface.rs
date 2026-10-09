//! Attachment masks and fallback order for vanilla multiface placement.

use serde_json::{Map, Value};

use crate::placement_state::{only_keys, set_value, value};

const FACE_BITS: [u8; 6] = [1, 2, 16, 4, 8, 32];
const OTHER_FACES: [[usize; 5]; 6] = [
    [1, 2, 3, 4, 5],
    [0, 2, 3, 4, 5],
    [1, 0, 3, 4, 5],
    [1, 0, 2, 4, 5],
    [1, 0, 2, 3, 5],
    [1, 0, 2, 3, 4],
];

/// Recognizes the vanilla blocks that use the six-face attachment mask.
pub(crate) fn is_multiface(identifier: &str) -> bool {
    matches!(identifier, "minecraft:glow_lichen" | "minecraft:sculk_vein")
}

/// Resolves a first attachment in air; unknown fallback supports defer placement.
pub(crate) fn attachment_states(
    canonical: &Map<String, Value>,
    clicked_face: u8,
    supports: [Option<bool>; 6],
) -> Option<Map<String, Value>> {
    if clicked_face > 5 || !only_keys(canonical, &["multi_face_direction_bits"]) {
        return None;
    }
    if value(canonical, "multi_face_direction_bits")?.as_u64()? > 63 {
        return None;
    }
    let opposite = usize::from(clicked_face ^ 1);
    let mut selected = None;
    for face in std::iter::once(opposite).chain(OTHER_FACES[opposite]) {
        if supports[face]? {
            selected = Some(face);
            break;
        }
    }
    let mut states = canonical.clone();
    set_value(
        &mut states,
        "multi_face_direction_bits",
        Value::from(FACE_BITS[selected?]),
    )?;
    Some(states)
}

/// Returns the support of a first attachment; existing masks require separate survival checks.
pub(crate) fn support_face(states: &Map<String, Value>) -> Option<u8> {
    let mask = value(states, "multi_face_direction_bits")?.as_u64()?;
    FACE_BITS
        .iter()
        .position(|bit| u64::from(*bit) == mask)
        .map(|face| face as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Supplies a typed held state whose mask must not leak into first placement.
    fn canonical() -> Map<String, Value> {
        serde_json::from_value(json!({"multi_face_direction_bits":{"type":"int","value":63}}))
            .unwrap()
    }

    #[test]
    fn clicked_face_and_fallback_use_the_attachment_mask_table() {
        for (clicked, expected) in [2, 1, 4, 16, 32, 8].into_iter().enumerate() {
            let states = attachment_states(&canonical(), clicked as u8, [Some(true); 6]).unwrap();
            assert_eq!(
                value(&states, "multi_face_direction_bits"),
                Some(&json!(expected))
            );
            assert_eq!(support_face(&states), Some(clicked as u8 ^ 1));
        }
        for (clicked, order) in [
            [1, 0, 2, 3, 4, 5],
            [0, 1, 2, 3, 4, 5],
            [3, 1, 0, 2, 4, 5],
            [2, 1, 0, 3, 4, 5],
            [5, 1, 0, 2, 3, 4],
            [4, 1, 0, 2, 3, 5],
        ]
        .into_iter()
        .enumerate()
        {
            for first in 0..6 {
                let mut supports = [Some(false); 6];
                for &face in &order[first..] {
                    supports[face] = Some(true);
                }
                let placed = attachment_states(&canonical(), clicked as u8, supports).unwrap();
                assert_eq!(
                    value(&placed, "multi_face_direction_bits"),
                    Some(&json!([1, 2, 16, 4, 8, 32][order[first]]))
                );
            }
        }
        let mut supports = [Some(false); 6];
        supports[0] = Some(true);
        let states = attachment_states(&canonical(), 3, supports).unwrap();
        assert_eq!(value(&states, "multi_face_direction_bits"), Some(&json!(1)));
        supports[1] = None;
        assert!(attachment_states(&canonical(), 3, supports).is_none());
        assert!(attachment_states(&canonical(), 6, [Some(true); 6]).is_none());
    }
}
