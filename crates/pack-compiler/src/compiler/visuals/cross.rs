use super::super::*;
use super::context::{ModelStorage, RuleInputs, diagnostic_visual, push_model_template};
use super::dispatcher::CompileRuleResult;

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<([u32; 2], bool), u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_cross_visual(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let mut visual = diagnostic_visual(record);
    let faces = if is_aquatic_cross(record) {
        aquatic_cross_faces(record)
    } else {
        Some([cross_texture_face(record); 2])
    };
    if let Some(faces) = faces
        && let (Some(material_a), Some(material_b), Some(variant)) = (
            inputs.material(record, faces[0]),
            inputs.material(record, faces[1]),
            model_variant(inputs.pack, record, faces[0]),
        )
    {
        let materials = [material_a, material_b];
        let wheat_rows = record.name.as_ref() == "minecraft:wheat";
        let key = (materials, wheat_rows);
        let template = if let Some(&template) = templates.get(&key) {
            template
        } else {
            let quads = if wheat_rows {
                wheat_row_quads(material_a).to_vec()
            } else {
                crossed_quads(materials).to_vec()
            };
            let template = push_model_template(quads, 0, storage.templates, storage.quads)?;
            templates.insert(key, template);
            template
        };
        visual.faces = [material_a; 6];
        visual.kind = VisualKind::Cross;
        visual.model_template = template;
        visual.variant = variant;
    }
    Ok(CompileRuleResult::Compiled(visual))
}

/// Native row tessellation (MCSRC 06a21df0 / 06a98c10): four full-width
/// planes at X/Z 1/4 and 3/4, lowered 1/16 to meet farmland's surface.
fn wheat_row_quads(material: u32) -> [ModelQuad; 4] {
    let uvs = [[0, 0], [0, 4096], [4096, 4096], [4096, 0]];
    [
        [[64, 240, 0], [64, -16, 0], [64, -16, 256], [64, 240, 256]],
        [
            [192, 240, 256],
            [192, -16, 256],
            [192, -16, 0],
            [192, 240, 0],
        ],
        [[0, 240, 64], [0, -16, 64], [256, -16, 64], [256, 240, 64]],
        [
            [256, 240, 192],
            [256, -16, 192],
            [0, -16, 192],
            [0, 240, 192],
        ],
    ]
    .map(|positions| ModelQuad {
        positions,
        uvs,
        material,
        flags: MODEL_QUAD_FLAG_TWO_SIDED,
    })
}

pub(in crate::compiler) fn model_variant(
    pack: &PackSources,
    record: &RegistryRecord,
    face: BlockFace,
) -> Option<u32> {
    let TextureKey { key, .. } = resolve_texture_key(&pack.blocks, record, face);
    let key = key?;
    pack.terrain
        .get_for_model_record(&key, record)
        .map(|(_, variant)| variant)
}

pub(in crate::compiler) fn crossed_quads(materials: [u32; 2]) -> [ModelQuad; 2] {
    let uvs = [[0, 4096], [4096, 4096], [4096, 0], [0, 0]];
    [
        ModelQuad {
            positions: [[0, 0, 0], [256, 0, 256], [256, 256, 256], [0, 256, 0]],
            uvs,
            material: materials[0],
            flags: MODEL_QUAD_FLAG_TWO_SIDED,
        },
        ModelQuad {
            positions: [[256, 0, 0], [0, 0, 256], [0, 256, 256], [256, 256, 0]],
            uvs,
            material: materials[1],
            flags: MODEL_QUAD_FLAG_TWO_SIDED,
        },
    ]
}
