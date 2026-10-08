use protocol::{
    AbilityLayersEvidence, MAX_ABILITY_LAYERS, decode_abilities_update, into_world_event,
};
use valentine::bedrock::codec::{BedrockCodec, VarUInt};
use valentine::bedrock::version::v1_26_51::{
    SerializedAbilitiesData, SerializedAbilitiesDataSerializedLayer, UpdateAbilitiesPacket,
};

#[test]
fn local_ability_packet_is_not_silently_discarded() {
    let packet = UpdateAbilitiesPacket {
        data: SerializedAbilitiesData {
            target_player_raw_id: 42,
            layers: vec![SerializedAbilitiesDataSerializedLayer {
                serialized_layer: 1,
                abilities_set: 3,
                ability_values: 2,
                ..Default::default()
            }],
            ..Default::default()
        },
    };
    assert!(
        into_world_event(packet.into(), 0).unwrap().is_some(),
        "well-framed ability evidence must reach the committed world pipeline"
    );
}

fn body(owner: i64, count: u32) -> Vec<u8> {
    let mut body = owner.to_le_bytes().to_vec();
    body.extend_from_slice(&[0xff, 0xfe]);
    VarUInt(count).encode(&mut body).unwrap();
    for index in 0..count {
        body.extend_from_slice(&(index as u16).to_le_bytes());
        body.extend_from_slice(&(u32::MAX - index).to_le_bytes());
        body.extend_from_slice(&index.to_le_bytes());
        for bits in [0x7fc0_1234u32, 0x8000_0000, 0x3dcc_cccd] {
            body.extend_from_slice(&bits.to_le_bytes());
        }
    }
    body
}

#[test]
fn raw_order_masks_unknown_permissions_and_float_bits_are_exact_passive_evidence() {
    let update = decode_abilities_update(&body(-17, 3)).unwrap();
    assert_eq!(
        (
            update.actor_unique_id,
            update.player_permission,
            update.command_permission
        ),
        (-17, -1, 254)
    );
    let AbilityLayersEvidence::Received(layers) = update.layers else {
        panic!("bounded evidence");
    };
    assert_eq!(layers.len(), 3);
    for (index, layer) in layers.iter().enumerate() {
        assert_eq!(layer.layer_type, index as u16);
        assert_eq!(layer.abilities, u32::MAX - index as u32);
        assert_eq!(layer.values, index as u32);
        assert_eq!(
            [
                layer.fly_speed_bits,
                layer.vertical_fly_speed_bits,
                layer.walk_speed_bits
            ],
            [0x7fc0_1234, 0x8000_0000, 0x3dcc_cccd]
        );
    }
}

#[test]
fn empty_max_and_over_policy_are_distinct_correctly_framed_evidence() {
    for count in [0, MAX_ABILITY_LAYERS as u32] {
        assert!(
            matches!(decode_abilities_update(&body(0, count)).unwrap().layers, AbilityLayersEvidence::Received(ref layers) if layers.len() == count as usize)
        );
    }
    let update = decode_abilities_update(&body(0, MAX_ABILITY_LAYERS as u32 + 1)).unwrap();
    assert_eq!(update.actor_unique_id, 0);
    assert_eq!(
        update.layers,
        AbilityLayersEvidence::Unavailable {
            declared_layers: 33
        }
    );
}

#[test]
fn owned_and_raw_paths_preserve_identical_unknown_values_and_layer_order() {
    use valentine::bedrock::version::v1_26_51::{
        EnumsCommandPermissionLevel, EnumsPlayerPermissionLevel,
    };
    for count in [0, 32, 33] {
        let packet = UpdateAbilitiesPacket {
            data: SerializedAbilitiesData {
                target_player_raw_id: -17,
                player_permissions: EnumsPlayerPermissionLevel::Unknown(-1),
                command_permissions: EnumsCommandPermissionLevel::Unknown(254),
                layers: (0..count)
                    .map(|index| SerializedAbilitiesDataSerializedLayer {
                        serialized_layer: index,
                        abilities_set: u32::MAX - u32::from(index),
                        ability_values: u32::from(index),
                        fly_speed: f32::from_bits(0x7fc0_1234),
                        vertical_fly_speed: f32::from_bits(0x8000_0000),
                        walk_speed: f32::from_bits(0x3dcc_cccd),
                    })
                    .collect(),
            },
        };
        let mut encoded = Vec::new();
        packet.encode(&mut encoded).unwrap();
        assert_eq!(
            into_world_event(packet.into(), 0).unwrap(),
            Some(protocol::WorldEvent::Abilities(
                decode_abilities_update(&encoded).unwrap()
            ))
        );
    }
}

#[test]
fn every_truncation_tail_and_count_overflow_remains_a_wire_failure_even_over_policy() {
    for count in [1, 33] {
        let complete = body(42, count);
        for prefix in 0..complete.len() {
            assert!(
                decode_abilities_update(&complete[..prefix]).is_err(),
                "prefix {prefix} count {count}"
            );
        }
        let mut trailing = complete;
        trailing.push(0);
        assert!(decode_abilities_update(&trailing).is_err());
    }
    let mut oversized = body(42, 0);
    oversized.truncate(10);
    oversized.extend_from_slice(&[0xff, 0xff, 0xff, 0xff, 0xff, 0x01]);
    assert!(decode_abilities_update(&oversized).is_err());
    oversized.truncate(10);
    oversized.extend_from_slice(&[0xff, 0xff, 0xff, 0xff, 0x0f]);
    assert!(
        decode_abilities_update(&oversized).is_err(),
        "valid huge count is truncated, not unavailable"
    );
    oversized.truncate(10);
    oversized.extend_from_slice(&[0x80, 0]);
    assert!(
        decode_abilities_update(&oversized).is_err(),
        "noncanonical count is malformed"
    );
}
