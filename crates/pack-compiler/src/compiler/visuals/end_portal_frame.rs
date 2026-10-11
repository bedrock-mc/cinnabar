//! End portal frames use a rotated base and a state-dependent eye cuboid.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use {super::super::*, assets::BlockFace};

const FRAME_TOP: i16 = 13 * 16;
const EYE_NEAR: i16 = 4 * 16;
const EYE_FAR: i16 = 12 * 16;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::compiler) struct TemplateKey {
    materials: [u32; 6],
    eye_material: Option<u32>,
    direction: u32,
}

pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    record.name.as_ref() == assets::END_PORTAL_FRAME_IDENTIFIER
        && record.contributor_role == ContributorRole::Primary
        && !record.flags.contains(BlockFlags::AIR)
}

fn frame_direction(record: &RegistryRecord) -> Option<u32> {
    match canonical_state_str(&record.canonical_state, "minecraft:cardinal_direction")?.as_ref() {
        "south" => Some(0),
        "west" => Some(1),
        "north" => Some(2),
        "east" => Some(3),
        _ => None,
    }
}

pub(in crate::compiler) fn eye_descriptor(
    pack: &PackSources,
    record: &RegistryRecord,
) -> Option<(Descriptor, Box<str>)> {
    if canonical_state_u32(&record.canonical_state, "end_portal_eye_bit") != Some(1) {
        return None;
    }
    // Vanilla overrides every eye face with carried texture (face 1, variant 0).
    let key = crate::pack::resolve_carried_face_key(&pack.blocks, record, BlockFace::Up)?;
    let (path, state_variant) = pack.terrain.get_for_model_record(&key, record)?;
    Some((
        Descriptor {
            path: path.into(),
            texture_key: key.clone().into(),
            flags: MATERIAL_FLAG_ALPHA_CUTOUT,
            state_variant,
        },
        key.into(),
    ))
}

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<TemplateKey, u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_record(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let mut visual = diagnostic_visual(record);
    let Some(direction) = frame_direction(record) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let Some(eye @ 0..=1) = canonical_state_u32(&record.canonical_state, "end_portal_eye_bit")
    else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let Some(materials) = inputs.materials(record) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let eye_material = if eye == 1 {
        let Some((descriptor, _)) = eye_descriptor(inputs.pack, record) else {
            return Ok(CompileRuleResult::Compiled(visual));
        };
        let Some(&material) = inputs.material_by_descriptor.get(&descriptor) else {
            return Ok(CompileRuleResult::Compiled(visual));
        };
        Some(material)
    } else {
        None
    };
    let key = TemplateKey {
        materials,
        eye_material,
        direction,
    };
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let mut quads = frame_cuboid(materials, [0; 3], [256, FRAME_TOP, 256], direction).to_vec();
        if let Some(material) = eye_material {
            quads.extend(frame_cuboid(
                [material; 6],
                [EYE_NEAR, FRAME_TOP, EYE_NEAR],
                [EYE_FAR, 256, EYE_FAR],
                direction,
            ));
        }
        let template = push_model_template(quads, 0, storage.templates, storage.quads)?;
        templates.insert(key, template);
        template
    };
    set_model_visual(&mut visual, materials, template);
    Ok(CompileRuleResult::Compiled(visual))
}

fn frame_cuboid(
    materials: [u32; 6],
    min: [i16; 3],
    max: [i16; 3],
    direction: u32,
) -> [ModelQuad; 6] {
    let mut quads = super::geometry::vanilla_cuboid_quads(materials, min, max);
    for (face, quad) in BlockFace::ALL.into_iter().zip(&mut quads) {
        for uv in &mut quad.uvs {
            let [u, v] = *uv;
            *uv = match face {
                // Native top rotations: direction 0→3, 1→2, 2→0, 3→1.
                BlockFace::Up => match direction {
                    0 => [4096 - u, 4096 - v],
                    1 => [4096 - v, u],
                    3 => [v, 4096 - u],
                    _ => [u, v],
                },
                _ => [u, v],
            };
        }
        let axis = match face {
            BlockFace::West | BlockFace::East => 0,
            BlockFace::Down | BlockFace::Up => 1,
            _ => 2,
        };
        if quad
            .positions
            .iter()
            .all(|position| position[axis] == 0 || position[axis] == 256)
        {
            quad.flags |= (quad.flags & MODEL_QUAD_FLAG_FACE_MASK) << 4;
        }
    }
    quads
}

#[cfg(test)]
#[path = "end_portal_frame_tests.rs"]
mod tests;
