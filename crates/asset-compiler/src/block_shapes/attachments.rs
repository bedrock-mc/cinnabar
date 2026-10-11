use super::{selection::bounds, state::State};
use assets::{RegistryRecord, block_shapes::ShapeBox};

/// Compiles block-local face attachments whose bounds depend only on state.
pub(super) fn shape(record: &RegistryRecord) -> Option<ShapeBox> {
    let name = record.name.strip_prefix("minecraft:")?;
    let state = State::read(record)?;
    if name == "frame" || name == "glow_frame" {
        let inset = if state.bit("item_frame_map_bit")? || state.bit("item_frame_photo_bit")? {
            0.0
        } else {
            0.125
        };
        return face_box(state.number("facing_direction")?, inset, 0.0625);
    }
    if name == "ladder" {
        let facing = state.number("facing_direction")?;
        return (2..=5)
            .contains(&facing)
            .then(|| face_box(facing, 0.0, 0.1875))
            .flatten();
    }
    if name.ends_with("_skull") || name.ends_with("_head") {
        let (min, max) = match state.number("facing_direction")? {
            0 | 1 => ([0.25, 0.0, 0.25], [0.75, 0.5, 0.75]),
            2 => ([0.25, 0.25, 0.5], [0.75, 0.75, 1.0]),
            3 => ([0.25, 0.25, 0.0], [0.75, 0.75, 0.5]),
            4 => ([0.5, 0.25, 0.25], [1.0, 0.75, 0.75]),
            5 => ([0.0, 0.25, 0.25], [0.5, 0.75, 0.75]),
            _ => return None,
        };
        return Some(bounds(min, max));
    }
    None
}

/// Builds a thin sheet against one supporting face, inset on the tangent axes.
fn face_box(facing: i64, inset: f32, thickness: f32) -> Option<ShapeBox> {
    let edge = 1.0 - inset;
    let (min, max) = match facing {
        0 => ([inset, 1.0 - thickness, inset], [edge, 1.0, edge]),
        1 => ([inset, 0.0, inset], [edge, thickness, edge]),
        2 => ([inset, inset, 1.0 - thickness], [edge, edge, 1.0]),
        3 => ([inset, inset, 0.0], [edge, edge, thickness]),
        4 => ([1.0 - thickness, inset, inset], [1.0, edge, edge]),
        5 => ([0.0, inset, inset], [thickness, edge, edge]),
        _ => return None,
    };
    Some(bounds(min, max))
}
