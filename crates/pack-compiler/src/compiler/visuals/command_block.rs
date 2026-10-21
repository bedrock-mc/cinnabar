//! Command-block material roles and arrow orientation for all six facings.

use super::super::*;
use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use super::state::{exact_tagged_byte, exact_tagged_int};
use assets::facts::{BlockProperty as P, BlockRenderKind as R, block_render_fact};
use assets::{MODEL_POSITION_SCALE, MODEL_UV_SCALE};

const FRONTS: [BlockFace; 6] = [
    BlockFace::Down,
    BlockFace::Up,
    BlockFace::North,
    BlockFace::South,
    BlockFace::West,
    BlockFace::East,
];

/// Admits only command identities selected by the compiled block policy.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    block_render_fact(&record.name).kind == R::CommandBlock
}

/// Reads the exact facing/conditional selector product without accepting malformed states.
fn state(record: &RegistryRecord) -> Option<(usize, bool)> {
    let state: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&record.canonical_state).ok()?;
    if state.len() != 2 {
        return None;
    }
    Some((
        exact_tagged_int(
            state.get(P::FacingDirection.name())?,
            FRONTS.len() as u32 - 1,
        )? as usize,
        exact_tagged_byte(state.get(P::ConditionalBit.name())?, 1)? != 0,
    ))
}

/// Resolves each state role and its canonical atlas animation or static fallback.
pub(in crate::compiler) fn descriptor(
    pack: &PackSources,
    record: &RegistryRecord,
    face: BlockFace,
) -> Option<(Descriptor, Box<str>)> {
    let (direction, conditional) = state(record)?;
    let front = FRONTS[direction];
    let back = FRONTS[direction ^ 1];
    let source = if face == front {
        BlockFace::North
    } else if face == back {
        BlockFace::South
    } else if conditional {
        BlockFace::Up
    } else {
        BlockFace::East
    };
    let key = resolve_texture_key(&pack.blocks, record, source).key?;
    let (path, state_variant) = pack.terrain.get_clamped_untinted_variant(&key, 0)?;
    let selector = (0, state_variant);
    let path = pack
        .flipbooks
        .iter()
        .find(|flipbook| {
            flipbook.atlas_tile == key
                && flipbook.atlas_index == selector.0
                && flipbook.atlas_tile_variant == selector.1
        })
        .map_or(path, |flipbook| flipbook.texture_path.as_ref());
    Some((
        Descriptor {
            path: path.into(),
            texture_key: key.clone(),
            state_variant,
            flipbook_selector: Some(selector),
            flags: pack.blocks.isotropic_face_flags(record)[source as usize],
        },
        key,
    ))
}

/// Emits full opaque faces with state-owned arrow UVs and normal neighbour culling.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<([u32; 6], usize), u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_record(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let Some((direction, _)) = state(record) else {
        return Ok(CompileRuleResult::Reject);
    };
    let mut visual = diagnostic_visual(record);
    let Some(materials) = inputs.materials(record) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let key = (materials, direction);
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        const TURNS: [[u8; BlockFace::ALL.len()]; FRONTS.len()] = [
            [3, 3, 0, 0, 3, 3],
            [0, 0, 2, 1, 0, 0],
            [2, 1, 3, 0, 0, 0],
            [1, 2, 0, 3, 0, 0],
            [0, 0, 2, 2, 1, 2],
            [0, 0, 1, 1, 2, 1],
        ];
        let mut quads =
            super::geometry::vanilla_cuboid_quads(materials, [0; 3], [MODEL_POSITION_SCALE; 3]);
        for (face, quad) in BlockFace::ALL.into_iter().zip(&mut quads) {
            for uv in &mut quad.uvs {
                let [u, v] = *uv;
                *uv = match TURNS[direction][face as usize] {
                    1 => [v, MODEL_UV_SCALE - u],
                    2 => [MODEL_UV_SCALE - v, u],
                    3 => [MODEL_UV_SCALE - u, MODEL_UV_SCALE - v],
                    _ => [u, v],
                };
            }
            quad.flags |= face.model_quad_face_id() << 4;
        }
        let template = push_model_template(quads.to_vec(), 0, storage.templates, storage.quads)?;
        templates.insert(key, template);
        template
    };
    set_model_visual(&mut visual, materials, template);
    visual.flags.insert(BlockFlags::OCCLUDES_FULL_FACE);
    visual.support = VisualSupport::Exact;
    Ok(CompileRuleResult::Compiled(visual))
}
