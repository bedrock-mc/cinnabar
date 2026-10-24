//! Leaving for the menu keeps the newest server archives for a rejoin.
use client_shell::menu::test_support as menu_fixture;
use client_shell::ui_resources::UiRuntimeResource;

use crate::runtime::network::resource_packs::PackApplication;
use client_runtime::network::NetworkHandle;
use std::path::Path;

use bevy::{
    app::{App, AppExit, Update},
    ecs::system::ScheduleSystem,
    prelude::IntoScheduleConfigs,
};

use client_ui::ui_runtime::UiRuntime;
use {
    crate::{
        runtime::{
            network::CompiledStacks,
            shutdown::FatalError,
            world::{ClientWorld, TransferNotice},
        },
        session::{
            SessionController, drive_session, follow_server_transfer, recover_session_failure,
        },
    },
    client_runtime::{
        blob_cache::ClientBlobCacheOwner, pack_admission::ResourcePackAdmissionState,
    },
    launcher::install_layout::{InstallEnvironment, InstallLayout, Platform},
};
use {client_shell::menu::MenuRuntime, launcher::menu::MenuAction};

/// A development layout under `root` with no core executable, so every join fails to start.
fn missing_core_layout(root: &Path) -> InstallLayout {
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

/// Runs `system` once over a launcher menu in a world, whose session keeps or releases `kept`.
fn run_session<M>(
    root: &Path,
    kept: &'static CompiledStacks,
    client_world: ClientWorld,
    fatal_error: FatalError,
    prepare: impl FnOnce(&mut MenuRuntime),
    system: impl IntoScheduleConfigs<ScheduleSystem, M>,
) {
    session_app(root, kept, client_world, fatal_error, prepare, system).update();
}

/// The [`run_session`] app before its update, for fixtures that add resources first.
fn session_app<M>(
    root: &Path,
    kept: &'static CompiledStacks,
    client_world: ClientWorld,
    fatal_error: FatalError,
    prepare: impl FnOnce(&mut MenuRuntime),
    system: impl IntoScheduleConfigs<ScheduleSystem, M>,
) -> App {
    let mut menu = MenuRuntime::new_with_layout(
        true,
        Some(2),
        "Player".to_owned(),
        missing_core_layout(root),
        client_shell::player_skin::LocalPlayerSkin::generated_default("Player"),
    );
    menu_fixture::suppress_catalog(&mut menu);
    menu.show_world();
    prepare(&mut menu);
    let mut app = App::new();
    app.add_message::<AppExit>()
        .insert_resource(menu)
        .insert_resource(SessionController::default().with_kept_packs(kept))
        .insert_resource(NetworkHandle::<PackApplication>::disconnected())
        .insert_resource(ClientBlobCacheOwner::default())
        .insert_resource(ResourcePackAdmissionState::default())
        .insert_resource(crate::runtime::network::session_registries::tests::collision_resource())
        .insert_resource(UiRuntimeResource(UiRuntime::new(1)))
        .insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
            1,
        )))
        .insert_resource(client_world)
        .insert_resource(fatal_error)
        .insert_resource(client_interaction::movement::MovementTicker::default())
        .insert_resource(client_interaction::movement::LocalPhysicsController::default())
        .insert_resource(client_presentation::local_player::LocalPlayerFrameCarrier::default())
        .insert_resource(client_presentation::local_player::InteractionOriginSnapshot::default())
        .add_systems(Update, system);
    app
}

// A transfer whose replacement join cannot start lands in the menu, where a retry reuses them.
#[test]
fn a_failed_transfer_keeps_the_server_packs_for_a_rejoin() {
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
        FatalError::default(),
        |_| {},
        follow_server_transfer,
    );
    assert_eq!(kept.len(), 1);
}

#[test]
fn an_explicit_disconnect_keeps_the_server_packs_for_a_rejoin() {
    let root = tempfile::tempdir().unwrap();
    let kept = kept_stacks();
    run_session(
        root.path(),
        kept,
        ClientWorld::default(),
        FatalError::default(),
        |menu| {
            menu.open_pause();
            menu.activate(MenuAction::PauseDisconnect);
        },
        drive_session,
    );
    assert_eq!(kept.len(), 1);
}

#[test]
fn a_session_failure_keeps_the_server_packs_for_a_rejoin() {
    let root = tempfile::tempdir().unwrap();
    let kept = kept_stacks();
    run_session(
        root.path(),
        kept,
        ClientWorld::default(),
        FatalError(Some("network read failed: closed".to_owned())),
        |_| {},
        recover_session_failure,
    );
    assert_eq!(kept.len(), 1);
}

// Leaving kept the ended server's actor artwork as the menu's artwork, often GiBs of pack rasters.
#[test]
fn leaving_restores_the_startup_actor_artwork() {
    let (_, startup, _) = crate::tests::actor_rest_presentation::compiled_fixture(
        "1.0",
        1,
        assets::ActorPoseMode::CompiledLiteral,
    );
    let raster = assets::ActorTexture {
        source: 0,
        width: 2,
        height: 2,
        pixel_sha256: [0; 32],
        rgba8: std::sync::Arc::from(vec![9u8; 16]),
    };
    let session = startup.clone().with_pack_artwork(&[raster], &[]);
    assert_ne!(session.identity(), startup.identity());
    let root = tempfile::tempdir().unwrap();
    let mut app = session_app(
        root.path(),
        kept_stacks(),
        ClientWorld::default(),
        FatalError::default(),
        |menu| {
            menu.open_pause();
            menu.activate(MenuAction::PauseDisconnect);
        },
        drive_session,
    );
    app.insert_resource(render_scene::ActorArtworkResource(session));
    app.insert_resource(crate::session::StartupActorArtwork(startup.clone()));
    app.update();
    let artwork = app.world().resource::<render_scene::ActorArtworkResource>();
    assert_eq!(artwork.0.identity(), startup.identity());
}
