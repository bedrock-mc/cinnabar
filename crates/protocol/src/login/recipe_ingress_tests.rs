#[test]
fn empty_clear_recipe_packet_is_published_without_generic_materialization() {
    let mut bytes = bytes::Bytes::from_static(&[13, 52, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    let raw = crate::raw_packet::decode_packet_raw(&mut bytes).unwrap();
    let event = super::decode_world_raw_with(raw, 0, |_| {
        panic!("recipe ingress must not call the generic decoder")
    })
    .unwrap();
    assert!(
        matches!(event, Some(crate::WorldEvent::Inventory(_))),
        "an empty clear update must retire old recipe authority"
    );
}

#[test]
fn recipe_unlocks_reach_the_world_inventory_consumer() {
    let packet = crate::Packet::new(bedrock_protocol_1_26_51::packets::UnlockedRecipes {
        packet_type: bedrock_protocol_1_26_51::enums::PacketType::Initiallyunlockedrecipes,
        unlocked_recipes_list: vec!["test:recipe".into()],
    });
    let mut bytes = crate::encode(&packet).unwrap();
    bytes::Buf::advance(&mut bytes, 1);
    let raw = crate::raw_packet::decode_packet_raw(&mut bytes).unwrap();
    let event = super::decode_world_raw_with(raw, 0, |_| {
        panic!("recipe unlock ingress must use its bounded decoder")
    })
    .unwrap();
    assert!(
        matches!(event, Some(crate::WorldEvent::Inventory(_))),
        "recipe unlock packets must reach ordered inventory ingress"
    );
}

#[test]
fn recipe_unlocks_are_allowlisted_and_both_normalizers_agree() {
    use bedrock_protocol_1_26_51::packets::{PacketId, UnlockedRecipes};
    assert!(crate::world::routes_world_packet(PacketId::UnlockedRecipes));
    let packet = crate::Packet::new(UnlockedRecipes {
        packet_type: u32::MAX.into(),
        unlocked_recipes_list: vec!["test:A".into(), "test:A".into(), "test:B".into()],
    });
    let normalized = crate::into_world_event(packet.clone(), 0).unwrap();
    let mut bytes = crate::encode(&packet).unwrap();
    bytes::Buf::advance(&mut bytes, 1);
    let raw = crate::raw_packet::decode_packet_raw(&mut bytes).unwrap();
    assert_eq!(
        super::decode_world_raw_with(raw, 0, super::decode_test_raw).unwrap(),
        normalized
    );
}
