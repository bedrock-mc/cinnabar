//! Lenient normalization of server-driven debug drawing changes.

use render_api::primitive_shapes::{
    PrimitiveShapeChange, PrimitiveShapeData, PrimitiveShapeKind, PrimitiveShapeUpdate,
    PrimitiveShapesEvent, PrimitiveText,
};
use valentine::bedrock::version::v1_26_51::{
    EnumsScriptModuleMinecraftScriptPrimitiveShapeType as WireKind, MceColor,
    PrimitiveShapeDataPayload, PrimitiveShapeDataPayloadExtraShapeData as Extra,
    PrimitiveShapesPacket, Vec3,
};

/// Normalizes each decoded entry while preserving packet order.
pub(crate) fn normalize(packet: PrimitiveShapesPacket) -> PrimitiveShapesEvent {
    let entries = packet.arrayofprimitiveshapescanbeamixofnewupdatedorremoved;
    let mut result = PrimitiveShapesEvent {
        changes: Vec::with_capacity(entries.len()),
        skipped_entries: 0,
    };
    for entry in entries {
        match normalize_entry(entry) {
            Some(change) => result.changes.push(change),
            None => result.skipped_entries += 1,
        }
    }
    result
}

/// A deletion ignores unused fields; malformed numeric updates never reach rendering.
fn normalize_entry(entry: PrimitiveShapeDataPayload) -> Option<PrimitiveShapeChange> {
    let Some(wire_kind) = entry.shape_type else {
        return Some(PrimitiveShapeChange::Remove {
            network_id: entry.network_id,
        });
    };
    let kind = match wire_kind {
        WireKind::Line => PrimitiveShapeKind::Line,
        WireKind::Box => PrimitiveShapeKind::Box,
        WireKind::Sphere => PrimitiveShapeKind::Sphere,
        WireKind::Circle => PrimitiveShapeKind::Circle,
        WireKind::Text => PrimitiveShapeKind::Text,
        WireKind::Arrow => PrimitiveShapeKind::Arrow,
        _ => return None,
    };
    let location = entry.location.map(vector);
    let rotation = entry.rotation.map(vector);
    if !location.is_none_or(finite_vector)
        || !rotation.is_none_or(finite_vector)
        || !entry.scale.is_none_or(f32::is_finite)
        || !entry.total_time_left.is_none_or(f32::is_finite)
        || !entry.maximum_render_distance.is_none_or(f32::is_finite)
    {
        return None;
    }
    let data = normalize_data(entry.extra_shape_data)?;
    Some(PrimitiveShapeChange::Upsert(PrimitiveShapeUpdate {
        network_id: entry.network_id,
        kind,
        location,
        rotation,
        scale: entry.scale,
        color: entry.color.map(color),
        total_time_left: entry.total_time_left,
        maximum_render_distance: entry.maximum_render_distance,
        dimension: entry.dimension_id.map(|dimension| dimension.value),
        attached_actor: entry
            .attached_to_entity_id
            .map(|actor| actor.actor_unique_id),
        data,
    }))
}

/// Preserves recognized extra data so the store can match the id's retained concrete kind.
fn normalize_data(extra: Extra) -> Option<PrimitiveShapeData> {
    let data = match extra {
        Extra::LineDataPayload(line) => {
            let end = vector(line.line_end_location);
            finite_vector(end).then_some(())?;
            PrimitiveShapeData::Line { end }
        }
        Extra::BoxDataPayload(shape) => {
            let bounds = vector(shape.box_bound);
            finite_vector(bounds).then_some(())?;
            PrimitiveShapeData::Box { bounds }
        }
        Extra::SphereDataPayload(sphere) => PrimitiveShapeData::Segments(sphere.num_segments),
        Extra::ArrowDataPayload(arrow) => {
            let end = arrow.arrow_end_location.map(vector);
            if !end.is_none_or(finite_vector)
                || !arrow.arrow_head_length.is_none_or(f32::is_finite)
                || !arrow.arrow_head_radius.is_none_or(f32::is_finite)
            {
                return None;
            }
            PrimitiveShapeData::Arrow {
                end,
                head_length: arrow.arrow_head_length,
                head_radius: arrow.arrow_head_radius,
                segments: arrow.num_segments,
            }
        }
        Extra::TextDataPayload(text) => {
            if !text.line_gap_height.is_finite() || text.text.len() > crate::MAX_UI_TEXT_BYTES {
                return None;
            }
            PrimitiveShapeData::Text(PrimitiveText {
                text: text.text.into(),
                use_rotation: text.use_rotation,
                background_color: text.background_color.map(color),
                line_gap_height: text.line_gap_height,
                depth_test: text.depth_test,
                show_backface: text.show_backface,
                show_text_backface: text.show_text_backface,
            })
        }
        _ => PrimitiveShapeData::None,
    };
    Some(data)
}

/// Preserves world coordinates without introducing a rendering dependency.
fn vector(value: Vec3) -> [f32; 3] {
    [value.x, value.y, value.z]
}

/// Rejects non-finite positions before they can poison GPU transforms.
fn finite_vector(value: [f32; 3]) -> bool {
    value.into_iter().all(f32::is_finite)
}

/// Converts the packed ARGB integer without changing its alpha.
fn color(value: MceColor) -> [f32; 4] {
    let [blue, green, red, alpha] = value.color.to_le_bytes();
    [red, green, blue, alpha].map(|channel| f32::from(channel) / 255.0)
}

#[cfg(test)]
mod tests;
