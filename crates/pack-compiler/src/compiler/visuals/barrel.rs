//! Barrel face materials and UVs follow the lid direction and open state.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use super::state::{exact_tagged_byte, exact_tagged_int};
use {super::super::*, assets::BlockFace};

/// Identifies the terrain-owned barrel without admitting similarly named custom blocks.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    record.name.as_ref() == "minecraft:barrel"
}

/// Reads the complete typed selector product; malformed selectors stay diagnostic.
fn state(record: &RegistryRecord) -> Option<(usize, usize)> {
    let state: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&record.canonical_state).ok()?;
    if state.len() != 2 {
        return None;
    }
    Some((
        exact_tagged_int(state.get("facing_direction")?, 5)? as usize,
        exact_tagged_byte(state.get("open_bit")?, 1)? as usize,
    ))
}

/// Maps a world face to the pack's north-facing default barrel.
fn source_face(face: BlockFace, direction: usize) -> BlockFace {
    // Retain all six authored faces, including individual side-face overrides.
    const FACES: [[BlockFace; 6]; 6] = [
        [
            BlockFace::West,
            BlockFace::East,
            BlockFace::North,
            BlockFace::South,
            BlockFace::Up,
            BlockFace::Down,
        ],
        [
            BlockFace::West,
            BlockFace::East,
            BlockFace::South,
            BlockFace::North,
            BlockFace::Down,
            BlockFace::Up,
        ],
        BlockFace::ALL,
        [
            BlockFace::East,
            BlockFace::West,
            BlockFace::Down,
            BlockFace::Up,
            BlockFace::South,
            BlockFace::North,
        ],
        [
            BlockFace::North,
            BlockFace::South,
            BlockFace::Down,
            BlockFace::Up,
            BlockFace::East,
            BlockFace::West,
        ],
        [
            BlockFace::South,
            BlockFace::North,
            BlockFace::Down,
            BlockFace::Up,
            BlockFace::West,
            BlockFace::East,
        ],
    ];
    FACES[direction][face as usize]
}

/// Resolves the state's authored face and open variant, including scalar pack overrides.
pub(in crate::compiler) fn descriptor(
    pack: &PackSources,
    record: &RegistryRecord,
    face: BlockFace,
) -> Option<(Descriptor, Box<str>)> {
    let (direction, open) = state(record)?;
    let source_face = source_face(face, direction);
    let key = resolve_texture_key(&pack.blocks, record, source_face).key?;
    let (path, state_variant) = pack.terrain.get_clamped_untinted_variant(&key, open)?;
    Some((
        Descriptor {
            path: path.into(),
            texture_key: key.clone(),
            state_variant,
            flipbook_selector: Some((state_variant, state_variant)),
            flags: pack.blocks.isotropic_face_flags(record)[source_face as usize],
        },
        key,
    ))
}

/// Emits state-specific UVs while preserving full-face culling and opaque neighbour coverage.
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
        // Face order is west, east, down, up, north, south.
        const TURNS: [[u8; 6]; 6] = [
            [3, 3, 0, 0, 3, 3],
            [0; 6],
            [2, 2, 3, 0, 0, 0],
            [1, 1, 0, 3, 0, 0],
            [0, 0, 2, 2, 2, 2],
            [0, 0, 1, 1, 1, 1],
        ];
        let mut quads = super::geometry::vanilla_cuboid_quads(materials, [0; 3], [256; 3]);
        for (face, quad) in BlockFace::ALL.into_iter().zip(&mut quads) {
            for uv in &mut quad.uvs {
                let [u, v] = *uv;
                *uv = match TURNS[direction][face as usize] {
                    1 => [v, 4096 - u],
                    2 => [4096 - v, u],
                    3 => [4096 - u, 4096 - v],
                    _ => [u, v],
                };
            }
            quad.flags |= (quad.flags & MODEL_QUAD_FLAG_FACE_MASK) << 4;
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
