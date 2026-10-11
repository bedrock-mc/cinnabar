//! Anvils share a four-piece silhouette and rotate about the vertical block axis.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use {super::super::*, assets::BlockFace};

const PIECES: [([i16; 3], [i16; 3]); 4] = [
    ([32, 0, 32], [224, 64, 224]),
    ([64, 64, 48], [192, 80, 208]),
    ([96, 80, 64], [160, 160, 192]),
    ([48, 160, 0], [208, 256, 256]),
];

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::compiler) struct TemplateKey {
    materials: [u32; 6],
    direction: u32,
}

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<TemplateKey, u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !matches!(
        record.name.as_ref(),
        "minecraft:anvil" | "minecraft:chipped_anvil" | "minecraft:damaged_anvil"
    ) || record.contributor_role != ContributorRole::Primary
        || record.flags.contains(BlockFlags::AIR)
    {
        return Ok(CompileRuleResult::NoMatch);
    }
    let mut visual = diagnostic_visual(record);
    let direction = canonical_state_str(&record.canonical_state, "minecraft:cardinal_direction")
        .and_then(|direction| match direction.as_ref() {
            "south" => Some(0),
            "west" => Some(1),
            "north" => Some(2),
            "east" => Some(3),
            _ => None,
        });
    let Some(direction) = direction else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let Some(materials) = inputs.materials(record) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let key = TemplateKey {
        materials,
        direction,
    };
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let quads = PIECES
            .into_iter()
            .enumerate()
            .flat_map(|(part, (mut min, mut max))| {
                if direction & 1 != 0 {
                    min.swap(0, 2);
                    max.swap(0, 2);
                }
                let part_materials = if part == 0 {
                    [materials[BlockFace::Down as usize]; 6]
                } else {
                    materials
                };
                anvil_cuboid(part_materials, min, max, direction)
            })
            .collect();
        let template = push_model_template(quads, 0, storage.templates, storage.quads)?;
        templates.insert(key, template);
        template
    };
    set_model_visual(&mut visual, materials, template);
    visual.support = VisualSupport::VanillaFallback;
    Ok(CompileRuleResult::Compiled(visual))
}

fn anvil_cuboid(
    materials: [u32; 6],
    min: [i16; 3],
    max: [i16; 3],
    direction: u32,
) -> [ModelQuad; 6] {
    // Face order is west, east, down, up, north, south.
    let rotations = match direction {
        0 => [1, 2, 3, 3, 0, 0],
        1 => [0, 0, 1, 2, 1, 2],
        2 => [2, 1, 0, 0, 0, 0],
        _ => [0, 0, 2, 1, 2, 1],
    };
    let mut quads = super::geometry::vanilla_cuboid_quads(materials, min, max);
    for (quad, rotation) in quads.iter_mut().zip(rotations) {
        for uv in &mut quad.uvs {
            let [u, v] = *uv;
            *uv = match rotation {
                1 => [v, 4096 - u],
                2 => [4096 - v, u],
                3 => [4096 - u, 4096 - v],
                _ => [u, v],
            };
        }
    }
    quads
}
