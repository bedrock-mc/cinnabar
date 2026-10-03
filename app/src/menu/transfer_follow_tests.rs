use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use bevy::prelude::{App, Update};

use super::{
    super::{
        CoreProcessGuard, MAX_TRANSFER_CHAIN_HOPS, MenuAction, MenuRuntime, MenuScreen,
        format_transfer_address,
    },
    follow_server_transfer,
};
use crate::{
    app::ClientBlobCacheOwner,
    install_layout::{InstallEnvironment, InstallLayout, Platform},
    runtime::{
        network::{NetworkHandle, ResourcePackAdmissionState},
        world::{ClientWorld, TransferNotice},
    },
    ui_runtime::UiRuntime,
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

fn missing_core_layout(root: &Path) -> InstallLayout {
    InstallLayout::resolve(
        Platform::Linux,
        &InstallEnvironment {
            executable: root.join("target/debug/bedrock-client"),
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
fn transfer_addresses_bracket_ipv6_and_leave_ordinary_hosts_untouched() {
    assert_eq!(
        format_transfer_address("game.example.net", 19133),
        "game.example.net:19133"
    );
    assert_eq!(format_transfer_address("::1", 19132), "[::1]:19132");
    assert_eq!(
        format_transfer_address("2001:db8::10", 25565),
        "[2001:db8::10]:25565"
    );
    assert_eq!(
        format_transfer_address("[2001:db8::10]", 25565),
        "[2001:db8::10]:25565",
        "an already-bracketed transfer literal must not be bracketed twice",
    );
}

#[test]
fn handoff_targets_are_well_formed_without_any_host_allowlist() {
    let menu = MenuRuntime::new(true, 2, "Player".to_owned());

    let (address, _) = menu
        .transfer_handoff_target(" game.example.net ", 19133)
        .expect("a trimmed well-formed host is a valid target");
    assert_eq!(address, "game.example.net:19133");

    let (address, _) = menu
        .transfer_handoff_target("minigames.other-host.example", 19321)
        .expect("cross-host transfers are legitimate vanilla behavior");
    assert_eq!(address, "minigames.other-host.example:19321");

    assert!(menu.transfer_handoff_target("", 19132).is_none());
    assert!(menu.transfer_handoff_target("   ", 19132).is_none());
}

#[test]
fn the_automatic_transfer_chain_is_bounded() {
    let mut menu = MenuRuntime::new(true, 2, "Player".to_owned());

    // A user-initiated join always starts a fresh bounded chain.
    menu.begin_fresh_transfer_chain();
    for _ in 0..MAX_TRANSFER_CHAIN_HOPS {
        assert!(menu.consume_transfer_chain_hop());
    }
    assert!(
        !menu.consume_transfer_chain_hop(),
        "an exhausted chain must refuse to follow again"
    );

    // And another user join renews it after exhaustion.
    menu.begin_fresh_transfer_chain();
    assert!(menu.consume_transfer_chain_hop());
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
        menu.servers.push(super::super::SavedServer {
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
            assert!(menu.exit_requested);
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
    menu.mark_connected();
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
        .insert_resource(CoreProcessGuard::default())
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
        .add_systems(Update, super::drive_menu_connection);
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
        .insert_resource(CoreProcessGuard::default())
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
        .add_systems(Update, super::drive_menu_connection);
    app
}

#[cfg(unix)]
#[test]
fn cancellation_precedes_a_ready_join_response() {
    let root = TempRoot::new();
    let mut app = app_with_core(root.path(), "#!/bin/sh\nexit 0\n");
    let (reply, ready) = crossbeam_channel::bounded(1);
    reply.send(Err("retired join failed".to_owned())).unwrap();
    {
        let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
        menu.enter(MenuScreen::Play);
        menu.mark_connecting();
        menu.join = Some(super::JoinAttempt {
            generation: menu.session_generation,
            address: "local world".to_owned(),
            auth_cache: None,
            local_world: true,
            stage: super::JoinStage::Launcher(ready),
        });
        menu.disconnect_requested = true;
    }
    app.update();
    let menu = app.world().resource::<MenuRuntime>();
    assert_eq!(menu.screen(), MenuScreen::Play);
    assert!(
        menu.message
            .as_deref()
            .is_none_or(|message| !message.contains("retired join"))
    );
    assert!(menu.join.is_none());
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
    assert!(frames < std::time::Duration::from_secs(1), "{frames:?}");
    assert!(menu.is_connecting() && menu.join.is_some());
    assert!(menu.view().connecting);

    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .disconnect_requested = true;
    app.update();
    let menu = app.world().resource::<MenuRuntime>();
    assert!(!menu.is_connecting() && menu.join.is_none());
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
    while app.world().resource::<CoreProcessGuard>().id().is_none()
        && app.world().resource::<MenuRuntime>().is_connecting()
    {
        app.update();
    }
    // Frames wait for the exit, so a launch slowed by load cannot let the start deadline win.
    let spawned = std::time::Instant::now();
    while !app.world_mut().resource_mut::<CoreProcessGuard>().exited() {
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
    let menu = app.world().resource::<MenuRuntime>();
    assert!(menu.join.is_none());
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
    let old_generation = menu.next_session_generation();
    menu.mark_connected();
    if from_settings {
        menu.open_pause();
        menu.activate(MenuAction::PauseSettings);
    }

    let client_world = ClientWorld {
        stream: Some(client_world::WorldStream::new(protocol::WorldBootstrap {
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
        .insert_resource(CoreProcessGuard::default())
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
