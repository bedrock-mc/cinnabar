//! Retained oracle: First vine attachments on a verified solid supporting cell.

use serde_json::{Map, Value};

use crate::placement_oracle::placement_state::{only_keys, set_value, value};

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
