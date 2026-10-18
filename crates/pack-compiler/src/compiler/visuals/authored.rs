//! State-resolved vanilla component geometry projected through the shared block model parser.
use super::super::*;
use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use crate::block_geometry::BlockGeometryCatalog;
use assets::facts::{BlockRenderKind, block_render_fact};

/// Compiles admitted pack geometry with its authored face UVs and quarter-turn transform.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<(u32, Box<str>, u8), u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if block_render_fact(&record.name).kind != BlockRenderKind::AuthoredModel {
        return Ok(CompileRuleResult::NoMatch);
    }
    let Some(model) = inputs.pack.blocks.component_model(record) else {
        return Ok(CompileRuleResult::Reject);
    };
    let Some(material) = inputs.material(record, BlockFace::Up) else {
        return Ok(CompileRuleResult::Reject);
    };
    let key = (material, model.identifier.clone(), model.rotation);
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let mut catalog = BlockGeometryCatalog::default();
        catalog.insert(&model.document);
        let models = catalog.compile(&std::collections::HashSet::from([model
            .identifier
            .as_ref()]));
        let Some(geometry) = models.get(model.identifier.as_ref()) else {
            return Ok(CompileRuleResult::Reject);
        };
        if geometry.skipped_cubes != 0 {
            return Ok(CompileRuleResult::Reject);
        }
        let mut quads = Vec::new();
        for cube in &geometry.cubes {
            for (face, instance) in cube.quads() {
                if instance.is_some_and(|instance| instance != model.material_instance.as_ref()) {
                    return Ok(CompileRuleResult::Reject);
                }
                let mut positions = [[0i16; 3]; 4];
                let mut uvs = [[0u16; 2]; 4];
                for (target, point) in positions.iter_mut().zip(face.positions) {
                    let mut point = point;
                    for _ in 0..model.rotation {
                        point = [point[2], point[1], 16.0 - point[0]];
                    }
                    for axis in 0..3 {
                        let coordinate = (point[axis] * 16.0).round();
                        if !coordinate.is_finite()
                            || !(i16::MIN as f32..=i16::MAX as f32).contains(&coordinate)
                        {
                            return Ok(CompileRuleResult::Reject);
                        }
                        target[axis] = coordinate as i16;
                    }
                }
                for (target, source) in uvs.iter_mut().zip(face.uvs) {
                    for axis in 0..2 {
                        let value = (source[axis] / geometry.texture_size[axis] * 4096.0).round();
                        if !value.is_finite() || !(0.0..=4096.0).contains(&value) {
                            return Ok(CompileRuleResult::Reject);
                        }
                        target[axis] = value as u16;
                    }
                }
                let mut direction = face.face;
                for _ in 0..model.rotation {
                    direction = [5, 4, 2, 3, 0, 1][direction];
                }
                let flags = if face.rotated {
                    0
                } else {
                    [3, 4, 1, 2, 5, 6][direction]
                };
                quads.push(ModelQuad {
                    positions,
                    uvs,
                    material,
                    flags,
                });
            }
        }
        if quads.is_empty() || quads.len() > assets::MAX_MODEL_TEMPLATE_QUADS {
            return Ok(CompileRuleResult::Reject);
        }
        let template = push_model_template(quads, 0, storage.templates, storage.quads)?;
        templates.insert(key, template);
        template
    };
    let mut visual = diagnostic_visual(record);
    set_model_visual(&mut visual, [material; 6], template);
    Ok(CompileRuleResult::Compiled(visual))
}
