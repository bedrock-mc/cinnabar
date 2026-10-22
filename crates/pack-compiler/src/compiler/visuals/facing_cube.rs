//! Full cubes whose pack faces turn with the block's horizontal facing.
//!
//! The pack's `blocks.json` authors these blocks for one facing. Furnace-family cubes show their
//! front texture on the facing side and the north texture on the other sides; the loom family
//! turns every face toward its direction and flips its top and bottom artwork to match.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use super::state::{exact_tagged_int, exact_tagged_string};
use assets::facts::{BlockProperty, BlockRenderKind as R, block_render_fact};
use {super::super::*, assets::BlockFace};

/// Horizontal facing of each `direction` value, in the registry's south, west, north, east order.
const DIRECTION_FACING: [BlockFace; 4] = [
    BlockFace::South,
    BlockFace::West,
    BlockFace::North,
    BlockFace::East,
];

/// Whether the record's pack faces follow its facing.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    matches!(
        block_render_fact(&record.name).kind,
        R::Furnace | R::LitFurnace | R::FacingCube
    )
}

/// The horizontal facing selected by the record's complete, typed state.
fn facing(record: &RegistryRecord) -> Option<BlockFace> {
    let state: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&record.canonical_state).ok()?;
    if state.len() != 1 {
        return None;
    }
    let index = if block_render_fact(&record.name).kind == R::FacingCube {
        exact_tagged_int(state.get(BlockProperty::Direction.name())?, 3)? as usize
    } else {
        let name =
            exact_tagged_string(state.get(BlockProperty::MinecraftCardinalDirection.name())?)?;
        ["south", "west", "north", "east"]
            .iter()
            .position(|&candidate| candidate == name)?
    };
    Some(DIRECTION_FACING[index])
}

/// The pack face drawn on `face` and its native UV flip, or `None` for a malformed state.
pub(in crate::compiler) fn source_face(
    record: &RegistryRecord,
    face: BlockFace,
) -> Option<(BlockFace, u8)> {
    let facing = facing(record)?;
    let kind = block_render_fact(&record.name).kind;
    if matches!(kind, R::Furnace | R::LitFurnace) {
        let source = match face {
            BlockFace::Down | BlockFace::Up => face,
            _ if face == facing && kind == R::LitFurnace => BlockFace::East,
            _ if face == facing => BlockFace::South,
            _ => BlockFace::North,
        };
        return Some((source, 0));
    }
    Some(match (facing, face) {
        (_, BlockFace::Down | BlockFace::Up) => (face, vertical_flip(facing, face)),
        (BlockFace::South, side) => (opposite(side), 0),
        (BlockFace::West, BlockFace::North) => (BlockFace::East, 0),
        (BlockFace::West, BlockFace::South) => (BlockFace::West, 0),
        (BlockFace::West, BlockFace::West) => (BlockFace::North, 0),
        (BlockFace::West, BlockFace::East) => (BlockFace::South, 0),
        (BlockFace::East, BlockFace::North) => (BlockFace::West, 0),
        (BlockFace::East, BlockFace::South) => (BlockFace::East, 0),
        (BlockFace::East, BlockFace::West) => (BlockFace::South, 0),
        (BlockFace::East, BlockFace::East) => (BlockFace::North, 0),
        (_, side) => (side, 0),
    })
}

/// Native top and bottom flip for a horizontal facing, in [`super::geometry::flip_cube_uvs`] terms.
fn vertical_flip(facing: BlockFace, face: BlockFace) -> u8 {
    match (facing, face) {
        (BlockFace::North, BlockFace::Up) | (BlockFace::South, BlockFace::Down) => 3,
        (BlockFace::West, _) => 1,
        (BlockFace::East, _) => 2,
        _ => 0,
    }
}

fn opposite(face: BlockFace) -> BlockFace {
    match face {
        BlockFace::West => BlockFace::East,
        BlockFace::East => BlockFace::West,
        BlockFace::Down => BlockFace::Up,
        BlockFace::Up => BlockFace::Down,
        BlockFace::North => BlockFace::South,
        BlockFace::South => BlockFace::North,
    }
}

/// Compiles flipped top and bottom UVs for the loom family; furnace cubes stay ordinary cubes.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<([u32; 6], [u8; 6]), u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if block_render_fact(&record.name).kind != R::FacingCube {
        return Ok(CompileRuleResult::NoMatch);
    }
    let Some(flips) = BlockFace::ALL
        .into_iter()
        .map(|face| source_face(record, face).map(|(_, flip)| flip))
        .collect::<Option<Vec<_>>>()
    else {
        return Ok(CompileRuleResult::Reject);
    };
    let flips: [u8; 6] = flips.try_into().expect("one flip per face");
    let mut visual = diagnostic_visual(record);
    let Some(materials) = inputs.materials(record) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let key = (materials, flips);
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let quads = super::geometry::flipped_cube_quads(materials, flips);
        let template = push_model_template(quads.to_vec(), 0, storage.templates, storage.quads)?;
        templates.insert(key, template);
        template
    };
    set_model_visual(&mut visual, materials, template);
    visual.flags.insert(BlockFlags::OCCLUDES_FULL_FACE);
    visual.support = VisualSupport::Exact;
    Ok(CompileRuleResult::Compiled(visual))
}
