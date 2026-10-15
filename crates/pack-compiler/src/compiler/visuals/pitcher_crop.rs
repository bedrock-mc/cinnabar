//! Pitcher crops combine a buried pod with independently selected flower halves.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use super::state::{exact_tagged_byte, exact_tagged_int};
use assets::facts::{BlockRenderKind as R, block_render_fact};
use {super::super::*, assets::BlockFace};

pub(in crate::compiler) type TemplateKey = ([u32; 4], u8, bool);

/// Identifies pitcher crop states without admitting the harvested ornamental plant.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    block_render_fact(&record.name).kind == R::PitcherCrop
}

/// Reads the complete growth and half selector product.
fn state(record: &RegistryRecord) -> Option<(u8, bool)> {
    let state: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&record.canonical_state).ok()?;
    if state.len() != 2 {
        return None;
    }
    Some((
        exact_tagged_int(state.get(assets::facts::BlockProperty::Growth.name())?, 7)? as u8,
        exact_tagged_byte(
            state.get(assets::facts::BlockProperty::UpperBlockBit.name())?,
            1,
        )? != 0,
    ))
}

/// Selects pod faces and the exact lower or upper flower stage from authored arrays.
pub(in crate::compiler) fn descriptor(
    pack: &PackSources,
    record: &RegistryRecord,
    face: BlockFace,
) -> Option<(Descriptor, Box<str>)> {
    let (growth, _) = state(record)?;
    let key = resolve_texture_key(&pack.blocks, record, face).key?;
    let variant = match face {
        BlockFace::South => usize::from(growth.clamp(1, 4) - 1),
        BlockFace::East | BlockFace::West => usize::from(growth != 3),
        _ => 0,
    };
    let (path, state_variant) = pack.terrain.get_clamped_untinted_variant(&key, variant)?;
    Some((
        Descriptor {
            path: path.into(),
            texture_key: key.clone(),
            state_variant,
            flipbook_selector: None,
            flags: MATERIAL_FLAG_ALPHA_CUTOUT,
        },
        key,
    ))
}

/// Emits only admitted halves, with the pod retained beneath every lower-stage flower.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<TemplateKey, u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_record(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let Some((growth, upper)) = state(record) else {
        return Ok(CompileRuleResult::Reject);
    };
    let mut visual = diagnostic_visual(record);
    if upper && growth < 3 {
        visual.kind = VisualKind::Invisible;
        return Ok(CompileRuleResult::Compiled(visual));
    }
    let flower = if upper {
        inputs.material(record, BlockFace::East)
    } else {
        inputs.material(record, BlockFace::South)
    };
    let pod = [BlockFace::North, BlockFace::Down, BlockFace::Up]
        .map(|face| inputs.material(record, face));
    let materials = if upper {
        let Some(flower) = flower else {
            return Ok(CompileRuleResult::Reject);
        };
        [flower; 4]
    } else {
        let [Some(side), Some(bottom), Some(top)] = pod else {
            return Ok(CompileRuleResult::Reject);
        };
        if growth > 0 && flower.is_none() {
            return Ok(CompileRuleResult::Reject);
        }
        [side, bottom, top, flower.unwrap_or(side)]
    };
    let key = (materials, growth.min(4), upper);
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let mut quads = if upper {
            Vec::new()
        } else {
            pod_quads(materials, growth == 0).to_vec()
        };
        if upper || growth > 0 {
            let base = if !upper && growth <= 2 { 80 } else { 0 };
            quads.extend(super::cross::plant_quads(
                materials[3],
                256,
                base,
                super::cross::NARROW_PLANT_INSET,
            ));
        }
        let template = push_model_template(quads, 0, storage.templates, storage.quads)?;
        templates.insert(key, template);
        template
    };
    set_model_visual(&mut visual, [materials[0]; 6], template);
    // Keep crossed-plant semantics around the pod and flower geometry.
    visual.kind = VisualKind::Cross;
    if !upper {
        visual.faces[BlockFace::Down as usize] = materials[1];
        visual.faces[BlockFace::Up as usize] = materials[2];
        visual.faces[BlockFace::South as usize] = materials[3];
    }
    Ok(CompileRuleResult::Compiled(visual))
}

/// Crops each pod face to its dedicated top, bottom or side sprite rectangle.
fn pod_quads(materials: [u32; 4], small: bool) -> [ModelQuad; 6] {
    let [side, bottom, top, _] = materials;
    let (min, max) = if small {
        ([80, -16, 80], [176, 48, 176])
    } else {
        ([48, -16, 48], [208, 80, 208])
    };
    let mut quads =
        super::geometry::vanilla_cuboid_quads([side, side, bottom, top, side, side], min, max);
    for (face, quad) in BlockFace::ALL.into_iter().zip(&mut quads) {
        if matches!(face, BlockFace::Down | BlockFace::Up) {
            continue;
        }
        for (position, uv) in quad.positions.iter().zip(&mut quad.uvs) {
            uv[0] = if uv[0] <= 2048 {
                768
            } else if small {
                2304
            } else {
                3328
            };
            uv[1] = if position[1] == max[1] {
                2560
            } else if small {
                3584
            } else {
                4096
            };
        }
    }
    quads
}
