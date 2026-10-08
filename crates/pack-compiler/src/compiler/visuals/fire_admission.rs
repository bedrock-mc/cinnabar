use super::super::*;
use super::state::{exact_tagged_byte, exact_tagged_string};

/// Compile independent top-support and catch-chance facts for fire geometry.
/// The cube support fallback remains provisional; complete admission is tracked in plan.md.
pub(in crate::compiler) fn apply(record: &RegistryRecord, visual: &mut BlockVisual) {
    visual
        .flags
        .remove(BlockFlags::FIRE_TOP_SUPPORT | BlockFlags::FIRE_FLAMMABLE);
    if matches!(visual.kind, VisualKind::Diagnostic | VisualKind::Invisible)
        || record.flags.contains(BlockFlags::AIR)
    {
        return;
    }
    visual.flags.set(
        BlockFlags::FIRE_FLAMMABLE,
        known_wool_flammable(record.name.as_ref()),
    );
    visual
        .flags
        .set(BlockFlags::FIRE_TOP_SUPPORT, top_support(record, visual));
}

fn top_support(record: &RegistryRecord, visual: &BlockVisual) -> bool {
    if record.model_family == ModelFamily::Leaves
        || record.flags.contains(BlockFlags::LEAF_MODEL)
        || record.name.as_ref() == "minecraft:powder_snow"
    {
        return false;
    }
    if is_slab(record) {
        // Double slabs support every face; single slabs support the top only when upper.
        // Half 2 is the registry double slab.
        return record.model_state.get(ModelStateField::Half) == Some(2)
            || canonical_upper_half(record, "top_slot_bit").unwrap_or(false);
    }
    if is_stair(record) {
        // Upper stairs support the top; legacy upside-down state takes precedence.
        return canonical_upper_half(record, "upside_down_bit").unwrap_or(false);
    }
    // Soul sand retains top support despite its lowered model.
    if matches!(
        record.name.as_ref(),
        "minecraft:stone" | "minecraft:netherrack" | "minecraft:glass" | "minecraft:soul_sand"
    ) {
        return true;
    }
    // Keep ordinary cube geometry as the explicitly provisional support map.
    // Full glass's model flags can differ from its input registry cube flags.
    visual.kind == VisualKind::Cube
        || (record.model_family == ModelFamily::Cube
            && record.flags.contains(BlockFlags::CUBE_GEOMETRY))
}

fn canonical_upper_half(record: &RegistryRecord, legacy_key: &str) -> Option<bool> {
    let state =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&record.canonical_state)
            .ok()?;
    if legacy_key == "upside_down_bit"
        && let Some(value) = state.get(legacy_key)
    {
        return exact_tagged_byte(value, 1).map(|value| value != 0);
    }
    if let Some(value) = state.get("minecraft:vertical_half") {
        return match exact_tagged_string(value)? {
            "bottom" => Some(false),
            "top" => Some(true),
            _ => None,
        };
    }
    exact_tagged_byte(state.get(legacy_key)?, 1).map(|value| value != 0)
}

fn known_wool_flammable(name: &str) -> bool {
    let Some(name) = name.strip_prefix("minecraft:") else {
        return false;
    };
    // Wool and its stair/slab variants have catch chance 30 and destroy chance 60.
    // Missing catch chance is not inferred from wood material or appearance.
    let color = ["_wool", "_wool_stairs", "_wool_slab", "_wool_double_slab"]
        .into_iter()
        .find_map(|suffix| name.strip_suffix(suffix));
    matches!(
        color,
        Some(
            "white"
                | "orange"
                | "magenta"
                | "light_blue"
                | "yellow"
                | "lime"
                | "pink"
                | "gray"
                | "light_gray"
                | "cyan"
                | "purple"
                | "blue"
                | "brown"
                | "green"
                | "red"
                | "black"
        )
    )
}

#[cfg(test)]
#[path = "fire_admission_tests.rs"]
mod tests;
