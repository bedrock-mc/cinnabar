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
    fn current_redstone_lamps_compile_as_exact_opaque_cubes() {
        let target: serde_json::Value =
            serde_json::from_slice(include_bytes!("../../../../../assets/bedrock-target.json"))
                .unwrap();
        let protocol = u32::try_from(target["wire_protocol"].as_u64().unwrap()).unwrap();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let registry = assets::read_registry_for_protocol(
            &std::fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap()))
                .unwrap(),
            protocol,
        )
        .unwrap();
        let records = registry
            .into_iter()
            .filter(|record| {
                matches!(record.name.as_ref(), "minecraft:air" | "minecraft:stone")
                    || is_literal_cube(record)
                        && matches!(
                            record.name.as_ref(),
                            "minecraft:redstone_lamp" | "minecraft:lit_redstone_lamp"
                        )
            })
            .collect::<Vec<_>>();
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(directory.path().join("textures/blocks")).unwrap();
        std::fs::write(
            directory.path().join("blocks.json"),
            r#"{"stone":{"textures":"stone"},"redstone_lamp":{"textures":"lamp_off"},"lit_redstone_lamp":{"textures":"lamp_on"}}"#,
        )
        .unwrap();
        std::fs::write(
            directory.path().join("textures/terrain_texture.json"),
            r#"{"texture_data":{"stone":{"textures":"textures/blocks/stone"},"lamp_off":{"textures":"textures/blocks/lamp_off"},"lamp_on":{"textures":"textures/blocks/lamp_on"}}}"#,
        )
        .unwrap();
        std::fs::write(
            directory.path().join("textures/flipbook_textures.json"),
            "[]",
        )
        .unwrap();
        for (name, color) in [
            ("stone", [100, 100, 100, 255]),
            ("lamp_off", [80, 40, 20, 255]),
            ("lamp_on", [200, 160, 80, 255]),
        ] {
            image::RgbaImage::from_pixel(assets::TILE_SIZE, assets::TILE_SIZE, image::Rgba(color))
                .save(directory.path().join(format!("textures/blocks/{name}.png")))
                .unwrap();
        }
        let lights = vec![
            assets::LightProperties::default();
            records
                .iter()
                .map(|record| record.sequential_id as usize + 1)
                .max()
                .unwrap()
        ];
        let (compiled, _) = compile_pack_inner(
            directory.path(),
            &records,
            &lights,
            CompiledBiomeAssets::diagnostic(),
            protocol,
        )
        .unwrap();
        let lamps = records
            .iter()
            .filter(|record| record.name.ends_with("redstone_lamp"));
        assert_eq!(lamps.clone().count(), 2);
        for record in lamps {
            let visual = compiled.visuals[record.sequential_id as usize];
            assert_eq!(visual.kind, VisualKind::Cube, "{}", record.name);
            assert_eq!(visual.support, VisualSupport::Exact, "{}", record.name);
            assert!(
                visual
                    .flags
                    .contains(BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE)
            );
            assert_eq!(visual.model_template, assets::NO_MODEL_TEMPLATE);
            for material in visual.faces {
                assert_ne!(material, DIAGNOSTIC_MATERIAL);
                assert_eq!(
                    compiled.materials[material as usize].flags, 0,
                    "{}",
                    record.name
                );
            }
        }
    }

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
