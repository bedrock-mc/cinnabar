use super::super::*;
use super::state::{exact_tagged_byte, exact_tagged_string};
use assets::facts::BlockProperty;

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
        assets::facts::block_render_fact(&record.name).fire_flammable,
    );
    visual
        .flags
        .set(BlockFlags::FIRE_TOP_SUPPORT, top_support(record, visual));
}

/// Keeps the admitted top-support flags independent of the chosen mesh.
fn top_support(record: &RegistryRecord, visual: &BlockVisual) -> bool {
    if record.model_family == ModelFamily::Leaves
        || record.flags.contains(BlockFlags::LEAF_MODEL)
        || assets::facts::block_render_fact(&record.name).kind
            == assets::facts::BlockRenderKind::PowderSnow
    {
        return false;
    }
    if is_slab(record) {
        // Double slabs support every face; single slabs support the top only when upper.
        // Half 2 is the registry double slab.
        return record.model_state.get(ModelStateField::Half) == Some(2)
            || canonical_upper_half(record, BlockProperty::TopSlotBit).unwrap_or(false);
    }
    if is_stair(record) {
        // Upper stairs support the top; legacy upside-down state takes precedence.
        return canonical_upper_half(record, BlockProperty::UpsideDownBit).unwrap_or(false);
    }
    // Soul sand retains top support despite its lowered model.
    if assets::facts::block_render_fact(&record.name).fire_top {
        return true;
    }
    // Keep ordinary cube geometry as the explicitly provisional support map.
    // Full glass's model flags can differ from its input registry cube flags.
    visual.kind == VisualKind::Cube
        || (record.model_family == ModelFamily::Cube
            && record.flags.contains(BlockFlags::CUBE_GEOMETRY))
}

/// Reads a legacy half selector, preferring its registered vertical-half override.
fn canonical_upper_half(record: &RegistryRecord, property: BlockProperty) -> Option<bool> {
    let state =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&record.canonical_state)
            .ok()?;
    if property == BlockProperty::UpsideDownBit
        && let Some(value) = state.get(property.name())
    {
        return exact_tagged_byte(value, 1).map(|value| value != 0);
    }
    if let Some(value) = state.get(BlockProperty::MinecraftVerticalHalf.name()) {
        return match exact_tagged_string(value)? {
            "bottom" => Some(false),
            "top" => Some(true),
            _ => None,
        };
    }
    exact_tagged_byte(state.get(property.name())?, 1).map(|value| value != 0)
}

#[cfg(test)]
#[path = "fire_admission_tests.rs"]
mod tests;
