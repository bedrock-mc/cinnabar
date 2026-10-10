//! Leaving for the menu releases the server packs recent joins keep for a following join.

use std::path::Path;

use bevy::{
    app::{App, AppExit, Update},
    ecs::system::ScheduleSystem,
    prelude::IntoScheduleConfigs,
};

use client_ui::ui_runtime::UiRuntime;
use {super::MenuRuntime, launcher::menu::MenuAction};
use {
    crate::{
        app::ClientBlobCacheOwner,
        runtime::{
            network::{CompiledStacks, NetworkHandle, ResourcePackAdmissionState},
            world::{ClientWorld, TransferNotice},
        },
        session::{
            SessionController, drive_session, follow_server_transfer, recover_session_failure,
        },
    },
    launcher::install_layout::{InstallEnvironment, InstallLayout, Platform},
};

/// A development layout under `root` with no core executable, so every join fails to start.
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

/// Stacks of their own, keeping one admitted stack as a finished join would.
fn kept_stacks() -> &'static CompiledStacks {
    let stack =
        resource_pack::validate_handoff(protocol::ResourcePackHandoff::from_archives(vec![
            protocol::ResourcePackArchive::unencrypted(
                "00000000-0000-0000-0000-00000000ca7e".parse().unwrap(),
                "1.0.0".into(),
                String::new(),
                vec![0; 32],
            ),
        ]));
    let kept = Box::leak(Box::new(CompiledStacks::from(stack)));
    assert_eq!(kept.len(), 1);
    kept
}

/// Runs `system` once over a launcher menu in a world, whose session releases `kept`.
fn run_session<M>(
    root: &Path,
    kept: &'static CompiledStacks,
    client_world: ClientWorld,
    prepare: impl FnOnce(&mut MenuRuntime),
    system: impl IntoScheduleConfigs<ScheduleSystem, M>,
) {
    let mut menu = MenuRuntime::new_with_layout(
        true,
        Some(2),
        "Player".to_owned(),
        missing_core_layout(root),
        crate::player_skin::LocalPlayerSkin::generated_default("Player"),
    );
    menu.catalog_started = true;
    menu.show_world();
    prepare(&mut menu);
    let mut app = App::new();
    app.add_message::<AppExit>()
        .insert_resource(menu)
        .insert_resource(SessionController::default().with_kept_packs(kept))
        .insert_resource(NetworkHandle::disconnected())
        .insert_resource(ClientBlobCacheOwner::default())
        .insert_resource(ResourcePackAdmissionState::default())
        .insert_resource(UiRuntime::new(1))
        .insert_resource(crate::player_runtime::PlayerRuntime::new(1))
        .insert_resource(client_world)
        .insert_resource(crate::movement::MovementTicker::default())
        .insert_resource(crate::movement::LocalPhysicsController::default())
        .insert_resource(client_presentation::local_player::LocalPlayerFrameCarrier::default())
        .insert_resource(client_presentation::local_player::InteractionOriginSnapshot::default())
        .add_systems(Update, system);
    app.update();
}

// A transfer whose replacement join cannot start lands in the menu with nothing to reuse them.
#[test]
fn a_failed_transfer_releases_the_kept_server_packs() {
    let root = tempfile::tempdir().unwrap();
    let kept = kept_stacks();
    let client_world = ClientWorld {
        transfer_notice: Some(TransferNotice {
            host: "transfer.example.net".to_owned(),
            port: 19132,
        }),
        ..ClientWorld::default()
    };
    run_session(
        root.path(),
        kept,
        client_world,
        |_| {},
        follow_server_transfer,
    );
    assert_eq!(kept.len(), 0);
}

#[test]
fn an_explicit_disconnect_releases_the_kept_server_packs() {
    let root = tempfile::tempdir().unwrap();
    let kept = kept_stacks();
    run_session(
        root.path(),
        kept,
        ClientWorld::default(),
        |menu| {
            menu.open_pause();
            menu.activate(MenuAction::PauseDisconnect);
        },
        drive_session,
    );
    assert_eq!(kept.len(), 0);
}

#[test]
fn a_session_failure_releases_the_kept_server_packs() {
    let root = tempfile::tempdir().unwrap();
    let kept = kept_stacks();
    let client_world = ClientWorld {
        fatal_error: Some("network read failed: closed".to_owned()),
        ..ClientWorld::default()
    };
    run_session(
        root.path(),
        kept,
        client_world,
        |_| {},
        recover_session_failure,
    );
    assert_eq!(kept.len(), 0);
}
