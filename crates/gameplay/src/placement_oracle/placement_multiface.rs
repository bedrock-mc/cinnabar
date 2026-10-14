//! Retained oracle: Attachment masks and fallback order for vanilla multiface placement.

use serde_json::{Map, Value};

use crate::placement_oracle::placement_state::{only_keys, set_value, value};

const FACE_BITS: [u8; 6] = [1, 2, 16, 4, 8, 32];
const OTHER_FACES: [[usize; 5]; 6] = [
    [1, 2, 3, 4, 5],
    [0, 2, 3, 4, 5],
    [1, 0, 3, 4, 5],
    [1, 0, 2, 4, 5],
    [1, 0, 2, 3, 5],
    [1, 0, 2, 3, 4],
];

/// Recognizes states that declare the six-face attachment mask.
pub(crate) fn is_multiface(states: &Map<String, Value>) -> bool {
    states.contains_key("multi_face_direction_bits")
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
