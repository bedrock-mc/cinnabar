//! Shared lightning-rod admission for model and terrain-array selection.
use assets::{BlockFlags, ContributorRole, RegistryRecord};
use serde::Deserialize;

/// Identifies the eight terrain-owned lightning-rod variants.
pub(crate) fn is_record(record: &RegistryRecord) -> bool {
    matches!(
        record.name.as_ref(),
        "minecraft:lightning_rod"
            | "minecraft:exposed_lightning_rod"
            | "minecraft:weathered_lightning_rod"
            | "minecraft:oxidized_lightning_rod"
            | "minecraft:waxed_lightning_rod"
            | "minecraft:waxed_exposed_lightning_rod"
            | "minecraft:waxed_weathered_lightning_rod"
            | "minecraft:waxed_oxidized_lightning_rod"
    ) && record.contributor_role == ContributorRole::Primary
        && !record.flags.contains(BlockFlags::AIR)
}

/// Accepts the complete six-facing, two-powered-state selector product.
pub(crate) fn state(record: &RegistryRecord) -> Option<(usize, usize)> {
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
        facing_direction: Tagged,
        powered_bit: Tagged,
    }
    let state: State = serde_json::from_str(&record.canonical_state).ok()?;
    (state.facing_direction.kind == "int"
        && state.facing_direction.value <= 5
        && state.powered_bit.kind == "byte"
        && state.powered_bit.value <= 1)
        .then_some((
            state.facing_direction.value as usize,
            state.powered_bit.value as usize,
        ))
}
