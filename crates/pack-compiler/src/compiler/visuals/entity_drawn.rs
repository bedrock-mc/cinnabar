//! Blocks whose visible model belongs to the block-entity renderer.
//!
//! Terrain draws nothing for these states (or only the static part vanilla keeps in
//! terrain), so the entity model is never hidden behind a placeholder cube.

use super::super::*;
use super::context::{
    CuboidTemplateKey, ModelStorage, RuleInputs, diagnostic_visual, intern_cuboid_template,
    set_model_visual,
};
use super::dispatcher::CompileRuleResult;

/// Enchanting-table base height in 1/256 block; the book floats above it as an entity.
const ENCHANTING_TABLE_HEIGHT: i16 = 12 * 16;

/// Whether the block entity renderer draws this whole block, with no terrain part.
pub(in crate::compiler) fn is_entity_drawn_name(name: &str) -> bool {
    let Some(name) = name.strip_prefix("minecraft:") else {
        return false;
    };
    let name = name.strip_prefix("waxed_").unwrap_or(name);
    matches!(
        name,
        "chest"
            | "trapped_chest"
            | "ender_chest"
            | "bed"
            | "standing_banner"
            | "wall_banner"
            | "skull"
            | "decorated_pot"
            | "conduit"
            | "bell"
            | "frame"
            | "glow_frame"
            | "lectern"
            | "copper_chest"
            | "exposed_copper_chest"
            | "weathered_copper_chest"
            | "oxidized_copper_chest"
    ) || name.ends_with("_shulker_box")
        || name.ends_with("copper_golem_statue")
}

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<CuboidTemplateKey, u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if record.name.as_ref() == "minecraft:enchanting_table" {
        let Some(materials) = inputs.materials(record) else {
            return Ok(CompileRuleResult::NoMatch);
        };
        let template = intern_cuboid_template(
            materials,
            [0, 0, 0],
            [256, ENCHANTING_TABLE_HEIGHT, 256],
            templates,
            storage.templates,
            storage.quads,
        )?;
        let mut visual = diagnostic_visual(record);
        set_model_visual(&mut visual, materials, template);
        return Ok(CompileRuleResult::Compiled(visual));
    }
    if !is_entity_drawn_name(&record.name) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let mut visual = diagnostic_visual(record);
    visual.flags.remove(
        BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE | BlockFlags::LEAF_MODEL,
    );
    visual.kind = VisualKind::Invisible;
    visual.support = VisualSupport::Exact;
    Ok(CompileRuleResult::Compiled(visual))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_drawn_names_cover_the_block_entity_families_only() {
        for name in [
            "minecraft:chest",
            "minecraft:trapped_chest",
            "minecraft:ender_chest",
            "minecraft:waxed_exposed_copper_chest",
            "minecraft:bed",
            "minecraft:standing_banner",
            "minecraft:wall_banner",
            "minecraft:skull",
            "minecraft:undyed_shulker_box",
            "minecraft:silver_shulker_box",
            "minecraft:bell",
        ] {
            assert!(is_entity_drawn_name(name), "{name}");
        }
        for name in [
            "minecraft:furnace",
            "minecraft:stone",
            "minecraft:barrel",
            "custom:chest",
        ] {
            assert!(!is_entity_drawn_name(name), "{name}");
        }
    }
}
