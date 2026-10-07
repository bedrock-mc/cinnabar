use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use bevy::prelude::{App, Update};

use super::{MenuAction, MenuRuntime, MenuScreen};
use crate::{
    app::ClientBlobCacheOwner,
    install_layout::{InstallEnvironment, InstallLayout, Platform},
    runtime::{
        network::{NetworkHandle, ResourcePackAdmissionState},
        world::{ClientWorld, TransferNotice},
    },
    session::{SessionController, drive_session, follow_server_transfer},
};
use client_ui::ui_runtime::UiRuntime;

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

fn missing_core_layout(root: &Path) -> InstallLayout {
    InstallLayout::resolve(
        Platform::Linux,
        &InstallEnvironment {
            executable: root.join("target/debug/bedrock-client"),
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
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    failed_automatic_replacement(&mut player_runtime, true);
}

#[test]
fn pointer_opened_dialogs_accept_keyboard_confirmation_and_navigation() {
    for remove_saved in [false, true] {
        let root = TempRoot::new();
        let mut menu = MenuRuntime::new_with_layout(
            true,
            Some(2),
            "Player".to_owned(),
            missing_core_layout(root.path()),
            crate::player_skin::LocalPlayerSkin::generated_default("Player"),
        );
        menu.servers.push(super::SavedServer {
            name: "Local".to_owned(),
            address: "127.0.0.1:19132".to_owned(),
            favorite: false,
            last_joined_unix: 0,
        });
        menu.focused = 6;
        let (open, confirm) = if remove_saved {
            (
                MenuAction::RemoveSavedDialog(0),
                MenuAction::ConfirmRemoveSaved(0),
            )
        } else {
            (MenuAction::OpenExitDialog, MenuAction::ConfirmExit)
        };
        menu.activate(open);
        assert_eq!(menu.view().focused_action, Some(confirm));
        menu.move_focus(1);
        assert_eq!(menu.view().focused_action, Some(MenuAction::DismissDialog));
        menu.move_focus(-1);
        menu.activate_focused();
        assert!(menu.dialog.is_none());
        if remove_saved {
            assert!(menu.servers.is_empty());
        } else {
            assert!(menu.intents.exit);
        }
    }
}

#[test]
fn failed_automatic_replacement_from_gameplay_reopens_the_launcher() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    failed_automatic_replacement(&mut player_runtime, false);
}

#[test]
fn explicit_disconnect_discards_queued_terminal_events_and_closes_the_old_receiver() {
    use crate::runtime::network::NetworkControlEvent;
    use bevy::app::AppExit;
    use tokio::sync::mpsc;

    let root = TempRoot::new();
    let mut menu = MenuRuntime::new_with_layout(
        true,
        Some(2),
        "Player".to_owned(),
        missing_core_layout(root.path()),
        crate::player_skin::LocalPlayerSkin::generated_default("Player"),
    );
    menu.catalog_started = true;
    menu.show_world();
    menu.open_pause();
    menu.activate(MenuAction::PauseDisconnect);
    let mut network = NetworkHandle::disconnected();
    let (old_controls, receiver) = mpsc::channel(2);
    *network.control_events_mut() = receiver;
    old_controls
        .try_send(NetworkControlEvent::Transferred {
            target: crate::runtime::network::SessionTransferTarget {
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
        .insert_resource(UiRuntime::new(1))
        .insert_resource(crate::player_runtime::PlayerRuntime::new(1))
        .insert_resource(ClientWorld::default())
        .insert_resource(crate::movement::MovementTicker::default())
        .insert_resource(crate::movement::LocalPhysicsController::default())
        .insert_resource(crate::local_player::LocalPlayerFrameCarrier::default())
        .insert_resource(crate::local_player::InteractionOriginSnapshot::default())
        .add_systems(Update, drive_session);
    app.update();

    let menu = app.world().resource::<MenuRuntime>();
    assert!(menu.is_visible());
    assert_eq!(menu.view().screen, MenuScreen::Home);
    assert!(!menu.is_connecting());
    assert_eq!(
        app.world()
            .resource::<NetworkHandle>()
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

/// A menu whose per-session core is `script`, in an app driving joins.
#[cfg(unix)]
fn app_with_core(root: &Path, script: &str) -> App {
    use std::os::unix::fs::PermissionsExt;

    let layout = missing_core_layout(root);
    fs::create_dir_all(layout.core_executable.parent().unwrap()).unwrap();
    fs::write(&layout.core_executable, script).unwrap();
    fs::set_permissions(&layout.core_executable, fs::Permissions::from_mode(0o755)).unwrap();
    let menu = MenuRuntime::new_with_layout(
        true,
        Some(2),
        "Player".to_owned(),
        layout,
        crate::player_skin::LocalPlayerSkin::generated_default("Player"),
    );
    let mut app = App::new();
    app.add_message::<bevy::app::AppExit>()
        .insert_resource(menu)
        .insert_resource(SessionController::default())
        .insert_resource(NetworkHandle::disconnected())
        .insert_resource(ClientBlobCacheOwner::default())
        .insert_resource(ResourcePackAdmissionState::default())
        .insert_resource(UiRuntime::new(1))
        .insert_resource(crate::player_runtime::PlayerRuntime::new(1))
        .insert_resource(ClientWorld::default())
        .insert_resource(crate::movement::MovementTicker::default())
        .insert_resource(crate::movement::LocalPhysicsController::default())
        .insert_resource(crate::local_player::LocalPlayerFrameCarrier::default())
        .insert_resource(crate::local_player::InteractionOriginSnapshot::default())
        .add_systems(Update, drive_session);
    app
}

#[cfg(unix)]
#[test]
fn cancellation_precedes_a_ready_join_response() {
    use crate::session::JoinStage;

    let root = TempRoot::new();
    let mut app = app_with_core(root.path(), "#!/bin/sh\nexit 0\n");
    let (reply, ready) = crossbeam_channel::bounded(1);
    reply.send(Err("retired join failed".to_owned())).unwrap();
    app.world_mut()
        .resource_mut::<SessionController>()
        .adopt_join("local world", true, JoinStage::Launcher(ready));
    {
        let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
        menu.enter(MenuScreen::Play);
        menu.show_connecting();
        menu.intents.disconnect = true;
    }
    app.update();
    let menu = app.world().resource::<MenuRuntime>();
    assert_eq!(menu.screen(), MenuScreen::Play);
    assert!(
        menu.message
            .as_deref()
            .is_none_or(|message| !message.contains("retired join"))
    );
    assert!(!app.world().resource::<SessionController>().join_pending());
}

// Join frames stay short while the core starts; cancelling reaps it.
#[cfg(unix)]
#[test]
fn a_join_frame_does_not_wait_for_the_core() {
    let root = TempRoot::new();
    // Holds stdin like bedrock-core and never publishes an endpoint.
    let mut app = app_with_core(root.path(), "#!/bin/sh\nIFS= read -r hold\n");
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .request_connect("127.0.0.1:19132".to_owned());
    let started = std::time::Instant::now();
    app.update();
    app.update();
    let frames = started.elapsed();
    if std::env::var_os("CINNABAR_MENU_LATENCY").is_some() {
        eprintln!("menu-latency join (two frames)          {frames:?}");
    }
    let menu = app.world().resource::<MenuRuntime>();
    let joining = app.world().resource::<SessionController>().join_pending();
    assert!(frames < std::time::Duration::from_secs(1), "{frames:?}");
    assert!(menu.is_connecting() && joining);
    assert!(menu.view().connecting);

    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .intents
        .disconnect = true;
    app.update();
    let menu = app.world().resource::<MenuRuntime>();
    assert!(!menu.is_connecting());
    assert!(!app.world().resource::<SessionController>().join_pending());
}

// A core that dies before publishing fails the join on a later frame, not
// after the full start timeout.
#[cfg(unix)]
#[test]
fn a_core_that_exits_early_fails_the_join_promptly() {
    let root = TempRoot::new();
    let mut app = app_with_core(root.path(), "#!/bin/sh\nexit 3\n");
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .request_connect("127.0.0.1:19132".to_owned());
    while app
        .world_mut()
        .resource_mut::<SessionController>()
        .core_mut()
        .id()
        .is_none()
        && app.world().resource::<MenuRuntime>().is_connecting()
    {
        app.update();
    }
    // Frames wait for the exit, so a launch slowed by load cannot let the start deadline win.
    let spawned = std::time::Instant::now();
    while !app
        .world_mut()
        .resource_mut::<SessionController>()
        .core_mut()
        .exited()
    {
        assert!(
            spawned.elapsed() < std::time::Duration::from_secs(60),
            "stub core never exited"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    while app.world().resource::<MenuRuntime>().is_connecting() {
        app.update();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(!app.world().resource::<SessionController>().join_pending());
    let menu = app.world().resource::<MenuRuntime>();
    // The exit is seen, not the start timeout (a new executable's first
    // launch can itself take seconds on macOS).
    assert!(
        menu.message.as_deref().is_some_and(|message| {
            message.starts_with("Could not start 127.0.0.1")
                && message.contains("exited before publishing")
        }),
        "{:?}",
        menu.message
    );
}

fn failed_automatic_replacement(
    player_runtime: &mut crate::player_runtime::PlayerRuntime,
    from_settings: bool,
) {
    let root = TempRoot::new();
    let mut menu = MenuRuntime::new_with_layout(
        true,
        Some(2),
        "Player".to_owned(),
        missing_core_layout(root.path()),
        crate::player_skin::LocalPlayerSkin::generated_default("Player"),
    );
    let controller = SessionController::default();
    let old_generation = controller.generation();
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
            air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
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
        .insert_resource(NetworkHandle::disconnected())
        .insert_resource(ClientBlobCacheOwner::default())
        .insert_resource(ResourcePackAdmissionState::default())
        .insert_resource(runtime)
        .insert_resource(player_runtime.clone())
        .insert_resource(client_world)
        .insert_resource(crate::movement::MovementTicker::default())
        .insert_resource(crate::movement::LocalPhysicsController::default())
        .insert_resource(crate::local_player::LocalPlayerFrameCarrier::default())
        .insert_resource(crate::local_player::InteractionOriginSnapshot::default())
        .add_systems(Update, follow_server_transfer);
    app.update();

    assert!(app.world().resource::<ClientWorld>().stream.is_none());
    let runtime = app.world().resource::<UiRuntime>();
    assert_ne!(runtime.session_id(), old_generation);
    assert!(!runtime.chat_focused());
    assert!(runtime.chat_editor().as_str().is_empty());

    let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
    assert!(menu.is_visible());
    assert_eq!(menu.view().screen, MenuScreen::Home);
    assert!(
        menu.view()
            .message
            .as_deref()
            .is_some_and(|message| message.starts_with("Could not start transfer.example.net"))
    );
    menu.go_back();
    assert_eq!(menu.view().screen, MenuScreen::Home);
}
