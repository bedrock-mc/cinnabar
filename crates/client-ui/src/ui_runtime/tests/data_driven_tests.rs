use protocol::ddui::{
    DataDrivenScreenCloseReason, DataDrivenUiEvent, DataStoreChange, DataValue,
    ShowDataDrivenScreen, data_driven_screen_closed_packet,
};

use super::*;

fn show(form_id: u32) -> UiEvent {
    UiEvent::DataDrivenUi(DataDrivenUiEvent::Show(ShowDataDrivenScreen {
        screen_id: "minecraft:custom_form".into(),
        form_id,
        data_instance_id: Some(11),
    }))
}

fn sent(runtime: &mut UiRuntime) -> Vec<Vec<u8>> {
    std::iter::from_fn(|| runtime.take_client_packet())
        .map(|packet| protocol::encode(&packet).unwrap().to_vec())
        .collect()
}

fn closed(form_id: u32, reason: DataDrivenScreenCloseReason) -> Vec<u8> {
    protocol::encode(&data_driven_screen_closed_packet(form_id, reason))
        .unwrap()
        .to_vec()
}

#[test]
fn every_screen_request_is_answered_so_server_scripts_finish() {
    let mut runtime = UiRuntime::new(1);
    let mut player = player_state::PlayerState::new(1);
    runtime.apply(&mut player, envelope(1, 1, show(4))).unwrap();
    assert_eq!(
        sent(&mut runtime),
        [closed(4, DataDrivenScreenCloseReason::ClientCanceled)]
    );

    runtime.open_chat(&mut player);
    runtime.apply(&mut player, envelope(1, 2, show(5))).unwrap();
    assert_eq!(
        sent(&mut runtime),
        [closed(5, DataDrivenScreenCloseReason::UserBusy)]
    );

    for (sequence, event) in [
        (3, DataDrivenUiEvent::Close { form_id: Some(5) }),
        (4, DataDrivenUiEvent::Close { form_id: None }),
        (5, DataDrivenUiEvent::Reload),
    ] {
        runtime
            .apply(
                &mut player,
                envelope(1, sequence, UiEvent::DataDrivenUi(event)),
            )
            .unwrap();
    }
    assert!(sent(&mut runtime).is_empty());
}

#[test]
fn data_stores_follow_the_server_until_the_session_ends() {
    let mut runtime = UiRuntime::new(1);
    let mut player = player_state::PlayerState::new(1);
    let store = |changes| UiEvent::DataDrivenUi(DataDrivenUiEvent::Store(changes));
    runtime
        .apply(
            &mut player,
            envelope(
                1,
                1,
                store(vec![
                    DataStoreChange::Replace {
                        store: "minecraft".into(),
                        property: "message_box_data_11".into(),
                        update_count: 1,
                        value: DataValue::String("body".into()),
                    },
                    DataStoreChange::Remove {
                        store: "other".into(),
                    },
                    DataStoreChange::Update(protocol::ddui::DataStoreUpdate {
                        store: "minecraft".into(),
                        property: "unknown".into(),
                        path: "title".into(),
                        value: protocol::ddui::DataPrimitive::Bool(true),
                        property_update_count: 0,
                        path_update_count: 1,
                    }),
                ]),
            ),
        )
        .unwrap();
    let ddui = runtime.data_driven_ui();
    assert_eq!(
        ddui.stores().property("minecraft", "message_box_data_11"),
        Some(&DataValue::String("body".into()))
    );
    assert_eq!(ddui.ignored_store_changes(), 1);

    player.begin_session(2);
    runtime.begin_session(2);
    assert!(
        runtime
            .data_driven_ui()
            .stores()
            .property("minecraft", "message_box_data_11")
            .is_none()
    );
}
