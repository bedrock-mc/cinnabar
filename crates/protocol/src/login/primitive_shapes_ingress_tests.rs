use super::*;
use crate::raw_packet::decode_packet_raw;
use bytes::Buf;

#[test]
fn primitive_shapes_raw_ingress_retains_valid_changes_and_counts_odd_entries() {
    use bedrock_protocol_1_26_51::enums::ScriptModuleMinecraftScriptPrimitiveShapeType as Kind;
    use bedrock_protocol_1_26_51::packets::PrimitiveShapes;
    use bedrock_protocol_1_26_51::types::{PrimitiveShape, PrimitiveShapeExtraShapeData as Extra};

    let packet: Packet = crate::Packet::new(PrimitiveShapes {
        array_of_primitive_shapes_can_be_a_mix_of_new_updated_or_removed: vec![
            PrimitiveShape {
                network_id: wire::VarULong(7),
                shape_type: Some(Kind::Line),
                extra_shape_data: Extra::LineData {
                    line_end_location: [1.0, 2.0, 3.0].into(),
                },
                ..Default::default()
            },
            PrimitiveShape {
                network_id: wire::VarULong(8),
                shape_type: Some(Kind::Box),
                scale: Some(wire::F32LE(f32::NAN)),
                ..Default::default()
            },
            PrimitiveShape {
                network_id: wire::VarULong(9),
                shape_type: Some(Kind::Unknown(255)),
                ..Default::default()
            },
            PrimitiveShape {
                network_id: wire::VarULong(7),
                ..Default::default()
            },
        ],
    });
    let mut encoded = crate::encode(&packet).expect("encoded shape packet");
    encoded.advance(1);
    let raw = decode_packet_raw(&mut encoded).expect("raw shape packet");
    let event = decode_world_raw_with(raw, 0, super::decode_test_raw)
        .expect("shape wire decode")
        .expect("shape event admitted from raw ingress");
    let WorldEvent::PrimitiveShapes(event) = event else {
        panic!("wrong event kind");
    };
    assert_eq!(event.skipped_entries, 2);
    assert_eq!(event.changes.len(), 2);
    let crate::PrimitiveShapeChange::Upsert(shape) = &event.changes[0] else {
        panic!("missing shape creation");
    };
    assert_eq!(shape.network_id, 7);
    assert_eq!(
        shape.data,
        crate::PrimitiveShapeData::Line {
            end: [1.0, 2.0, 3.0]
        }
    );
    assert_eq!(
        event.changes[1],
        crate::PrimitiveShapeChange::Remove { network_id: 7 }
    );
}
