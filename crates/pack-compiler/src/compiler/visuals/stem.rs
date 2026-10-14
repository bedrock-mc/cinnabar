//! Growing stems retain their upright sprite when a fruit attachment appears.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use super::state::exact_tagged_int;
use assets::facts::{BlockRenderKind as R, block_render_fact};
use {super::super::*, assets::BlockFace};

pub(in crate::compiler) type TemplateKey = ([u32; 2], i16, u8);

/// Identifies the two terrain-owned fruit stems.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    block_render_fact(&record.name).kind == R::Stem
}

/// Reads the complete bounded stem state without accepting extra selectors.
fn state(record: &RegistryRecord) -> Option<(u32, u8)> {
    let state: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&record.canonical_state).ok()?;
    if state.len() != 2 {
        return None;
    }
    Some((
        exact_tagged_int(state.get("growth")?, 7)?,
        exact_tagged_int(state.get("facing_direction")?, 5)? as u8,
    ))
}

/// Resolves disconnected body art separately from the attached connector art.
pub(in crate::compiler) fn descriptor(
    pack: &PackSources,
    record: &RegistryRecord,
    face: BlockFace,
) -> Option<(Descriptor, Box<str>)> {
    let (growth, facing) = state(record)?;
    let key = resolve_texture_key(&pack.blocks, record, face).key?;
    let variant = usize::from(face != BlockFace::Down && facing != 0);
    let (path, state_variant) = pack.terrain.get_clamped_untinted_variant(&key, variant)?;
    Some((
        Descriptor {
            path: path.into(),
            texture_key: key.clone(),
            state_variant,
            flipbook_selector: None,
            flags: MATERIAL_FLAG_ALPHA_CUTOUT
                | assets::MATERIAL_FLAG_DISABLE_AO
                | assets::MATERIAL_FLAG_DISABLE_FACE_DIMMING
                | assets::stem::material_flags(growth)?,
        },
        key,
    ))
}

/// Compiles the growth-height body and its state-oriented fruit connector.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<TemplateKey, u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_record(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let Some((growth, facing)) = state(record) else {
        return Ok(CompileRuleResult::Reject);
    };
    let body = inputs.material(record, BlockFace::Down);
    let connector = if facing == 0 {
        body
    } else {
        inputs.material(record, BlockFace::North)
    };
    let (Some(body), Some(connector)) = (body, connector) else {
        return Ok(CompileRuleResult::Reject);
    };
    let height = if facing == 0 {
        (growth + 1) as i16 * 32
    } else {
        128
    };
    let key = ([body, connector], height, facing);
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let mut quads =
            super::cross::plant_quads(body, height, -16, super::cross::PLANT_INSET).to_vec();
        if facing != 0 {
            quads.push(connector_quad(connector, facing));
        }
        let template = push_model_template(quads, 0, storage.templates, storage.quads)?;
        templates.insert(key, template);
        template
    };
    let mut visual = diagnostic_visual(record);
    set_model_visual(&mut visual, [body; 6], template);
    // Keep crossed-plant admission when snow occupies another storage layer.
    visual.kind = VisualKind::Cross;
    Ok(CompileRuleResult::Compiled(visual))
}

/// Emits the full sprite plane with the endpoint selected by the attached facing.
fn connector_quad(material: u32, facing: u8) -> ModelQuad {
    let positions = if matches!(facing, 4 | 5) {
        [
            [0, 240, 128],
            [0, -16, 128],
            [256, -16, 128],
            [256, 240, 128],
        ]
    } else {
        [
            [128, 240, 256],
            [128, -16, 256],
            [128, -16, 0],
            [128, 240, 0],
        ]
    };
    let u = if matches!(facing, 2 | 5) { 4096 } else { 0 };
    ModelQuad {
        positions,
        uvs: [[u, 0], [u, 4096], [4096 - u, 4096], [4096 - u, 0]],
        material,
        flags: MODEL_QUAD_FLAG_TWO_SIDED,
    }
}
