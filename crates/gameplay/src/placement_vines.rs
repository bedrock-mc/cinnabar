//! First vine attachments on a verified solid supporting cell.

use serde_json::{Map, Value};

use crate::placement_state::{only_keys, set_value, value};

const CLICK_BITS: [u8; 4] = [1, 4, 8, 2];

/// Resolves a horizontal first attachment; ceilings and existing vine masks defer.
pub(crate) fn attachment_states(
    canonical: &Map<String, Value>,
    face: u8,
    supports: [Option<bool>; 6],
) -> Option<Map<String, Value>> {
    if !(2..=5).contains(&face)
        || !only_keys(canonical, &["vine_direction_bits"])
        || value(canonical, "vine_direction_bits")?.as_u64()? > 15
        || !supports[usize::from(face ^ 1)]?
    {
        return None;
    }
    let mut states = canonical.clone();
    set_value(
        &mut states,
        "vine_direction_bits",
        Value::from(CLICK_BITS[usize::from(face - 2)]),
    )?;
    Some(states)
}

/// Returns the support required by a single first-attachment mask.
pub(crate) fn support_face(states: &Map<String, Value>) -> Option<u8> {
    let mask = value(states, "vine_direction_bits")?.as_u64()?;
    CLICK_BITS
        .iter()
        .position(|bit| u64::from(*bit) == mask)
        .map(|index| (index as u8 + 2) ^ 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn horizontal_attachments_reset_the_held_mask_and_require_the_clicked_support() {
        let original =
            serde_json::from_value(json!({"vine_direction_bits":{"type":"int","value":15}}))
                .unwrap();
        for (face, expected) in [(2, 1), (3, 4), (4, 8), (5, 2)] {
            let placed = attachment_states(&original, face, [Some(true); 6]).unwrap();
            assert_eq!(
                value(&placed, "vine_direction_bits"),
                Some(&json!(expected))
            );
            assert_eq!(support_face(&placed), Some(face ^ 1));
            let mut supports = [Some(true); 6];
            supports[usize::from(face ^ 1)] = Some(false);
            assert!(attachment_states(&original, face, supports).is_none());
        }
        for face in [0, 1, 6] {
            assert!(attachment_states(&original, face, [Some(true); 6]).is_none());
        }
    }
}
