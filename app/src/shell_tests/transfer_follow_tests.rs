use crate::runtime::network::resource_packs::PackApplication;
use client_runtime::network::NetworkHandle;
use client_shell::menu::test_support as menu_fixture;
use client_shell::ui_resources::UiRuntimeResource;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use bevy::prelude::{App, Update};

use client_ui::ui_runtime::UiRuntime;
use {
    crate::{
        runtime::world::{ClientWorld, TransferNotice},
        session::{SessionController, drive_session, follow_server_transfer},
    },
    client_runtime::{
        blob_cache::ClientBlobCacheOwner, pack_admission::ResourcePackAdmissionState,
    },
    launcher::install_layout::{InstallEnvironment, InstallLayout, Platform},
};
use {
    client_shell::menu::MenuRuntime,
    launcher::menu::{MenuAction, MenuScreen},
};

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "cinnabar-menu-transfer-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create isolated transfer root");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fixture_layout(root: &Path) -> InstallLayout {
    InstallLayout::resolve(
        Platform::Linux,
        &InstallEnvironment {
            executable: root.join("target/debug/cinnabar"),
            user_root: None,
            home: Some(root.join("home")),
            local_app_data: None,
            xdg_config_home: None,
            xdg_data_home: None,
            xdg_runtime_dir: None,
        },
    )
    .expect("isolated development layout")
}

#[test]
fn failed_automatic_replacement_cannot_return_to_the_old_pause_menu() {
    let mut player_runtime = client_state::PlayerRuntime(player_state::PlayerState::new(1));

    failed_automatic_replacement(&mut player_runtime, true, false);
}

#[test]
fn failed_automatic_replacement_from_gameplay_reopens_the_launcher() {
    let mut player_runtime = client_state::PlayerRuntime(player_state::PlayerState::new(1));

    failed_automatic_replacement(&mut player_runtime, false, false);
}

#[test]
fn failed_remote_transfer_from_a_local_world_offers_retry_after_teardown() {
    let mut player_runtime = client_state::PlayerRuntime(player_state::PlayerState::new(1));
    failed_automatic_replacement(&mut player_runtime, false, true);
}

#[test]
fn explicit_disconnect_discards_queued_terminal_events_and_closes_the_old_receiver() {
    use crate::runtime::network::session::NetworkControlEvent;
    use bevy::app::AppExit;
    use tokio::sync::mpsc;

    let root = TempRoot::new();
    let mut menu = MenuRuntime::new_with_layout(
        true,
        Some(2),
        "Player".to_owned(),
        fixture_layout(root.path()),
        client_shell::player_skin::LocalPlayerSkin::generated_default("Player"),
    );
    menu.show_world();
    menu.open_pause();
    menu.activate(MenuAction::PauseDisconnect);
    let mut network = NetworkHandle::<PackApplication>::disconnected();
    let (old_controls, receiver) = mpsc::channel(2);
    *network.control_events_mut() = receiver;
    old_controls
        .try_send(NetworkControlEvent::Transferred {
            target: client_session::SessionTransferTarget {
                host: "old.example.net".to_owned(),
                port: 19132,
            },
            decode_error_count: 0,
        })
        .unwrap();
    old_controls
        .try_send(NetworkControlEvent::Stopped {
            decode_error_count: 0,
        })
        .unwrap();

    let mut app = App::new();
    app.add_message::<AppExit>()
        .insert_resource(menu)
        .insert_resource(SessionController::default())
        .insert_resource(network)
        .insert_resource(ClientBlobCacheOwner::default())
        .insert_resource(ResourcePackAdmissionState::default())
        .insert_resource(crate::runtime::network::session_registries::tests::collision_resource())
        .insert_resource(UiRuntimeResource(UiRuntime::new(1)))
        .insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
            1,
        )))
        .insert_resource(ClientWorld::default())
        .init_resource::<crate::runtime::shutdown::FatalError>()
        .insert_resource(client_interaction::movement::MovementTicker::default())
        .insert_resource(client_interaction::movement::LocalPhysicsController::default())
        .insert_resource(client_presentation::local_player::LocalPlayerFrameCarrier::default())
        .insert_resource(client_presentation::local_player::InteractionOriginSnapshot::default())
        .add_systems(Update, drive_session);
    app.update();

    let menu = app.world().resource::<MenuRuntime>();
    assert!(menu.is_visible());
    assert_eq!(menu.view().screen, MenuScreen::Home);
    assert!(!menu.is_connecting());
    assert_eq!(
        app.world()
            .resource::<NetworkHandle<PackApplication>>()
            .pending_event_count(),
        0
    );
    assert!(
        app.world()
            .resource::<ClientWorld>()
            .transfer_notice
            .is_none()
    );
    assert!(old_controls.is_closed());
}

/// Cancellation wins over the same prepared endpoint before network admission.
#[test]
fn cancellation_precedes_a_ready_join_response() {
    use bevy::app::AppExit;
    let root = TempRoot::new();
    let mut menu = MenuRuntime::new_with_layout(
        true,
        Some(2),
        "Player".into(),
        fixture_layout(root.path()),
        client_shell::player_skin::LocalPlayerSkin::generated_default("Player"),
    );
    menu.show_connecting();
    menu_fixture::request_disconnect(&mut menu);
    let mut controller = SessionController::default();
    controller.adopt_join(
        "fixture.invalid:19132",
        false,
        protocol::SessionEndpoint::Native(protocol::NativeConnectionTarget::new(
            "fixture.invalid:19132".into(),
            protocol::NativeTransport::Addressed,
        )),
    );
    let mut app = App::new();
    app.add_message::<AppExit>()
        .insert_resource(menu)
        .insert_resource(controller)
        .insert_resource(NetworkHandle::<PackApplication>::disconnected())
        .insert_resource(ClientBlobCacheOwner::default())
        .insert_resource(ResourcePackAdmissionState::default())
        .insert_resource(crate::runtime::network::session_registries::tests::collision_resource())
        .insert_resource(UiRuntimeResource(UiRuntime::new(1)))
        .insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
            1,
        )))
        .insert_resource(ClientWorld::default())
        .init_resource::<crate::runtime::shutdown::FatalError>()
        .insert_resource(client_interaction::movement::MovementTicker::default())
        .insert_resource(client_interaction::movement::LocalPhysicsController::default())
        .insert_resource(client_presentation::local_player::LocalPlayerFrameCarrier::default())
        .insert_resource(client_presentation::local_player::InteractionOriginSnapshot::default())
        .add_systems(Update, drive_session);
    app.update();
    assert!(!app.world().resource::<MenuRuntime>().is_connecting());
    assert!(!app.world().resource::<SessionController>().join_pending());
    assert_eq!(
        app.world()
            .resource::<NetworkHandle<PackApplication>>()
            .pending_event_count(),
        0
    );
}

/// A startup exit retires the original owner before its prepared endpoint is admitted.
#[test]
fn exit_before_first_session_frame_retires_a_pending_startup_join() {
    use bevy::prelude::{AppExit, First, MessageWriter};

    for (status, cause) in [
        (AppExit::Success, None),
        (
            AppExit::error(),
            Some("startup failed before session admission"),
        ),
    ] {
        let root = TempRoot::new();
        let mut menu = MenuRuntime::new_with_layout(
            false,
            Some(2),
            "Player".into(),
            fixture_layout(root.path()),
            client_shell::player_skin::LocalPlayerSkin::generated_default("Player"),
        );
        menu.show_connecting();
        let controller = SessionController::default()
            .with_client_random_id(Some(protocol::ClientRandomId::new(-7).unwrap()))
            .with_startup_endpoint(
                Some(protocol::SessionEndpoint::Native(
                    protocol::NativeConnectionTarget::new(
                        "fixture.invalid:19132".into(),
                        protocol::NativeTransport::Addressed,
                    ),
                )),
                Some("fixture.invalid:19132"),
                #[cfg(feature = "native-networking")]
                None,
            );
        let original_generation = controller.generation();
        assert!(controller.join_pending());
        let mut network = NetworkHandle::<PackApplication>::disconnected();
        let (old_controls, receiver) = tokio::sync::mpsc::channel(1);
        *network.control_events_mut() = receiver;
        let startup_exit = status.clone();
        let mut app = App::new();
        app.add_message::<AppExit>()
            .insert_resource(menu)
            .insert_resource(controller)
            .insert_resource(network)
            .insert_resource(ClientBlobCacheOwner::default())
            .insert_resource(ResourcePackAdmissionState::default())
            .insert_resource(
                crate::runtime::network::session_registries::tests::collision_resource(),
            )
            .insert_resource(UiRuntimeResource(UiRuntime::new(1)))
            .insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
                1,
            )))
            .insert_resource(ClientWorld::default())
            .insert_resource(crate::runtime::shutdown::FatalError(
                cause.map(str::to_owned),
            ))
            .insert_resource(client_interaction::movement::MovementTicker::default())
            .insert_resource(client_interaction::movement::LocalPhysicsController::default())
            .insert_resource(client_presentation::local_player::LocalPlayerFrameCarrier::default())
            .insert_resource(
                client_presentation::local_player::InteractionOriginSnapshot::default(),
            )
            .add_systems(First, move |mut exits: MessageWriter<AppExit>| {
                exits.write(startup_exit.clone());
            })
            .add_systems(Update, drive_session);

        let terminal = crate::runtime::shutdown::TerminalCause::install(&mut app);
        app.update();

        assert!(old_controls.is_closed());
        let controller = app.world().resource::<SessionController>();
        assert!(!controller.join_pending());
        assert!(!controller.status().connecting);
        assert_ne!(controller.generation(), original_generation);
        assert!(!app.world().resource::<MenuRuntime>().is_connecting());
        assert_eq!(app.should_exit(), Some(status));
        if let Some(cause) = cause {
            assert_eq!(
                app.world()
                    .resource::<crate::runtime::shutdown::FatalError>()
                    .0
                    .as_deref(),
                Some(cause),
            );
            assert!(terminal.exit_message().ends_with(cause));
        }
    }
}

fn failed_automatic_replacement(
    player_runtime: &mut client_state::PlayerRuntime,
    from_settings: bool,
    from_local_world: bool,
) {
    let root = TempRoot::new();
    let mut menu = MenuRuntime::new_with_layout(
        true,
        Some(2),
        "Player".to_owned(),
        fixture_layout(root.path()),
        client_shell::player_skin::LocalPlayerSkin::generated_default("Player"),
    );
    let controller = SessionController::default();
    let old_generation = controller.generation();
    let mut local_worlds = client_shell::local_worlds::LocalWorlds::default();
    if from_local_world {
        menu.request_local_world_join("Local world".into(), true);
        menu.sync_local_worlds(&mut local_worlds, false);
        menu.take_join_intent().unwrap();
        assert!(menu_fixture::local_world_state(&menu) == (true, true));
    }
    menu.show_world();
    if from_settings {
        menu.open_pause();
        menu.activate(MenuAction::PauseSettings);
    }

    let client_world = ClientWorld {
        stream: Some(chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: [0.0, 64.0, 0.0],
            world_spawn_position: [0, 64, 0],
            air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        })),
        transfer_notice: Some(TransferNotice {
            host: "transfer.example.net".to_owned(),
            port: 19132,
        }),
        ..ClientWorld::default()
    };
    let mut runtime = UiRuntime::new(old_generation);
    let _ = runtime.open_chat(player_runtime);
    runtime.insert_chat_text("old session draft").unwrap();

    let mut app = App::new();
    app.insert_resource(menu)
        .insert_resource(controller)
        .insert_resource(NetworkHandle::<PackApplication>::disconnected())
        .insert_resource(ClientBlobCacheOwner::default())
        .insert_resource(ResourcePackAdmissionState::default())
        .insert_resource(crate::runtime::network::session_registries::tests::collision_resource())
        .insert_resource(UiRuntimeResource(runtime))
        .insert_resource(player_runtime.clone())
        .insert_resource(client_world)
        .init_resource::<crate::runtime::shutdown::FatalError>()
        .insert_resource(client_interaction::movement::MovementTicker::default())
        .insert_resource(client_interaction::movement::LocalPhysicsController::default())
        .insert_resource(client_presentation::local_player::LocalPlayerFrameCarrier::default())
        .insert_resource(client_presentation::local_player::InteractionOriginSnapshot::default())
        .add_systems(Update, follow_server_transfer);
    app.update();

    assert!(app.world().resource::<ClientWorld>().stream.is_none());
    let runtime = app.world().resource::<UiRuntimeResource>();
    assert_ne!(runtime.session_id(), old_generation);
    assert!(!runtime.chat_focused());
    assert!(runtime.chat_editor().as_str().is_empty());

    let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
    assert!(menu.is_visible());
    assert_eq!(menu.view().screen, MenuScreen::Home);
    assert_eq!(
        menu.view().disconnect_message.as_ref().unwrap(),
        &crate::args::ArgsError::NativeNetworkingUnavailable.to_string()
    );
    if from_local_world {
        assert!(
            !menu.view().can_reconnect,
            "local teardown must finish first"
        );
        menu.sync_local_worlds(&mut local_worlds, false);
        assert!(menu_fixture::local_world_state(&menu) == (false, false));
    }
    assert!(menu.view().can_reconnect);
    menu.activate(MenuAction::Reconnect);
    assert_eq!(
        menu.take_join_intent().unwrap().address,
        "transfer.example.net:19132"
    );
    menu.show_home();
    menu.show_join_failure("Could not start transfer.example.net".into());
    menu.activate(MenuAction::AddBack);
    assert_eq!(menu.view().screen, MenuScreen::Home);
}
