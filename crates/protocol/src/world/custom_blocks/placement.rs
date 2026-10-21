//! Placement trait parameters and server-owned placing behavior from block definitions.

use super::{CustomPlacement, Nbt};

/// Reads the yaw offset once, applying the trait's quarter-turn rounding.
pub(super) fn traits<'a>(
    traits: impl IntoIterator<Item = &'a Nbt>,
    trait_state_count: usize,
) -> CustomPlacement {
    let offset = traits
        .into_iter()
        .find_map(|trait_| trait_.field("y_rotation_offset").and_then(Nbt::number))
        .unwrap_or(0.0) as f32;
    CustomPlacement {
        trait_state_count,
        y_rotation_offset: if offset.is_finite() {
            if (offset % 90.0).abs() < 0.5 {
                offset
            } else {
                (offset / 90.0 + 0.5).floor().rem_euclid(4.0) * 90.0
            }
        } else {
            0.0
        },
        ..Default::default()
    }
}

#[cfg(test)]
mod tests;

/// Prediction overrides apply to the block type when any component set needs them.
pub(super) fn components(placement: &mut CustomPlacement, components: Option<&Nbt>) {
    let Some(components) = components else { return };
    let custom = components.field("minecraft:custom_components");
    // Version 1 custom components declare every handler.
    let flag = |name: &str| {
        matches!(
            custom.and_then(|component| component.field(name)),
            Some(Nbt::Byte(value)) if *value != 0
        )
    };
    placement.server_placing |= components.field("minecraft:on_player_placing").is_some()
        || flag("hasPlayerPlacing")
        || flag("isV1Component");
    placement.has_placement_filter |= components.field("minecraft:placement_filter").is_some();
}
