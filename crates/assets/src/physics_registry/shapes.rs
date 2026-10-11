//! Binary selection and interaction records for PREG1002.

use super::{Reader, invalid, physics_error};
use crate::AssetError;
use crate::block_shapes::{
    CompiledBlockShapes, CompiledDoor, CompiledFlow, OutlineContext, PickRule, SelectionContext,
    ShapeBox, ShapeFlags,
};

/// Reads bounded compiled facts without interpreting names or canonical state strings.
pub(super) fn read(reader: &mut Reader<'_>) -> Result<CompiledBlockShapes, AssetError> {
    let state_identity = reader
        .read_exact(32, "shape state identity")?
        .try_into()
        .expect("32 bytes");
    let facing = reader.read_u8("actor facing")?;
    let actor_facing = match facing {
        0..=5 => Some(facing),
        255 => None,
        _ => return invalid("invalid actor facing"),
    };
    let actor_visual = read_outline(reader)?;
    let count = usize::from(reader.read_u8("selection count")?);
    if count > super::MAX_BOXES {
        return invalid("too many selection boxes");
    }
    let pick_rule = match reader.read_u8("pick rule")? {
        0 => PickRule::Never,
        1 => PickRule::Always,
        2 => PickRule::LiquidSource,
        3 => PickRule::Liquid,
        4 => PickRule::Creative,
        5 => PickRule::HeldLight,
        _ => return invalid("unknown pick rule"),
    };
    let flags = ShapeFlags::from_bits(u16::from_le_bytes(
        reader
            .read_exact(2, "shape flags")?
            .try_into()
            .expect("two bytes"),
    ))
    .ok_or_else(|| physics_error("unknown shape flags"))?;
    let facing = reader.read_u8("door facing")?;
    let door_bits = reader.read_u8("door properties")?;
    let door = if facing == u8::MAX {
        if door_bits != 0 {
            return invalid("properties without a door");
        }
        None
    } else {
        if facing > 3 || door_bits & !7 != 0 {
            return invalid("invalid door properties");
        }
        Some(CompiledDoor {
            family: reader
                .read_exact(32, "door family")?
                .try_into()
                .expect("32 bytes"),
            facing,
            upper: door_bits & 1 != 0,
            open: door_bits & 2 != 0,
            hinge_right: door_bits & 4 != 0,
        })
    };
    let flow_bits = reader.read_u8("flow properties")?;
    let blocked_faces = reader.read_u8("blocked faces")?;
    let allowed_faces = reader.read_u8("allowed faces")?;
    let depth = reader.read_u8("liquid depth")?;
    if flow_bits & !7 != 0
        || blocked_faces & !0x3f != 0
        || allowed_faces & !0x3f != 0
        || (depth != u8::MAX && depth > 15)
    {
        return invalid("invalid flow properties");
    }
    let flow = if flow_bits & 1 != 0 {
        Some(CompiledFlow {
            blocks_motion: flow_bits & 2 != 0,
            is_solid: flow_bits & 4 != 0,
            blocked_faces,
            allowed_faces,
            liquid_depth: (depth != u8::MAX).then_some(depth),
        })
    } else {
        if flow_bits != 0 || blocked_faces != 0 || allowed_faces != 0 || depth != u8::MAX {
            return invalid("properties without flow facts");
        }
        None
    };
    let mut selection = Vec::with_capacity(count);
    for _ in 0..count {
        selection.push(read_box(reader)?);
    }
    let selection_context = match reader.read_u8("selection context")? {
        0 => SelectionContext::Static,
        1 => SelectionContext::PortalAxis,
        2 => SelectionContext::VineSupport,
        3 => SelectionContext::ScaffoldSupport,
        4 => SelectionContext::PistonArm,
        5 => SelectionContext::MovingBlock,
        _ => return invalid("unknown selection context"),
    };
    let contextual_count = usize::from(reader.read_u8("contextual selection count")?);
    if contextual_count > super::MAX_BOXES {
        return invalid("too many contextual selection boxes");
    }
    let mut contextual_selection = Vec::with_capacity(contextual_count);
    for _ in 0..contextual_count {
        contextual_selection.push(read_box(reader)?);
    }
    let outline_context = match reader.read_u8("outline context")? {
        0 => OutlineContext::Static,
        1 => OutlineContext::TwistingVines,
        2 => OutlineContext::WeepingVines,
        3 => OutlineContext::Chorus,
        _ => return invalid("unknown outline context"),
    };
    let outline = read_outline(reader)?;
    let connected_outline = read_outline(reader)?;
    let count = usize::from(reader.read_u8("tag count")?);
    let mut tags: Vec<Box<str>> = Vec::with_capacity(count);
    for _ in 0..count {
        let len = usize::from(reader.read_u8("tag length")?);
        let value = std::str::from_utf8(reader.read_exact(len, "tag")?)
            .map_err(|_| physics_error("invalid tag UTF-8"))?;
        if value.is_empty() || tags.iter().any(|tag| tag.as_ref() == value) {
            return invalid("empty or duplicate tag");
        }
        tags.push(value.into());
    }
    let shapes = CompiledBlockShapes {
        state_identity,
        actor_facing,
        actor_visual,
        selection: selection.into_boxed_slice(),
        selection_context,
        contextual_selection: contextual_selection.into_boxed_slice(),
        outline,
        connected_outline,
        outline_context,
        pick_rule,
        flags,
        door,
        flow,
        tags: tags.into_boxed_slice(),
    };
    validate(&shapes)?;
    Ok(shapes)
}

/// Writes compiled facts using the same bounded layout as the strict reader.
pub(super) fn write(bytes: &mut Vec<u8>, shapes: &CompiledBlockShapes) -> Result<(), AssetError> {
    validate(shapes)?;
    if shapes.selection.len() > super::MAX_BOXES
        || shapes.tags.len() > 255
        || shapes.selection.iter().any(|shape| !shape.is_valid())
    {
        return invalid("invalid compiled shapes");
    }
    bytes.extend(shapes.state_identity);
    bytes.push(shapes.actor_facing.unwrap_or(u8::MAX));
    bytes.push(u8::from(shapes.actor_visual.is_some()));
    if let Some(shape) = shapes.actor_visual {
        if !shape.is_valid() {
            return invalid("invalid actor visual box");
        }
        for value in shape.min.into_iter().chain(shape.max) {
            bytes.extend(value.to_le_bytes());
        }
    }
    bytes.push(shapes.selection.len() as u8);
    bytes.push(shapes.pick_rule as u8);
    bytes.extend_from_slice(&shapes.flags.bits().to_le_bytes());
    let (facing, bits) = shapes.door.map_or((u8::MAX, 0), |door| {
        (
            door.facing,
            u8::from(door.upper) | (u8::from(door.open) << 1) | (u8::from(door.hinge_right) << 2),
        )
    });
    bytes.extend([facing, bits]);
    if let Some(door) = shapes.door {
        bytes.extend(door.family);
    }
    let flow = shapes.flow;
    bytes.extend([
        flow.map_or(0, |flow| {
            1 | (u8::from(flow.blocks_motion) << 1) | (u8::from(flow.is_solid) << 2)
        }),
        flow.map_or(0, |flow| flow.blocked_faces),
        flow.map_or(0, |flow| flow.allowed_faces),
        flow.and_then(|flow| flow.liquid_depth).unwrap_or(u8::MAX),
    ]);
    for shape in &shapes.selection {
        for value in shape.min.into_iter().chain(shape.max) {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.push(shapes.selection_context as u8);
    if shapes.contextual_selection.len() > super::MAX_BOXES {
        return invalid("too many contextual selection boxes");
    }
    bytes.push(shapes.contextual_selection.len() as u8);
    for shape in &shapes.contextual_selection {
        if !shape.is_valid() {
            return invalid("invalid contextual selection box");
        }
        for value in shape.min.into_iter().chain(shape.max) {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.push(shapes.outline_context as u8);
    for outline in [shapes.outline, shapes.connected_outline] {
        bytes.push(u8::from(outline.is_some()));
        if let Some(shape) = outline {
            if !shape.is_valid() {
                return invalid("invalid compiled outline");
            }
            for value in shape.min.into_iter().chain(shape.max) {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
    }
    bytes.push(shapes.tags.len() as u8);
    for tag in &shapes.tags {
        if tag.is_empty() || tag.len() > 255 {
            return invalid("invalid compiled tag");
        }
        bytes.push(tag.len() as u8);
        bytes.extend_from_slice(tag.as_bytes());
    }
    Ok(())
}

/// Rejects incomplete geometry and mismatched context payloads before they reach hot lookups.
fn validate(shapes: &CompiledBlockShapes) -> Result<(), AssetError> {
    if shapes.actor_visual.is_some_and(|shape| !shape.is_valid())
        || shapes.outline.is_some_and(|shape| !shape.is_valid())
        || shapes
            .connected_outline
            .is_some_and(|shape| !shape.is_valid())
        || shapes.tags.iter().enumerate().any(|(index, tag)| {
            tag.is_empty() || tag.len() > 255 || shapes.tags[..index].contains(tag)
        })
        || shapes.door.is_some_and(|door| door.facing > 3)
        || shapes.flow.is_some_and(|flow| {
            flow.blocked_faces & !0x3f != 0
                || flow.allowed_faces & !0x3f != 0
                || flow.liquid_depth.is_some_and(|depth| depth > 15)
        })
    {
        return invalid("invalid compiled shape facts");
    }
    let expected = match shapes.selection_context {
        SelectionContext::Static | SelectionContext::PistonArm | SelectionContext::MovingBlock => 0,
        SelectionContext::PortalAxis => 1,
        SelectionContext::VineSupport => 3,
        SelectionContext::ScaffoldSupport => 9,
    };
    if shapes.contextual_selection.len() != expected
        || shapes.actor_facing.is_some_and(|face| face > 5)
        || (shapes.selection_context == SelectionContext::PistonArm
            && (shapes.actor_facing.is_none() || shapes.selection.len() != 3))
        || (shapes.selection.is_empty()
            && shapes.contextual_selection.is_empty()
            && shapes.pick_rule != PickRule::Never
            && shapes.selection_context != SelectionContext::MovingBlock)
    {
        return invalid("incomplete compiled selection geometry");
    }
    Ok(())
}

/// Reads one optional outline with finite positive bounds.
fn read_outline(reader: &mut Reader<'_>) -> Result<Option<ShapeBox>, AssetError> {
    Ok(match reader.read_u8("outline count")? {
        0 => None,
        1 => Some(read_box(reader)?),
        _ => return invalid("too many outline boxes"),
    })
}

/// Reads a finite, positive box for a context-dependent clipping rule.
fn read_box(reader: &mut Reader<'_>) -> Result<ShapeBox, AssetError> {
    let mut coords = [0.0; 6];
    for value in &mut coords {
        *value = f64::from_le_bytes(
            reader
                .read_exact(8, "shape coordinate")?
                .try_into()
                .expect("eight bytes"),
        );
    }
    let shape = ShapeBox::new(
        coords[..3].try_into().expect("three coordinates"),
        coords[3..].try_into().expect("three coordinates"),
    );
    if !shape.is_valid() {
        return invalid("invalid shape box");
    }
    Ok(shape)
}

#[cfg(test)]
mod tests;
