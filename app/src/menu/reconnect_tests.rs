use super::{MenuAction, MenuRuntime, MenuScreen};

#[test]
fn reconnect_retries_the_consumed_destination_twice_and_preserves_origin() {
    for (origin, address) in [
        (MenuScreen::Servers, "127.0.0.1:19132"),
        (MenuScreen::Social, "realm_id/42"),
        (MenuScreen::Play, "friend_xuid/42"),
        (MenuScreen::Home, "gathering/42"),
    ] {
        let mut menu = MenuRuntime::new(true, 2, "Player".into());
        menu.activate(MenuAction::Navigate(origin));
        menu.request_connect(address.into());
        let first = menu.take_join_intent().unwrap();
        for _ in 0..2 {
            menu.show_world();
            assert!(menu.absorb_session_failure("network session failed: closed"));
            assert!(menu.view().can_reconnect);
            assert_eq!(menu.view().focused_action, Some(MenuAction::Reconnect));
            menu.activate_focused();
            let retry = menu
                .take_join_intent()
                .expect("reconnect queues a fresh join");
            assert_eq!(retry.address, first.address);
            assert_eq!(retry.auth_cache, first.auth_cache);
            assert!(!retry.local_world);
            assert!(menu.view().disconnect_message.is_none());
        }
        menu.show_after_disconnect();
        assert_eq!(menu.screen(), origin);
    }
}

#[test]
fn failed_connection_offers_retry_without_entering_a_world() {
    let mut menu = MenuRuntime::new(true, 2, "Player".into());
    menu.request_connect("127.0.0.1:19132".into());
    menu.take_join_intent().unwrap();
    menu.show_join_failure("Could not connect: refused".into());
    assert!(menu.view().disconnect_message.is_some());
    assert!(menu.view().can_reconnect);
}

#[test]
fn a_new_join_from_a_failure_still_accepts_loading_cancel() {
    for late_failure in [false, true] {
        let mut menu = MenuRuntime::new(true, 2, "Player".into());
        menu.request_connect("127.0.0.1:19132".into());
        menu.take_join_intent().unwrap();
        menu.absorb_session_failure("network session failed: closed");
        menu.request_connect("127.0.0.1:19133".into());
        assert!(menu.view().disconnect_message.is_none());
        if late_failure {
            menu.disconnect_message = Some("network session failed: late account event".into());
        }
        menu.feeds.join = super::JoinProgress::new(super::JoinKind::External);
        menu.feeds.join.observe(Some(super::JoinStage::Packs {
            done: 0,
            total: 1,
            received_bytes: 1,
            total_bytes: 4,
        }));
        menu.activate(MenuAction::AddBack);
        assert!(
            menu.take_disconnect_request(),
            "loading Cancel must retire the new attempt"
        );
        assert!(menu.view().disconnect_message.is_none());
    }
}

#[test]
fn local_worlds_and_changed_accounts_cannot_retry_a_remote_destination() {
    let mut menu = MenuRuntime::new(true, 2, "Player".into());
    menu.request_connect("127.0.0.1:19132".into());
    menu.take_join_intent().unwrap();
    menu.feeds.account_active_id = Some("replacement-account".into());
    menu.absorb_session_failure("network session failed: closed");
    assert!(!menu.view().can_reconnect);
    menu.activate(MenuAction::Reconnect);
    assert!(menu.take_join_intent().is_none());

    menu.request_local_world_join("Local world".into(), false);
    menu.take_join_intent().unwrap();
    menu.absorb_session_failure("network session failed: closed");
    assert!(!menu.view().can_reconnect);
    menu.activate(MenuAction::Reconnect);
    assert!(menu.take_join_intent().is_none());
}

#[test]
fn dismiss_and_escape_return_to_the_origin_without_a_stale_retry() {
    for escape in [false, true] {
        let mut menu = MenuRuntime::new(true, 2, "Player".into());
        menu.activate(MenuAction::Navigate(MenuScreen::Servers));
        menu.request_connect("127.0.0.1:19132".into());
        menu.take_join_intent().unwrap();
        menu.absorb_session_failure("network session failed: closed");
        if escape {
            menu.go_back();
        } else {
            menu.activate(MenuAction::DismissDialog);
        }
        assert!(menu.view().disconnect_message.is_none());
        assert_eq!(menu.screen(), MenuScreen::Servers);
        menu.activate(MenuAction::Reconnect);
        assert!(menu.take_join_intent().is_none());
    }
}

#[test]
fn reconnect_drives_two_fresh_production_attempts_and_retires_old_controls() {
    use crate::runtime::{
        network::{NetworkHandle, ResourcePackAdmissionState},
        world::ClientWorld,
    };
    use crate::session::{SessionController, drive_session};
    use bevy::prelude::{App, AppExit, Update};
    use client_ui::ui_runtime::UiRuntime;

    let mut menu = MenuRuntime::new(true, 2, "Player".into());
    menu.catalog_started = true;
    menu.layout.core_executable = menu.layout.user_data_root.join("missing-test-core");
    menu.activate(MenuAction::Navigate(MenuScreen::Servers));
    menu.request_connect("127.0.0.1:19132".into());
    let mut network = NetworkHandle::disconnected();
    let (old_controls, receiver) = tokio::sync::mpsc::channel(1);
    *network.control_events_mut() = receiver;
    let mut app = App::new();
    app.add_message::<AppExit>()
        .insert_resource(menu)
        .insert_resource(SessionController::default())
        .insert_resource(network)
        .insert_resource(crate::app::ClientBlobCacheOwner::default())
        .insert_resource(ResourcePackAdmissionState::default())
        .insert_resource(UiRuntime::new(1))
        .insert_resource(crate::player_runtime::PlayerRuntime::new(1))
        .insert_resource(ClientWorld::default())
        .insert_resource(crate::movement::MovementTicker::default())
        .insert_resource(crate::movement::LocalPhysicsController::default())
        .insert_resource(crate::local_player::LocalPlayerFrameCarrier::default())
        .insert_resource(crate::local_player::InteractionOriginSnapshot::default())
        .add_systems(Update, drive_session);
    for attempt in 0..2 {
        let previous = app.world().resource::<UiRuntime>().session_id();
        app.update();
        assert!(app.world().resource::<UiRuntime>().session_id() > previous);
        assert!(old_controls.is_closed());
        let menu = app.world().resource::<MenuRuntime>();
        assert!(!menu.is_connecting());
        assert!(menu.view().can_reconnect);
        assert!(
            menu.view()
                .disconnect_message
                .as_ref()
                .unwrap()
                .contains("127.0.0.1:19132")
        );
        assert!(app.world().resource::<ClientWorld>().stream.is_none());
        if attempt == 0 {
            app.world_mut()
                .resource_mut::<MenuRuntime>()
                .activate_focused();
        }
    }
}
