use super::super::*;
use super::context::{
    ModelStorage, PressurePlateTemplateKey, RuleInputs, diagnostic_visual, push_model_template,
    set_model_visual,
};
use super::dispatcher::CompileRuleResult;

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<PressurePlateTemplateKey, u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_pressure_plate(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    const PRESSED: u32 = 1 << 1;
    let mut visual = diagnostic_visual(record);
    if let Some(materials) = inputs.materials(record)
        && let Some(flags @ (0 | PRESSED)) = record.model_state.get(ModelStateField::Flags)
    {
        let key = PressurePlateTemplateKey {
            materials,
            pressed: flags == PRESSED,
        };
        let template = if let Some(&template) = templates.get(&key) {
            template
        } else {
            let template = push_model_template(
                pressure_plate_quads(materials, key.pressed).to_vec(),
                0,
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

pub(in crate::compiler) fn pressure_plate_quads(
    materials: [u32; 6],
    pressed: bool,
) -> [ModelQuad; 6] {
    // Visible geometry and UVs come from the local vanilla
    // pressure_plate_{up,down}.json models. The pressed side strip is
    // 15..15.5 pixels rather than the generic cuboid's 15.5..16 strip.
    let max_y = if pressed { 8 } else { 16 };
    let mut quads = cuboid_quads(materials, [16, 0, 16], [240, max_y, 240]);
    if pressed {
        for face in [
            BlockFace::West,
            BlockFace::East,
            BlockFace::North,
            BlockFace::South,
        ] {
            for uv in &mut quads[face as usize].uvs {
                uv[1] -= 128;
            }
        }
    }
    quads
}
