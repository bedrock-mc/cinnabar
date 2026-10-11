use {
    super::MenuRuntime,
    launcher::menu::{MenuAction, MenuScreen},
};

#[test]
fn remote_join_failure_logs_its_complete_provisioning_diagnostic() {
    let log = tempfile::NamedTempFile::new().unwrap();
    let subscriber = bevy::log::tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_writer(log.as_file().try_clone().unwrap())
        .finish();
    let mut menu = MenuRuntime::new(true, 2, "Player".into());
    menu.request_connect("127.0.0.1:19132".into());
    menu.take_join_intent().unwrap();
    let diagnostic = "Could not start 127.0.0.1:19132: missing core at /test/missing-core";
    bevy::log::tracing::subscriber::with_default(subscriber, || {
        bevy::log::tracing::callsite::rebuild_interest_cache();
        menu.show_join_failure(diagnostic.into());
    });
    let logged = std::fs::read_to_string(log.path()).unwrap();
    assert!(
        logged.contains(diagnostic),
        "complete cause must remain in the log"
    );
    assert!(menu.view().can_reconnect);
}

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
fn local_join_failure_keeps_its_world_error_without_retry() {
    let mut menu = MenuRuntime::new(true, 2, "Player".into());
    menu.request_local_world_join("Local world".into(), false);
    menu.take_join_intent().unwrap();
    menu.begin_join_progress("Local world", true);
    menu.show_join_failure("Could not open local world".into());
    assert_eq!(
        menu.view().message.as_deref(),
        Some("Could not open local world")
    );
    assert!(menu.view().disconnect_message.is_none());
    assert!(!menu.view().can_reconnect);
}

#[test]
fn join_request_popup_keeps_input_priority_over_a_failure() {
    for answer in [Some(true), Some(false), None] {
        let mut menu = MenuRuntime::new(true, 2, "Player".into());
        menu.request_connect("127.0.0.1:19132".into());
        menu.take_join_intent().unwrap();
        menu.show_world();
        menu.push_join_request(42, "Alex".into(), std::time::Duration::ZERO);
        menu.absorb_session_failure("network session failed: closed");
        assert_eq!(menu.view().join_request_prompt(), Some("Alex"));
        assert_eq!(
            menu.focus_actions(),
            [
                MenuAction::JoinRequest(true),
                MenuAction::JoinRequest(false)
            ]
        );
        for hidden in [MenuAction::Reconnect, MenuAction::DismissDialog] {
            menu.activate(hidden);
            assert!(menu.take_join_intent().is_none());
            assert!(menu.view().disconnect_message.is_some());
            assert_eq!(menu.view().join_request_prompt(), Some("Alex"));
        }
        match answer {
            Some(true) => menu.activate_focused(),
            Some(false) => menu.activate(MenuAction::JoinRequest(false)),
            None => menu.go_back(),
        }
        assert_eq!(menu.take_join_reply(), Some((42, answer.unwrap_or(false))));
        assert!(menu.view().join_request_prompt().is_none());
        assert!(menu.view().disconnect_message.is_some());
        assert!(menu.view().can_reconnect);
        assert_eq!(menu.view().focused_action, Some(MenuAction::Reconnect));
        menu.activate_focused();
        assert_eq!(menu.take_join_intent().unwrap().address, "127.0.0.1:19132");
    }
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
        menu.feeds.join =
            launcher::menu::view::JoinProgress::new(launcher::menu::view::JoinKind::External);
        menu.feeds
            .join
            .observe(Some(launcher::menu::view::JoinStage::Packs {
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
        .insert_resource(client_presentation::local_player::LocalPlayerFrameCarrier::default())
        .insert_resource(client_presentation::local_player::InteractionOriginSnapshot::default())
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

#[test]
fn featured_presence_survives_join_adoption_and_clears_on_transfer() {
    let mut menu = MenuRuntime::new(true, 2, "Player".into());
    menu.request_featured_connect("example.invalid".into());
    let intent = menu.take_join_intent().unwrap();
    menu.remember_retry_target(&intent.address, intent.auth_cache.as_deref(), false);
    assert!(menu.presence_is_featured());
    menu.remember_retry_target("other.invalid", intent.auth_cache.as_deref(), false);
    assert!(!menu.presence_is_featured());
    menu.remember_retry_target("Local world", None, true);
    assert!(menu.presence_address().is_none());
}
