//! Shared admission and charge selection for respawn-anchor block art.

use assets::{BlockFlags, ContributorRole, RegistryRecord};
use serde::Deserialize;

/// Identifies terrain-owned respawn anchors before provisional fallback selection.
pub(crate) fn is_record(record: &RegistryRecord) -> bool {
    record.name.as_ref() == "minecraft:respawn_anchor"
        && record.contributor_role == ContributorRole::Primary
        && !record.flags.contains(BlockFlags::AIR)
}

/// Accepts only the complete integer charge state in the supported zero-to-four range.
pub(crate) fn charge(record: &RegistryRecord) -> Option<usize> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Tagged {
        #[serde(rename = "type")]
        kind: String,
        value: u32,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct State {
        respawn_anchor_charge: Tagged,
    }
    let state: State = serde_json::from_str(&record.canonical_state).ok()?;
    (state.respawn_anchor_charge.kind == "int" && state.respawn_anchor_charge.value <= 4)
        .then_some(state.respawn_anchor_charge.value as usize)
}
