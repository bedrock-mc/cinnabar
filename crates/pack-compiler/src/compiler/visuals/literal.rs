//! Stateless blocks the pack defines literally: full cubes whose faces come from the block
//! texture map, and blocks vanilla draws nothing for.
//!
//! Anything that needs geometry the pack does not carry (hopper, brewing stand, campfire,
//! candle, cauldron, end rod, ...) stays diagnostic until measured.

use super::super::*;
use super::context::{RuleInputs, diagnostic_visual};
use super::dispatcher::CompileRuleResult;

/// Whether the state is a plain full cube whose six faces the terrain texture map names.
pub(in crate::compiler) fn is_literal_cube(record: &RegistryRecord) -> bool {
    record.canonical_state.as_ref() == "{}"
        && matches!(
            record.name.as_ref(),
            "minecraft:redstone_lamp"
                | "minecraft:lit_redstone_lamp"
                | "minecraft:glowingobsidian"
                | "minecraft:lodestone"
                | "minecraft:cartography_table"
                | "minecraft:target"
                | "minecraft:netherreactor"
                | "minecraft:moss_block"
                | "minecraft:pale_moss_block"
                | "minecraft:mud"
                | "minecraft:soul_sand"
                | "minecraft:sculk"
                | "minecraft:dirt_with_roots"
                | "minecraft:budding_amethyst"
                | "minecraft:crimson_nylium"
                | "minecraft:warped_nylium"
                | "minecraft:allow"
                | "minecraft:deny"
        )
}

/// Whether vanilla terrain draws nothing for this block in normal play.
///
/// Invisible bedrock and moving blocks use the never-tessellated block shape; barriers, light
/// blocks and structure voids tessellate only into terrain layers drawn while a creative local
/// player holds that block. Neither kind is full, so neither culls a neighbour's face.
pub(in crate::compiler) fn is_default_invisible(name: &str) -> bool {
    matches!(
        name,
        "minecraft:barrier"
            | "minecraft:structure_void"
            | "minecraft:invisible_bedrock"
            | "minecraft:moving_block"
    ) || name
        .strip_prefix("minecraft:light_block_")
        .is_some_and(|level| level.parse::<u8>().is_ok_and(|level| level <= 15))
}

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
) -> CompileRuleResult {
    if is_default_invisible(&record.name) {
        let mut visual = diagnostic_visual(record);
        visual.flags.remove(
            BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE | BlockFlags::LEAF_MODEL,
        );
        visual.kind = VisualKind::Invisible;
        visual.support = VisualSupport::Exact;
        return CompileRuleResult::Compiled(visual);
    }
    if !is_literal_cube(record) {
        return CompileRuleResult::NoMatch;
    }
    let Some(materials) = inputs.materials(record) else {
        return CompileRuleResult::NoMatch;
    };
    let mut visual = diagnostic_visual(record);
    visual.flags |= BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE;
    visual.faces = materials;
    visual.kind = VisualKind::Cube;
    CompileRuleResult::Compiled(visual)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invisible_names_cover_light_levels_zero_through_fifteen_only() {
        assert!(is_default_invisible("minecraft:barrier"));
        assert!(is_default_invisible("minecraft:light_block_0"));
        assert!(is_default_invisible("minecraft:light_block_15"));
        assert!(!is_default_invisible("minecraft:light_block_16"));
        assert!(!is_default_invisible("minecraft:light_block_x"));
        assert!(!is_default_invisible("minecraft:glass"));
    }
}
