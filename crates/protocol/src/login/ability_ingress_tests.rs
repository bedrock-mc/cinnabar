use bedrock_protocol_1_26_51::packets::UpdateAbilities;
use bedrock_protocol_1_26_51::types::{
    SerializedAbilitiesData, SerializedAbilitiesDataSerializedLayer,
};
use bedrock_protocol_1_26_51::wire;

fn raw(count: usize) -> crate::raw_packet::RawPacket {
    let packet: crate::Packet = crate::Packet::new(UpdateAbilities {
        data: SerializedAbilitiesData {
            target_player_raw_id: wire::I64LE(42),
            layers: vec![SerializedAbilitiesDataSerializedLayer::default(); count],
            ..Default::default()
        },
    });
    let mut frame = Vec::new();
    crate::codec::encode_entry(&packet, &mut frame).unwrap();
    crate::raw_packet::decode_packet_raw(&mut bytes::Bytes::from(frame)).unwrap()
}

#[test]
fn real_raw_ingress_preserves_empty_and_over_policy_without_generic_materialization() {
    for count in [0, 33] {
        let event = super::decode_world_raw_with(raw(count), 0, |_| {
            panic!("bounded raw decoder owns abilities")
        })
        .unwrap();
        let Some(crate::WorldEvent::Abilities(update)) = event else {
            panic!("published evidence");
        };
        assert_eq!(update.actor_unique_id, 42);
        match update.layers {
            crate::AbilityLayersEvidence::Received(layers) => assert_eq!(count, layers.len()),
            crate::AbilityLayersEvidence::Unavailable { declared_layers } => {
                assert_eq!(declared_layers, count as u32)
            }
        }
    }
}

#[test]
fn malformed_raw_ability_frame_is_not_a_skippable_semantic_packet() {
    let raw = raw(33);
    let mut frame = raw.inner_frame().to_vec();
    frame.pop();
    // Decode the unchanged complete frame to retain real packet header ownership,
    // then exercise body framing through the same decoder used by raw ingress.
    let error = crate::decode_abilities_update(&raw.body()[..raw.body().len() - 1]).unwrap_err();
    let mut skipped = 0;
    assert!(super::skip_semantic_world_error(error, &mut skipped).is_err());
    assert_eq!(skipped, 0);
    assert!(crate::raw_packet::decode_packet_raw(&mut bytes::Bytes::from(frame)).is_err());
}
