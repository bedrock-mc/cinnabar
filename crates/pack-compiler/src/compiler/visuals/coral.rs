//! Inclined floor and wall coral fan sprites.

use super::super::*;
use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use assets::BlockFace;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::compiler) enum Shape {
    Floor,
    Wall(u8),
}

/// Identifies registered coral fans without accepting unrelated custom names.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    family(record).is_some()
}

/// Returns the visual family for primary coral records.
fn family(record: &RegistryRecord) -> Option<Shape> {
    if record.contributor_role != ContributorRole::Primary || record.flags.contains(BlockFlags::AIR)
    {
        return None;
    }
    let name = record.name.strip_prefix("minecraft:")?;
    let name = name.strip_prefix("dead_").unwrap_or(name);
    let (species, shape) = if let Some(species) = name.strip_suffix("_coral_wall_fan") {
        (species, Shape::Wall(0))
    } else if let Some(species) = name.strip_suffix("_coral_fan") {
        (species, Shape::Floor)
    } else {
        return None;
    };
    matches!(species, "tube" | "brain" | "bubble" | "fire" | "horn").then_some(shape)
}

/// Admits the typed direction selectors used by registered coral fans.
fn shape(record: &RegistryRecord) -> Option<Shape> {
    let family = family(record)?;
    let state: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&record.canonical_state).ok()?;
    match family {
        Shape::Floor if state.len() == 1 => {
            super::state::exact_tagged_int(state.get("coral_fan_direction")?, 1)?;
            Some(Shape::Floor)
        }
        Shape::Wall(_) if state.len() == 1 => Some(Shape::Wall(super::state::exact_tagged_int(
            state.get("coral_direction")?,
            3,
        )? as u8)),
        _ => None,
    }
}

/// Marks fan sprite materials as cutouts.
pub(in crate::compiler) fn is_fan(record: &RegistryRecord) -> bool {
    matches!(family(record), Some(Shape::Floor | Shape::Wall(_)))
}

/// Compiles fan templates using each record's resolved sprite material.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<(u32, Shape), u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_record(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let Some(shape) = shape(record) else {
        return Ok(CompileRuleResult::Reject);
    };
    let mut visual = diagnostic_visual(record);
    let Some(materials) = inputs.materials(record) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let material = materials[BlockFace::Up as usize];
    let key = (material, shape);
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let template =
            push_model_template(quads(material, shape), 0, storage.templates, storage.quads)?;
        templates.insert(key, template);
        template
    };
    set_model_visual(&mut visual, materials, template);
    Ok(CompileRuleResult::Compiled(visual))
}

/// Quantizes the inclined sprite positions into the existing carrier format.
fn quad(material: u32, positions: [[f32; 3]; 4], flip_u: bool) -> ModelQuad {
    ModelQuad {
        positions: positions.map(|p| p.map(|v| (v * 256.0).round() as i16)),
        uvs: if flip_u {
            [[4096, 0], [4096, 4096], [0, 4096], [0, 0]]
        } else {
            [[0, 0], [0, 4096], [4096, 4096], [4096, 0]]
        },
        material,
        flags: MODEL_QUAD_FLAG_TWO_SIDED,
    }
}

/// Emits four spreading floor sprites or one wall-attached sprite with its authored UV orientation.
fn quads(material: u32, shape: Shape) -> Vec<ModelQuad> {
    match shape {
        Shape::Floor => vec![
            quad(
                material,
                [
                    [0.0, 0.45, -0.3],
                    [0.0, 0.0, 0.5],
                    [1.0, 0.0, 0.5],
                    [1.0, 0.45, -0.3],
                ],
                true,
            ),
            quad(
                material,
                [
                    [0.0, 0.45, 1.3],
                    [0.0, 0.0, 0.5],
                    [1.0, 0.0, 0.5],
                    [1.0, 0.45, 1.3],
                ],
                false,
            ),
            quad(
                material,
                [
                    [-0.3, 0.35, 0.0],
                    [0.5, 0.0, 0.0],
                    [0.5, 0.0, 1.0],
                    [-0.3, 0.35, 1.0],
                ],
                false,
            ),
            quad(
                material,
                [
                    [1.3, 0.35, 0.0],
                    [0.5, 0.0, 0.0],
                    [0.5, 0.0, 1.0],
                    [1.3, 0.35, 1.0],
                ],
                true,
            ),
        ],
        Shape::Wall(direction) => {
            let positions = match direction {
                0 => [
                    [0.81, 0.65, -0.05],
                    [0.01, 0.5, -0.05],
                    [0.01, 0.5, 1.05],
                    [0.81, 0.65, 1.05],
                ],
                1 => [
                    [0.19, 0.65, -0.05],
                    [0.99, 0.5, -0.05],
                    [0.99, 0.5, 1.05],
                    [0.19, 0.65, 1.05],
                ],
                2 => [
                    [1.05, 0.65, 0.81],
                    [1.05, 0.5, 0.01],
                    [-0.05, 0.5, 0.01],
                    [-0.05, 0.65, 0.81],
                ],
                _ => [
                    [1.05, 0.65, 0.19],
                    [1.05, 0.5, 0.99],
                    [-0.05, 0.5, 0.99],
                    [-0.05, 0.65, 0.19],
                ],
            };
            vec![quad(material, positions, false)]
        }
    }
}
