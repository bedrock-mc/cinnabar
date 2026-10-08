use super::*;
use valentine::bedrock::codec::BedrockCodec;
use valentine::bedrock::version::v1_26_51::{
    ActorUniqueId, ArrowDataPayload, BoxDataPayload, DimensionType, LineDataPayload,
    SphereDataPayload, TextDataPayload,
};

/// Encodes and decodes real wire entries before exercising event normalization.
fn through_wire(entries: Vec<PrimitiveShapeDataPayload>) -> PrimitiveShapesEvent {
    let packet = PrimitiveShapesPacket {
        arrayofprimitiveshapescanbeamixofnewupdatedorremoved: entries,
    };
    let mut bytes = Vec::new();
    packet.encode(&mut bytes).unwrap();
    normalize(PrimitiveShapesPacket::decode(&mut bytes.as_slice(), ()).unwrap())
}

/// Supplies only the identity fields required to create or patch one primitive.
fn entry(network_id: u64, kind: WireKind) -> PrimitiveShapeDataPayload {
    PrimitiveShapeDataPayload {
        network_id,
        shape_type: Some(kind),
        ..Default::default()
    }
}

#[test]
fn primitive_shapes_packet_becomes_a_world_event() {
    let packet = PrimitiveShapesPacket {
        arrayofprimitiveshapescanbeamixofnewupdatedorremoved: vec![entry(7, WireKind::Line)],
    };
    let result = crate::into_world_event(packet.into(), 0).unwrap();
    let Some(crate::WorldEvent::PrimitiveShapes(event)) = result else {
        panic!("primitive shapes packet was not dispatched");
    };
    assert_eq!(event.changes.len(), 1);
}

#[test]
fn optional_fields_and_removal_survive_wire_round_trip() {
    let mut shape = entry(7, WireKind::Arrow);
    shape.location = Some(Vec3 {
        x: 1.0,
        y: 2.0,
        z: 3.0,
    });
    shape.rotation = Some(Vec3 {
        x: 10.0,
        y: 20.0,
        z: 30.0,
    });
    shape.scale = Some(2.0);
    shape.total_time_left = Some(0.0);
    shape.maximum_render_distance = Some(-1.0);
    shape.color = Some(MceColor {
        color: i32::from_le_bytes([0, 128, 255, 64]),
    });
    shape.dimension_id = Some(DimensionType { value: 42 });
    shape.attached_to_entity_id = Some(ActorUniqueId {
        actor_unique_id: -1,
    });
    shape.extra_shape_data = Extra::ArrowDataPayload(Box::new(ArrowDataPayload {
        arrow_end_location: Some(Vec3 {
            x: 4.0,
            y: 5.0,
            z: 6.0,
        }),
        arrow_head_length: Some(0.5),
        arrow_head_radius: None,
        num_segments: Some(8),
    }));
    let removal = PrimitiveShapeDataPayload {
        network_id: 7,
        scale: Some(f32::NAN),
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
            Extra::LineDataPayload(LineDataPayload {
                line_end_location: Vec3::default(),
            }),
        ),
        (
            WireKind::Box,
            Extra::BoxDataPayload(BoxDataPayload {
                box_bound: Vec3 {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                },
            }),
        ),
        (
            WireKind::Sphere,
            Extra::SphereDataPayload(SphereDataPayload {
                num_segments: render_api::primitive_shapes::PRIMITIVE_DEFAULT_SEGMENTS,
            }),
        ),
        (
            WireKind::Circle,
            Extra::SphereDataPayload(SphereDataPayload { num_segments: 255 }),
        ),
        (
            WireKind::Text,
            Extra::TextDataPayload(Box::new(TextDataPayload {
                text: "debug".into(),
                depth_test: true,
                ..Default::default()
            })),
        ),
        (WireKind::Arrow, Extra::ArrowDataPayload(Box::default())),
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
    invalid_position.location = Some(Vec3 {
        x: f32::NAN,
        ..Default::default()
    });
    let mut invalid_endpoint = entry(2, WireKind::Line);
    invalid_endpoint.extra_shape_data = Extra::LineDataPayload(LineDataPayload {
        line_end_location: Vec3 {
            y: f32::INFINITY,
            ..Default::default()
        },
    });
    let mut invalid_text = entry(3, WireKind::Text);
    invalid_text.extra_shape_data = Extra::TextDataPayload(Box::new(TextDataPayload {
        line_gap_height: f32::NAN,
        ..Default::default()
    }));
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
    let packet = PrimitiveShapesPacket {
        arrayofprimitiveshapescanbeamixofnewupdatedorremoved: vec![entry(7, WireKind::Line)],
    };
    let mut bytes = Vec::new();
    packet.encode(&mut bytes).unwrap();
    bytes.pop();
    assert!(PrimitiveShapesPacket::decode(&mut bytes.as_slice(), ()).is_err());
}

#[test]
fn extra_payload_survives_a_different_declared_kind() {
    let mut shape = entry(8, WireKind::Sphere);
    shape.extra_shape_data = Extra::BoxDataPayload(BoxDataPayload {
        box_bound: Vec3 {
            x: 2.0,
            y: 3.0,
            z: 4.0,
        },
    });
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
