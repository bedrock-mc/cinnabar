//! Lenient normalization of server-driven debug drawing changes.

use bedrock_protocol_1_26_51::enums::ScriptModuleMinecraftScriptPrimitiveShapeType as WireKind;
use bedrock_protocol_1_26_51::packets::PrimitiveShapes;
use bedrock_protocol_1_26_51::types::{
    MceColor, PrimitiveShape, PrimitiveShapeExtraShapeData as Extra,
};
use bedrock_protocol_1_26_51::wire;
use render_api::primitive_shapes::{
    PrimitiveShapeChange, PrimitiveShapeData, PrimitiveShapeKind, PrimitiveShapeUpdate,
    PrimitiveShapesEvent, PrimitiveText,
};

/// Normalizes each decoded entry while preserving packet order.
pub(crate) fn normalize(packet: PrimitiveShapes) -> PrimitiveShapesEvent {
    let entries = packet.array_of_primitive_shapes_can_be_a_mix_of_new_updated_or_removed;
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
fn normalize_entry(entry: PrimitiveShape) -> Option<PrimitiveShapeChange> {
    let Some(wire_kind) = entry.shape_type else {
        return Some(PrimitiveShapeChange::Remove {
            network_id: entry.network_id.0,
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
    let location = entry.location.map(|value| value.to_array());
    let rotation = entry.rotation.map(|value| value.to_array());
    let scale = float(entry.scale);
    let total_time_left = float(entry.total_time_left);
    let maximum_render_distance = float(entry.maximum_render_distance);
    if !location.is_none_or(finite_vector)
        || !rotation.is_none_or(finite_vector)
        || !scale.is_none_or(f32::is_finite)
        || !total_time_left.is_none_or(f32::is_finite)
        || !maximum_render_distance.is_none_or(f32::is_finite)
    {
        return None;
    }
    let data = normalize_data(entry.extra_shape_data)?;
    Some(PrimitiveShapeChange::Upsert(PrimitiveShapeUpdate {
        network_id: entry.network_id.0,
        kind,
        location,
        rotation,
        scale,
        color: entry.color.map(color),
        total_time_left,
        maximum_render_distance,
        dimension: entry.dimension_id.map(|dimension| dimension.0),
        attached_actor: entry.attached_to_entity_id.map(|actor| actor.0),
        data,
    }))
}

/// Preserves recognized extra data so the store can match the id's retained concrete kind.
fn normalize_data(extra: Extra) -> Option<PrimitiveShapeData> {
    let data = match extra {
        Extra::LineData { line_end_location } => {
            let end = line_end_location.to_array();
            finite_vector(end).then_some(())?;
            PrimitiveShapeData::Line { end }
        }
        Extra::BoxData { box_bound } => {
            let bounds = box_bound.to_array();
            finite_vector(bounds).then_some(())?;
            PrimitiveShapeData::Box { bounds }
        }
        Extra::SphereData { num_segments } => PrimitiveShapeData::Segments(num_segments.0),
        Extra::ArrowData {
            arrow_end_location,
            arrow_head_length,
            arrow_head_radius,
            num_segments,
        } => {
            let end = arrow_end_location.map(|value| value.to_array());
            let head_length = float(arrow_head_length);
            let head_radius = float(arrow_head_radius);
            if !end.is_none_or(finite_vector)
                || !head_length.is_none_or(f32::is_finite)
                || !head_radius.is_none_or(f32::is_finite)
            {
                return None;
            }
            PrimitiveShapeData::Arrow {
                end,
                head_length,
                head_radius,
                segments: num_segments.map(|segments| segments.0),
            }
        }
        Extra::TextData {
            text,
            use_rotation,
            background_color,
            line_gap_height,
            depth_test,
            show_backface,
            show_text_backface,
        } => {
            if !line_gap_height.0.is_finite() || text.len() > crate::MAX_UI_TEXT_BYTES {
                return None;
            }
            PrimitiveShapeData::Text(PrimitiveText {
                text: text.to_string_lossy().into(),
                use_rotation,
                background_color: background_color.map(color),
                line_gap_height: line_gap_height.0,
                depth_test,
                show_backface,
                show_text_backface,
            })
        }
        _ => PrimitiveShapeData::None,
    };
    Some(data)
}

/// Unwraps an optional little-endian float field.
fn float(value: Option<wire::F32LE>) -> Option<f32> {
    value.map(|value| value.0)
}

/// Rejects non-finite positions before they can poison GPU transforms.
fn finite_vector(value: [f32; 3]) -> bool {
    value.into_iter().all(f32::is_finite)
}

/// Converts the packed ARGB integer without changing its alpha.
fn color(value: MceColor) -> [f32; 4] {
    let [blue, green, red, alpha] = value.0.to_le_bytes();
    [red, green, blue, alpha].map(|channel| f32::from(channel) / 255.0)
}

#[cfg(test)]
mod tests;
