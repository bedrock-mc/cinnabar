use super::*;
use bytes::Buf;
use jolyne::raw::decode_packet_raw;
use valentine::bedrock::{context::BedrockSession, version::v1_26_51::Vec3};

#[test]
fn primitive_shapes_raw_ingress_retains_valid_changes_and_counts_odd_entries() {
    use valentine::bedrock::version::v1_26_51::{
        EnumsScriptModuleMinecraftScriptPrimitiveShapeType as Kind, LineDataPayload,
        PrimitiveShapeDataPayload, PrimitiveShapeDataPayloadExtraShapeData as Extra,
        PrimitiveShapesPacket,
    };

    let session = BedrockSession { shield_item_id: 0 };
    let packet: Packet = PrimitiveShapesPacket {
        arrayofprimitiveshapescanbeamixofnewupdatedorremoved: vec![
            PrimitiveShapeDataPayload {
                network_id: 7,
                shape_type: Some(Kind::Line),
                extra_shape_data: Extra::LineDataPayload(LineDataPayload {
                    line_end_location: Vec3 {
                        x: 1.0,
                        y: 2.0,
                        z: 3.0,
                    },
                }),
                ..Default::default()
            },
            PrimitiveShapeDataPayload {
                network_id: 8,
                shape_type: Some(Kind::Box),
                scale: Some(f32::NAN),
                ..Default::default()
            },
            PrimitiveShapeDataPayload {
                network_id: 9,
                shape_type: Some(Kind::Unknown(255)),
                ..Default::default()
            },
            PrimitiveShapeDataPayload {
                network_id: 7,
                ..Default::default()
            },
        ],
    }
    .into();
    let mut encoded = crate::encode(&packet, &session).expect("encoded shape packet");
    encoded.advance(1);
    let raw = decode_packet_raw(&mut encoded).expect("raw shape packet");
    let event = decode_world_raw_with(raw, 0, |raw| raw.decode(&session))
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
