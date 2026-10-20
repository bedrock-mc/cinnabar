use bedrock_protocol_1_26_51::{packets::UpdateTrade, wire};

#[test]
fn update_trade_reaches_inventory_without_a_container_open() {
    let packet = crate::Packet::new(UpdateTrade {
        container_id: wire::U8(7),
        type_: wire::U8(protocol_ids::WindowType::Trade.wire_value() as u8),
        display_name: "Trader".into(),
        use_new_trade_screen: true,
        data: wire::NetworkNbt(vec![10, 0, 0].into()),
        ..Default::default()
    });
    assert!(matches!(
        crate::into_world_event(packet, 0).unwrap(),
        Some(crate::WorldEvent::Inventory(_))
    ));
}

/// Encodes one original named network-NBT field for compact protocol fixtures.
fn field(tag: u8, name: &str, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![tag];
    let mut writer = wire::Writer::new(&mut bytes);
    writer.write_str(name.as_bytes()).unwrap();
    writer.write_all(payload).unwrap();
    bytes
}

/// Encodes an original item compound with a custom tag and explicit saved count.
fn item(count: u8, tagged: bool) -> Vec<u8> {
    let mut text = Vec::new();
    wire::Writer::new(&mut text)
        .write_str(b"minecraft:apple")
        .unwrap();
    let mut bytes = field(8, "Name", &text);
    bytes.extend(field(1, "Count", &[count]));
    bytes.extend(field(2, "Damage", &3_i16.to_le_bytes()));
    if tagged {
        let mut tag = field(7, "custom", &[6, 1, 2, 255]);
        tag.extend(field(11, "numbers", &[4, 1, 6]));
        tag.extend(field(9, "empty", &[4, 0]));
        tag.push(0);
        bytes.extend(field(10, "tag", &tag));
        bytes.extend(field(10, "Block", &[0]));
    }
    bytes.push(0);
    bytes
}

#[test]
fn trading_keeps_adjusted_counts_recipe_identity_and_every_item_tag() {
    let mut offer = field(10, "buyA", &item(17, false));
    offer.extend(field(10, "buyB", &item(1, false)));
    offer.extend(field(10, "sell", &item(1, true)));
    offer.extend(field(3, "netId", &[182, 1]));
    offer.extend(field(3, "tier", &[2]));
    offer.extend(field(3, "uses", &[4]));
    offer.extend(field(3, "maxUses", &[24]));
    offer.push(0);
    let mut recipes = vec![10, 2];
    recipes.extend(offer);
    let mut root = vec![10, 0];
    root.extend(field(9, "Recipes", &recipes));
    root.push(0);
    let trade = super::normalize_trade(&UpdateTrade {
        container_id: wire::U8(7),
        type_: wire::U8(protocol_ids::WindowType::Trade.wire_value() as u8),
        data: wire::NetworkNbt(root.into()),
        ..Default::default()
    })
    .unwrap();
    let offer = &trade.offers[0];
    assert_eq!(offer.network_id, 91);
    assert_eq!(offer.inputs[0].as_ref().unwrap().count, 17);
    assert_eq!(offer.inputs[1].as_ref().unwrap().count, 1);
    assert_eq!((offer.tier, offer.uses, offer.max_uses), (1, 2, 12));
    let mut expected = vec![255, 255, 1, 10, 0, 0];
    for (tag, name, payload) in [
        (7, "custom", vec![3, 0, 0, 0, 1, 2, 255]),
        (
            11,
            "numbers",
            vec![2, 0, 0, 0, 255, 255, 255, 255, 3, 0, 0, 0],
        ),
        (9, "empty", vec![4, 0, 0, 0, 0]),
    ] {
        expected.push(tag);
        expected.extend_from_slice(&(name.len() as u16).to_le_bytes());
        expected.extend_from_slice(name.as_bytes());
        expected.extend(payload);
    }
    expected.extend_from_slice(&[0; 9]);
    assert_eq!(&*offer.output.extra_data, expected);
    assert_eq!(offer.output.metadata, 3);
    assert_eq!(offer.output.block_descriptor.as_deref(), Some(&[0][..]));
}
