use std::cell::OnceCell;

use assets::{
    BlockFlags, MODEL_QUAD_FLAG_CULL_FACE_MASK, MODEL_TEMPLATE_FLAG_FENCE_NETHER,
    MODEL_TEMPLATE_FLAG_FENCE_WOOD, MODEL_TEMPLATE_FLAG_GATE_AXIS_X,
    MODEL_TEMPLATE_FLAG_GATE_AXIS_Z, MODEL_TEMPLATE_FLAG_KELP, MODEL_TEMPLATE_FLAG_PANE,
    MODEL_TEMPLATE_FLAG_STAIR, MODEL_TEMPLATE_FLAG_WALL, NO_MODEL_TEMPLATE, NetworkIdMode,
    RuntimeAssets, VisualKind,
};
use world::MeshNeighbourhood;

use super::opaque::face_offset;
use crate::{
    BlockClassifier, Face, SIDE,
    contributors::{PaletteFacts, ResolvedPaletteEntry},
};

pub(crate) fn is_kelp_entry(visuals: &RuntimeAssets, entry: ResolvedPaletteEntry) -> bool {
    entry.kind == VisualKind::Model
        && entry.model_template != NO_MODEL_TEMPLATE
        && visuals
            .model_templates()
            .get(entry.model_template as usize)
            .is_some_and(|template| template.flags & MODEL_TEMPLATE_FLAG_KELP != 0)
}

pub(crate) fn select_model_templates<'a>(
    context: PaletteResolutionContext<'_, 'a>,
    facts: &PaletteFacts<'a>,
    neighbour_facts: &[OnceCell<PaletteFacts<'a>>; Face::ALL.len()],
    coordinate: [usize; 3],
    entry: ResolvedPaletteEntry,
) -> ([u32; 2], u8) {
    let flags = model_template_flags(context.visuals, entry);
    if flags & MODEL_TEMPLATE_FLAG_PANE != 0 {
        let mask = connected_model_mask(
            context,
            facts,
            neighbour_facts,
            coordinate,
            MODEL_TEMPLATE_FLAG_PANE,
        );
        return ([entry.model_template + mask, NO_MODEL_TEMPLATE], 1);
    }
    let fence_flag = flags & (MODEL_TEMPLATE_FLAG_FENCE_WOOD | MODEL_TEMPLATE_FLAG_FENCE_NETHER);
    if fence_flag != 0 {
        let mask = connected_model_mask(context, facts, neighbour_facts, coordinate, fence_flag);
        if mask != 0 {
            return ([entry.model_template, entry.model_template + 1 + mask], 2);
        }
        return ([entry.model_template, NO_MODEL_TEMPLATE], 1);
    }
    (
        [
            select_stair_template(context, facts, neighbour_facts, coordinate, entry),
            NO_MODEL_TEMPLATE,
        ],
        1,
    )
}

fn connected_model_mask<'a>(
    context: PaletteResolutionContext<'_, 'a>,
    facts: &PaletteFacts<'a>,
    neighbour_facts: &[OnceCell<PaletteFacts<'a>>; Face::ALL.len()],
    coordinate: [usize; 3],
    connection_flag: u32,
) -> u32 {
    [
        (Face::NegativeZ, 1_u32),
        (Face::PositiveX, 2),
        (Face::PositiveZ, 4),
        (Face::NegativeX, 8),
    ]
    .into_iter()
    .filter_map(|(face, bit)| {
        let neighbour = adjacent_palette_entry(context, facts, neighbour_facts, coordinate, face);
        let neighbour_flags = model_template_flags(context.visuals, neighbour);
        let connects = if connection_flag == MODEL_TEMPLATE_FLAG_PANE {
            neighbour_flags & MODEL_TEMPLATE_FLAG_PANE != 0
                || neighbour_flags & MODEL_TEMPLATE_FLAG_WALL != 0
                || neighbour.flags.contains(BlockFlags::OCCLUDES_FULL_FACE)
        } else {
            neighbour_flags & connection_flag != 0
                || match face {
                    Face::NegativeZ | Face::PositiveZ => {
                        neighbour_flags & MODEL_TEMPLATE_FLAG_GATE_AXIS_X != 0
                    }
                    Face::NegativeX | Face::PositiveX => {
                        neighbour_flags & MODEL_TEMPLATE_FLAG_GATE_AXIS_Z != 0
                    }
                    Face::NegativeY | Face::PositiveY => false,
                }
                || neighbour.flags.contains(BlockFlags::OCCLUDES_FULL_FACE)
        };
        connects.then_some(bit)
    })
    .fold(0, |mask, bit| mask | bit)
}

pub(crate) fn model_template_flags(visuals: &RuntimeAssets, entry: ResolvedPaletteEntry) -> u32 {
    (entry.kind == VisualKind::Model && entry.model_template != NO_MODEL_TEMPLATE)
        .then(|| visuals.model_templates().get(entry.model_template as usize))
        .flatten()
        .map_or(0, |template| template.flags)
}

fn select_stair_template<'a>(
    context: PaletteResolutionContext<'_, 'a>,
    facts: &PaletteFacts<'a>,
    neighbour_facts: &[OnceCell<PaletteFacts<'a>>; Face::ALL.len()],
    coordinate: [usize; 3],
    entry: ResolvedPaletteEntry,
) -> u32 {
    let Some((facing, upside_down)) = stair_signature(context.visuals, entry) else {
        return entry.model_template;
    };
    let rotated_facing = (facing + 1) & 3;
    let closed = adjacent_palette_entry(
        context,
        facts,
        neighbour_facts,
        coordinate,
        stair_face(facing),
    );
    if let Some((closed_facing, closed_upside)) = stair_signature(context.visuals, closed)
        && closed_upside == upside_down
    {
        if closed_facing == rotated_facing {
            return entry.model_template + 4;
        }
        if closed_facing == ((rotated_facing + 2) & 3) {
            let side = adjacent_palette_entry(
                context,
                facts,
                neighbour_facts,
                coordinate,
                stair_face(rotated_facing),
            );
            if stair_signature(context.visuals, side) != Some((facing, upside_down)) {
                return entry.model_template + 3;
            }
            return entry.model_template;
        }
    }
    let open = adjacent_palette_entry(
        context,
        facts,
        neighbour_facts,
        coordinate,
        stair_face((facing + 2) & 3),
    );
    if let Some((open_facing, open_upside)) = stair_signature(context.visuals, open)
        && open_upside == upside_down
    {
        if open_facing == rotated_facing {
            let side = adjacent_palette_entry(
                context,
                facts,
                neighbour_facts,
                coordinate,
                stair_face(rotated_facing),
            );
            if stair_signature(context.visuals, side) != Some((facing, upside_down)) {
                return entry.model_template + 1;
            }
        } else if open_facing == ((rotated_facing + 2) & 3) {
            return entry.model_template + 2;
        }
    }
    entry.model_template
}

fn stair_signature(visuals: &RuntimeAssets, entry: ResolvedPaletteEntry) -> Option<(u32, bool)> {
    (entry.kind == VisualKind::Model
        && entry.model_template != NO_MODEL_TEMPLATE
        && visuals
            .model_templates()
            .get(entry.model_template as usize)
            .is_some_and(|template| template.flags & MODEL_TEMPLATE_FLAG_STAIR != 0))
    .then_some((((entry.variant & 3) + 2) & 3, entry.variant & 4 != 0))
}

const fn stair_face(facing: u32) -> Face {
    match facing {
        0 => Face::PositiveZ,
        1 => Face::NegativeX,
        2 => Face::NegativeZ,
        3 => Face::PositiveX,
        _ => Face::NegativeZ,
    }
}

pub(crate) const fn model_quad_cull_face(flags: u32, rotation: u32) -> Option<Face> {
    let face = match (flags & MODEL_QUAD_FLAG_CULL_FACE_MASK) >> 4 {
        1 => Some(Face::NegativeY),
        2 => Some(Face::PositiveY),
        3 => Some(Face::NegativeX),
        4 => Some(Face::PositiveX),
        5 => Some(Face::NegativeZ),
        6 => Some(Face::PositiveZ),
        _ => None,
    };
    match (face, rotation & 3) {
        (Some(Face::NegativeX), 1) => Some(Face::NegativeZ),
        (Some(Face::PositiveX), 1) => Some(Face::PositiveZ),
        (Some(Face::NegativeZ), 1) => Some(Face::PositiveX),
        (Some(Face::PositiveZ), 1) => Some(Face::NegativeX),
        (Some(Face::NegativeX), 2) => Some(Face::PositiveX),
        (Some(Face::PositiveX), 2) => Some(Face::NegativeX),
        (Some(Face::NegativeZ), 2) => Some(Face::PositiveZ),
        (Some(Face::PositiveZ), 2) => Some(Face::NegativeZ),
        (Some(Face::NegativeX), 3) => Some(Face::PositiveZ),
        (Some(Face::PositiveX), 3) => Some(Face::NegativeZ),
        (Some(Face::NegativeZ), 3) => Some(Face::NegativeX),
        (Some(Face::PositiveZ), 3) => Some(Face::PositiveX),
        (other, _) => other,
    }
}

#[derive(Clone, Copy)]
pub(crate) struct PaletteResolutionContext<'assets, 'chunks> {
    pub(crate) classifier: BlockClassifier,
    pub(crate) visuals: &'assets RuntimeAssets,
    pub(crate) network_id_mode: NetworkIdMode,
    pub(crate) neighbourhood: &'assets MeshNeighbourhood<'chunks>,
}

pub(crate) fn adjacent_palette_entry<'a>(
    context: PaletteResolutionContext<'_, 'a>,
    facts: &PaletteFacts<'a>,
    neighbour_facts: &[OnceCell<PaletteFacts<'a>>; Face::ALL.len()],
    [x, y, z]: [usize; 3],
    face: Face,
) -> ResolvedPaletteEntry {
    let local = match face {
        Face::NegativeX => x.checked_sub(1).map(|x| [x, y, z]),
        Face::PositiveX => (x + 1 < SIDE).then_some([x + 1, y, z]),
        Face::NegativeY => y.checked_sub(1).map(|y| [x, y, z]),
        Face::PositiveY => (y + 1 < SIDE).then_some([x, y + 1, z]),
        Face::NegativeZ => z.checked_sub(1).map(|z| [x, y, z]),
        Face::PositiveZ => (z + 1 < SIDE).then_some([x, y, z + 1]),
    };
    if let Some([x, y, z]) = local {
        return facts.at(x, y, z);
    }

    let boundary = match face {
        Face::NegativeX => [SIDE - 1, y, z],
        Face::PositiveX => [0, y, z],
        Face::NegativeY => [x, SIDE - 1, z],
        Face::PositiveY => [x, 0, z],
        Face::NegativeZ => [x, y, SIDE - 1],
        Face::PositiveZ => [x, y, 0],
    };
    context
        .neighbourhood
        .sub_chunk(face_offset(face))
        .map(|sub_chunk| {
            neighbour_facts[face.index()]
                .get_or_init(|| {
                    PaletteFacts::new(
                        context.classifier,
                        context.visuals,
                        context.network_id_mode,
                        sub_chunk,
                    )
                })
                .at(boundary[0], boundary[1], boundary[2])
        })
        .unwrap_or(ResolvedPaletteEntry::AIR)
}
