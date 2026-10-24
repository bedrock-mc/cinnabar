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
use assets::facts::{BlockRenderKind as R, block_render_fact};

/// Enchanting-table base height in 1/256 block; the book floats above it as an entity.
const ENCHANTING_TABLE_HEIGHT: i16 = 12 * 16;

/// Whether the block entity renderer draws this whole block, with no terrain part.
pub(in crate::compiler) fn is_entity_drawn_name(name: &str) -> bool {
    block_render_fact(name).kind == R::EntityDrawn
}

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<CuboidTemplateKey, u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if block_render_fact(&record.name).kind == R::EnchantingTable {
        let Some(materials) = inputs.materials(record) else {
            return Ok(CompileRuleResult::NoMatch);
        };
        let template = intern_cuboid_template(
            materials,
            [0, 0, 0],
            [256, ENCHANTING_TABLE_HEIGHT, 256],
            0,
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
    // Particle routing still needs the pack's face materials when terrain draws no geometry.
    visual.faces = assets::BlockFace::ALL.map(|face| {
        inputs
            .material(record, face)
            .unwrap_or(assets::DIAGNOSTIC_MATERIAL)
    });
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
        let records = assets::read_registry_for_protocol(
            assets::pinned_block_registry_bytes(),
            assets::active_content_registry_protocol(),
        )
        .unwrap();
        for name in [
            "minecraft:chest",
            "minecraft:trapped_chest",
            "minecraft:ender_chest",
            "minecraft:waxed_exposed_copper_chest",
            "minecraft:bed",
            "minecraft:chalkboard",
            "minecraft:standing_banner",
            "minecraft:wall_banner",
            "minecraft:skeleton_skull",
            "minecraft:wither_skeleton_skull",
            "minecraft:zombie_head",
            "minecraft:player_head",
            "minecraft:creeper_head",
            "minecraft:dragon_head",
            "minecraft:piglin_head",
            "minecraft:undyed_shulker_box",
            "minecraft:light_gray_shulker_box",
            "minecraft:bell",
            assets::END_PORTAL_IDENTIFIER,
            assets::END_GATEWAY_IDENTIFIER,
        ] {
            assert!(
                records.iter().any(|record| record.name.as_ref() == name),
                "unregistered fixture {name}"
            );
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
