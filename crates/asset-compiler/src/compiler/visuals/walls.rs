use super::super::*;
use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<[u32; 7], u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_wall(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let mut visual = diagnostic_visual(record);
    if let Some(materials) = inputs.materials(record)
        && let Some(connections) = record
            .model_state
            .get(ModelStateField::Connections)
            .filter(|connections| wall_state_is_valid(*connections))
    {
        let [west, east, down, up, north, south] = materials;
        let key = [west, east, down, up, north, south, connections];
        let template = if let Some(&template) = templates.get(&key) {
            template
        } else {
            let template = push_model_template(
                wall_quads(materials, connections),
                MODEL_TEMPLATE_FLAG_WALL,
                storage.templates,
                storage.quads,
            )?;
            templates.insert(key, template);
            template
        };
        set_model_visual(&mut visual, materials, template);
    }
    Ok(CompileRuleResult::Compiled(visual))
}

pub(in crate::compiler) const fn wall_state_is_valid(connections: u32) -> bool {
    connections & !0x1ff == 0
        && connections & 3 <= 2
        && (connections >> 2) & 3 <= 2
        && (connections >> 4) & 3 <= 2
        && (connections >> 6) & 3 <= 2
}

pub(in crate::compiler) fn wall_quads(materials: [u32; 6], connections: u32) -> Vec<ModelQuad> {
    debug_assert!(wall_state_is_valid(connections));
    let north = connections & 3;
    let east = (connections >> 2) & 3;
    let south = (connections >> 4) & 3;
    let west = (connections >> 6) & 3;
    let post = (connections >> 8) & 1;
    let height = |connection| match connection {
        1 => 224,
        2 => 256,
        _ => unreachable!("wall connection is checked before geometry generation"),
    };
    let mut quads = Vec::with_capacity(30);
    // Visible extents come from the local vanilla
    // template_wall_{post,side,side_tall}.json render models. Dragonfly's
    // broader Wall::BBox components are collision-only and not render authority.
    if post != 0 {
        quads.extend(cuboid_quads(materials, [64, 0, 64], [192, 256, 192]));
    }
    if north != 0 {
        quads.extend(cuboid_quads(
            materials,
            [80, 0, 0],
            [176, height(north), 128],
        ));
    }
    if east != 0 {
        quads.extend(cuboid_quads(
            materials,
            [128, 0, 80],
            [256, height(east), 176],
        ));
    }
    if south != 0 {
        quads.extend(cuboid_quads(
            materials,
            [80, 0, 128],
            [176, height(south), 256],
        ));
    }
    if west != 0 {
        quads.extend(cuboid_quads(
            materials,
            [0, 0, 80],
            [128, height(west), 176],
        ));
    }
    quads
}
