use super::*;
use bedrock_protocol_1_26_51::types::{ActorUniqueID, DimensionType};
use bedrock_protocol_1_26_51::wire::{Decode, Encode};

/// Encodes and decodes real wire entries before exercising event normalization.
fn through_wire(entries: Vec<PrimitiveShape>) -> PrimitiveShapesEvent {
    let packet = PrimitiveShapes {
        array_of_primitive_shapes_can_be_a_mix_of_new_updated_or_removed: entries,
    };
    let bytes = packet.encode_to_vec().unwrap();
    normalize(PrimitiveShapes::decode_exact(&bytes).unwrap())
}

/// Supplies only the identity fields required to create or patch one primitive.
fn entry(network_id: u64, kind: WireKind) -> PrimitiveShape {
    PrimitiveShape {
        network_id: wire::VarULong(network_id),
        shape_type: Some(kind),
        ..Default::default()
    }
}

/// A text payload with default flags and the given line gap.
fn text_data(text: &str, line_gap_height: f32, depth_test: bool) -> Extra {
    Extra::TextData {
        text: text.into(),
        use_rotation: false,
        background_color: None,
        line_gap_height: wire::F32LE(line_gap_height),
        depth_test,
        show_backface: false,
        show_text_backface: false,
    }
}

#[test]
fn primitive_shapes_packet_becomes_a_world_event() {
    let packet = PrimitiveShapes {
        array_of_primitive_shapes_can_be_a_mix_of_new_updated_or_removed: vec![entry(
            7,
            WireKind::Line,
        )],
    };
    let result = crate::into_world_event(crate::Packet::new(packet), 0).unwrap();
    let Some(crate::WorldEvent::PrimitiveShapes(event)) = result else {
        panic!("primitive shapes packet was not dispatched");
    };
    assert_eq!(event.changes.len(), 1);
}

#[test]
fn optional_fields_and_removal_survive_wire_round_trip() {
    let mut shape = entry(7, WireKind::Arrow);
    shape.location = Some([1.0, 2.0, 3.0].into());
    shape.rotation = Some([10.0, 20.0, 30.0].into());
    shape.scale = Some(wire::F32LE(2.0));
    shape.total_time_left = Some(wire::F32LE(0.0));
    shape.maximum_render_distance = Some(wire::F32LE(-1.0));
    shape.color = Some(MceColor(i32::from_le_bytes([0, 128, 255, 64])));
    shape.dimension_id = Some(DimensionType(42));
    shape.attached_to_entity_id = Some(ActorUniqueID(-1));
    shape.extra_shape_data = Extra::ArrowData {
        arrow_end_location: Some([4.0, 5.0, 6.0].into()),
        arrow_head_length: Some(wire::F32LE(0.5)),
        arrow_head_radius: None,
        num_segments: Some(wire::U8(8)),
    };
    let removal = PrimitiveShape {
        network_id: wire::VarULong(7),
        scale: Some(wire::F32LE(f32::NAN)),
        ..Default::default()
    };
    let event = through_wire(vec![shape, entry(7, WireKind::Arrow), removal]);
    assert_eq!(event.skipped_entries, 0);
    let PrimitiveShapeChange::Upsert(shape) = &event.changes[0] else {
        panic!("missing upsert")
    };
    assert_eq!(shape.location, Some([1.0, 2.0, 3.0]));
    assert_eq!(shape.color, Some([1.0, 128.0 / 255.0, 0.0, 64.0 / 255.0]));
    assert_eq!(shape.dimension, Some(42));
    assert_eq!(shape.attached_actor, Some(-1));
    assert_eq!(shape.total_time_left, Some(0.0));
    assert_eq!(shape.maximum_render_distance, Some(-1.0));
    let PrimitiveShapeChange::Upsert(patch) = &event.changes[1] else {
        panic!("missing patch")
    };
    assert_eq!(patch.location, None);
    assert_eq!(patch.data, PrimitiveShapeData::None);
    assert_eq!(
        event.changes[2],
        PrimitiveShapeChange::Remove { network_id: 7 }
    );
}

#[test]
fn all_six_geometry_payloads_are_typed() {
    let cases = [
        (
            WireKind::Line,
            Extra::LineData {
                line_end_location: Default::default(),
            },
        ),
        (
            WireKind::Box,
            Extra::BoxData {
                box_bound: [1.0, 2.0, 3.0].into(),
            },
        ),
        (
            WireKind::Sphere,
            Extra::SphereData {
                num_segments: wire::U8(render_api::primitive_shapes::PRIMITIVE_DEFAULT_SEGMENTS),
            },
        ),
        (
            WireKind::Circle,
            Extra::SphereData {
                num_segments: wire::U8(255),
            },
        ),
        (WireKind::Text, text_data("debug", 0.0, true)),
        (
            WireKind::Arrow,
            Extra::ArrowData {
                arrow_end_location: None,
                arrow_head_length: None,
                arrow_head_radius: None,
                num_segments: None,
            },
        ),
    ];
    let entries = cases
        .into_iter()
        .enumerate()
        .map(|(id, (kind, data))| {
            let mut shape = entry(id as u64, kind);
            shape.extra_shape_data = data;
            shape
        })
        .collect();
    let event = through_wire(entries);
    assert_eq!(event.skipped_entries, 0);
    assert_eq!(event.changes.len(), 6);
    for change in event.changes {
        let PrimitiveShapeChange::Upsert(shape) = change else {
            panic!("missing upsert")
        };
        assert_ne!(shape.data, PrimitiveShapeData::None);
    }
}

#[test]
fn odd_entries_are_counted_without_dropping_valid_neighbors() {
    let mut invalid_position = entry(1, WireKind::Line);
    invalid_position.location = Some([f32::NAN, 0.0, 0.0].into());
    let mut invalid_endpoint = entry(2, WireKind::Line);
    invalid_endpoint.extra_shape_data = Extra::LineData {
        line_end_location: [0.0, f32::INFINITY, 0.0].into(),
    };
    let mut invalid_text = entry(3, WireKind::Text);
    invalid_text.extra_shape_data = text_data("", f32::NAN, false);
    let event = through_wire(vec![
        entry(0, WireKind::Box),
        invalid_position,
        invalid_endpoint,
        invalid_text,
        entry(4, WireKind::Unknown(255)),
        entry(5, WireKind::Cone),
        entry(6, WireKind::Sphere),
    ]);
    assert_eq!(event.skipped_entries, 5);
    assert_eq!(event.changes.len(), 2);
}

#[test]
fn truncated_packet_framing_is_still_fatal() {
    let packet = PrimitiveShapes {
        array_of_primitive_shapes_can_be_a_mix_of_new_updated_or_removed: vec![entry(
            7,
            WireKind::Line,
        )],
    };
    let mut bytes = packet.encode_to_vec().unwrap();
    bytes.pop();
    assert!(PrimitiveShapes::decode_exact(&bytes).is_err());
}

#[test]
fn extra_payload_survives_a_different_declared_kind() {
    let mut shape = entry(8, WireKind::Sphere);
    shape.extra_shape_data = Extra::BoxData {
        box_bound: [2.0, 3.0, 4.0].into(),
    };
    let event = through_wire(vec![shape]);
    assert_eq!(event.skipped_entries, 0);
    let PrimitiveShapeChange::Upsert(update) = &event.changes[0] else {
        panic!("missing upsert");
    };
    assert_eq!(update.kind, PrimitiveShapeKind::Sphere);
    assert_eq!(
        update.data,
        PrimitiveShapeData::Box {
            bounds: [2.0, 3.0, 4.0]
        }
    );
}
