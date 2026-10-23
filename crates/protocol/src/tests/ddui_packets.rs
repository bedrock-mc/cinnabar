use std::sync::Arc;

use bedrock_protocol_1_26_51::packets::{
    self, ClientboundDataDrivenUICloseScreen, ClientboundDataDrivenUIReload,
    ClientboundDataDrivenUIShowScreen, ClientboundDataStore, ServerboundDataDrivenScreenClosed,
    ServerboundDataStore,
};
use bedrock_protocol_1_26_51::types::{
    BedrockDDUI, BedrockDDUIDataStoreUpdate, BedrockDDUIDataStoreUpdateData, DynamicValue,
};
use bedrock_protocol_1_26_51::wire;
use protocol::ddui::{
    DataDrivenScreenCloseReason, DataDrivenUiEvent, DataPrimitive, DataStores, DataValue,
    ShowDataDrivenScreen, StoreApply, data_driven_screen_closed_packet, data_store_update_packet,
};
use protocol::{UiEvent, WorldEvent, decode_batch, into_world_event};

fn ddui<P>(packet: P) -> DataDrivenUiEvent
where
    packets::Packet: From<P>,
{
    match into_world_event(protocol::Packet::new(packet), 0) {
        Ok(Some(WorldEvent::Ui(UiEvent::DataDrivenUi(event)))) => event,
        other => panic!("expected a DDUI event, got {other:?}"),
    }
}

/// Re-decodes an outgoing packet through the codec, as the server would read it.
fn sent(packet: &protocol::Packet) -> packets::Packet {
    let mut packets = decode_batch(protocol::encode(packet).unwrap()).unwrap();
    assert_eq!(packets.len(), 1);
    *packets.pop().unwrap().data
}

fn string(value: &str) -> DynamicValue {
    DynamicValue::String(value.into())
}

/// A scripted form property shaped like the vanilla server's: an array-shaped `layout` map.
fn form_property() -> DynamicValue {
    let control = |entries: Vec<(&str, DynamicValue)>| {
        DynamicValue::Map(entries.into_iter().map(|(k, v)| (k.into(), v)).collect())
    };
    control(vec![
        ("title", string("Title")),
        (
            "layout",
            control(vec![
                ("length", DynamicValue::Int64(wire::I64LE(2))),
                (
                    "0",
                    control(vec![
                        ("label_visible", DynamicValue::Bool(true)),
                        ("text", string("Clicks: 0")),
                    ]),
                ),
                (
                    "1",
                    control(vec![
                        ("slider_visible", DynamicValue::Bool(true)),
                        ("value", DynamicValue::Double(wire::F64LE(4.0))),
                    ]),
                ),
            ]),
        ),
    ])
}

fn server_update(
    path: &str,
    data: BedrockDDUIDataStoreUpdateData,
    counts: [u32; 2],
) -> BedrockDDUI {
    BedrockDDUI::DataStoreUpdate(BedrockDDUIDataStoreUpdate {
        data_store_name: "minecraft".into(),
        property: "custom_form_data_7".into(),
        path: path.into(),
        data,
        property_update_count: wire::U32LE(counts[0]),
        path_update_count: wire::U32LE(counts[1]),
    })
}

fn apply_all(stores: &mut DataStores, packet: ClientboundDataStore) -> Vec<StoreApply> {
    let DataDrivenUiEvent::Store(changes) = ddui(packet) else {
        panic!("expected store changes");
    };
    changes
        .into_iter()
        .map(|change| stores.apply(change))
        .collect()
}

#[test]
fn screen_packets_normalize_to_show_close_and_reload() {
    assert_eq!(
        ddui(ClientboundDataDrivenUIShowScreen {
            screen_id: "minecraft:custom_form".into(),
            form_id: wire::U32LE(3),
            data_instance_id: Some(wire::U32LE(4_271_100_270)),
        }),
        DataDrivenUiEvent::Show(ShowDataDrivenScreen {
            screen_id: Arc::from("minecraft:custom_form"),
            form_id: 3,
            data_instance_id: Some(4_271_100_270),
        })
    );
    assert_eq!(
        ddui(ClientboundDataDrivenUICloseScreen { form_id: None }),
        DataDrivenUiEvent::Close { form_id: None }
    );
    assert_eq!(
        ddui(ClientboundDataDrivenUICloseScreen {
            form_id: Some(wire::U32LE(3)),
        }),
        DataDrivenUiEvent::Close { form_id: Some(3) }
    );
    assert_eq!(
        ddui(ClientboundDataDrivenUIReload {}),
        DataDrivenUiEvent::Reload
    );
}

#[test]
fn store_updates_apply_only_against_the_current_property_replacement() {
    let mut stores = DataStores::default();
    let outcomes = apply_all(
        &mut stores,
        ClientboundDataStore {
            updates: vec![
                server_update(
                    "layout[0].text",
                    BedrockDDUIDataStoreUpdateData::String("early".into()),
                    [0, 1],
                ),
                BedrockDDUI::DataStoreChange {
                    data_store_name: "minecraft".into(),
                    property: "custom_form_data_7".into(),
                    update_count: wire::U32LE(1),
                    the_new_property_value: form_property(),
                },
                server_update(
                    "layout[0].text",
                    BedrockDDUIDataStoreUpdateData::String("Clicks: 1".into()),
                    [1, 1],
                ),
                server_update(
                    "layout[1].value",
                    BedrockDDUIDataStoreUpdateData::Double(wire::F64LE(9.0)),
                    [0, 1],
                ),
                server_update(
                    "layout[5].value",
                    BedrockDDUIDataStoreUpdateData::Double(wire::F64LE(9.0)),
                    [1, 1],
                ),
            ],
        },
    );
    assert_eq!(outcomes[0], StoreApply::UnknownProperty);
    assert_eq!(outcomes[1], StoreApply::Applied);
    assert_eq!(outcomes[2], StoreApply::Applied);
    assert_eq!(outcomes[3], StoreApply::StaleProperty);
    assert!(matches!(outcomes[4], StoreApply::InvalidPath(_)));

    let property = stores.property("minecraft", "custom_form_data_7").unwrap();
    assert_eq!(
        property
            .at_path("layout[0].text")
            .and_then(DataValue::as_str),
        Some("Clicks: 1")
    );
    assert_eq!(
        property
            .at_path("layout[1].value")
            .and_then(DataValue::as_number),
        Some(4.0)
    );
    assert_eq!(property.get("layout").unwrap().array_len(), 2);

    apply_all(
        &mut stores,
        ClientboundDataStore {
            updates: vec![BedrockDDUI::DataStoreRemoval {
                data_store_name: "minecraft".into(),
            }],
        },
    );
    assert!(stores.property("minecraft", "custom_form_data_7").is_none());
}

#[test]
fn client_writes_send_the_property_counter_and_the_next_path_counter() {
    let mut stores = DataStores::default();
    apply_all(
        &mut stores,
        ClientboundDataStore {
            updates: vec![
                BedrockDDUI::DataStoreChange {
                    data_store_name: "minecraft".into(),
                    property: "custom_form_data_7".into(),
                    update_count: wire::U32LE(1),
                    the_new_property_value: form_property(),
                },
                server_update(
                    "layout[1].value",
                    BedrockDDUIDataStoreUpdateData::Double(wire::F64LE(8.0)),
                    [1, 2],
                ),
            ],
        },
    );
    let update = stores
        .write(
            "minecraft",
            "custom_form_data_7",
            "layout[1].value",
            DataPrimitive::Number(6.0),
        )
        .unwrap();
    assert_eq!(
        (update.property_update_count, update.path_update_count),
        (1, 3)
    );
    assert_eq!(
        stores
            .property("minecraft", "custom_form_data_7")
            .unwrap()
            .at_path("layout[1].value")
            .and_then(DataValue::as_number),
        Some(6.0)
    );

    let packets::Packet::ServerboundDataStore(ServerboundDataStore {
        update: wire_update,
    }) = sent(&data_store_update_packet(&update))
    else {
        panic!("expected ServerboundDataStore");
    };
    assert_eq!(wire_update.data_store_name, "minecraft");
    assert_eq!(wire_update.property, "custom_form_data_7");
    assert_eq!(wire_update.path, "layout[1].value");
    assert_eq!(
        wire_update.data,
        BedrockDDUIDataStoreUpdateData::Double(wire::F64LE(6.0))
    );
    assert_eq!(wire_update.property_update_count, wire::U32LE(1));
    assert_eq!(wire_update.path_update_count, wire::U32LE(3));

    assert_eq!(
        stores.write("minecraft", "missing", "x", DataPrimitive::Bool(true)),
        Err(StoreApply::UnknownProperty)
    );
}

#[test]
fn closed_packets_carry_the_form_and_its_reason_name() {
    for (reason, name) in [
        (
            DataDrivenScreenCloseReason::ProgrammaticClose,
            "programmaticclose",
        ),
        (
            DataDrivenScreenCloseReason::ProgrammaticCloseAll,
            "programmaticcloseall",
        ),
        (
            DataDrivenScreenCloseReason::ClientCanceled,
            "clientcanceled",
        ),
        (DataDrivenScreenCloseReason::UserBusy, "userbusy"),
        (DataDrivenScreenCloseReason::InvalidForm, "invalidform"),
    ] {
        let packets::Packet::ServerboundDataDrivenScreenClosed(ServerboundDataDrivenScreenClosed {
            form_id,
            close_reason,
        }) = sent(&data_driven_screen_closed_packet(9, reason))
        else {
            panic!("expected ServerboundDataDrivenScreenClosed");
        };
        assert_eq!(form_id, wire::U32LE(9));
        assert_eq!(close_reason, name);
    }
}

#[test]
fn store_changes_preserve_nested_values_and_null_replacements() {
    let mut stores = DataStores::default();
    apply_all(
        &mut stores,
        ClientboundDataStore {
            updates: vec![
                BedrockDDUI::DataStoreChange {
                    data_store_name: "minecraft".into(),
                    property: "custom_form_data_7".into(),
                    update_count: wire::U32LE(1),
                    the_new_property_value: form_property(),
                },
                BedrockDDUI::DataStoreChange {
                    data_store_name: "minecraft".into(),
                    property: "custom_form_data_7".into(),
                    update_count: wire::U32LE(2),
                    the_new_property_value: DynamicValue::None,
                },
            ],
        },
    );
    assert_eq!(
        stores.property("minecraft", "custom_form_data_7"),
        Some(&DataValue::None)
    );
}
