use super::*;
use bevy::prelude::{App, Update};
use client_shell::{menu::test_support as menu_fixture, ui_resources::UiRuntimeResource};
use client_ui::ui_runtime::UiRuntime;
use launcher::{accounts::AccountStore, install_layout::Platform};
use std::sync::Arc;

#[derive(Resource)]
struct Selection(Option<launcher_host::native_account::NativeGatheringSelection>);

/// Dispatches the actual typed entry without polling credential or transport preparation.
fn begin_selected(
    mut menu: ResMut<MenuRuntime>,
    mut session: SessionResources,
    mut selection: ResMut<Selection>,
) {
    if let Some(selection) = selection.0.take() {
        begin(&mut menu, &mut session, selection);
    }
}

/// Supplies the same bootstrap-only world used by the existing far-terrain fixtures.
fn stream() -> chunk_pipeline::WorldStream {
    chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    })
}

#[test]
fn native_gathering_replacement_restores_destination_for_world_consumers() {
    let root = tempfile::tempdir().unwrap();
    let layout = launcher::test_support::scratch_at(Platform::Linux, root.path());
    let account = Arc::new(launcher_host::native_account::NativeAccount::new(
        AccountStore::new(layout.auth_cache()),
        layout.device_profile_file(),
        None,
    ));
    let selection = launcher_host::native_account::test_support::gathering_selection(
        account,
        uuid::Uuid::from_u128(7),
    );
    let address = selection.connection_target().address;
    let expected_identity = format!("remote:{}", address.trim().to_lowercase());
    let mut menu = MenuRuntime::new_with_layout(
        true,
        Some(2),
        "Fixture".into(),
        layout.clone(),
        client_shell::player_skin::LocalPlayerSkin::generated_default("Fixture"),
    );
    menu_fixture::suppress_catalog(&mut menu);
    menu.show_world();
    let mut controller = SessionController::default()
        .with_native_transport(Some(protocol::NativeTransport::RakNet))
        .with_client_random_id(Some(protocol::ClientRandomId::new(1).unwrap()))
        .with_address(Some("previous.example:19132"));
    controller.trust = Some(Box::new(
        launcher_host::server_trust::NativeSessionTrust::new(layout.server_trust_file()),
    ));
    let old_generation = controller.generation;
    let mut ui = UiRuntime::new(old_generation);
    ui.experiences.select_destination("previous.example:19132");
    assert!(ui.experiences.audience.is_some());
    let mut app = App::new();
    app.insert_resource(menu)
        .insert_resource(controller)
        .insert_resource(Selection(Some(selection)))
        .insert_resource(NetworkHandle::<PackApplication>::disconnected())
        .insert_resource(client_runtime::pack_admission::ResourcePackAdmissionState::default())
        .insert_resource(crate::runtime::network::session_registries::tests::collision_resource())
        .insert_resource(UiRuntimeResource(ui))
        .insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
            old_generation,
        )))
        .insert_resource(crate::runtime::world::ClientWorld {
            stream: Some(stream()),
            ..Default::default()
        })
        .init_resource::<crate::runtime::shutdown::FatalError>()
        .insert_resource(client_interaction::movement::MovementTicker::default())
        .insert_resource(client_interaction::movement::LocalPhysicsController::default())
        .insert_resource(client_presentation::local_player::LocalPlayerFrameCarrier::default())
        .insert_resource(client_presentation::local_player::InteractionOriginSnapshot::default())
        .add_systems(Update, begin_selected);
    app.update();

    let controller = app.world().resource::<SessionController>();
    assert_ne!(controller.generation, old_generation);
    assert!(controller.connecting && controller.native_gathering.is_some());
    let join = controller.join.as_ref().unwrap();
    assert_eq!(join.generation, controller.generation);
    let JoinStage::Prepared(protocol::SessionEndpoint::Native(target)) = &join.stage else {
        panic!("the selected Gathering did not retain its native preparation owner");
    };
    assert_eq!(target.address, address);
    assert!(controller.core.id().is_none() && controller.directory.is_none());
    let identity = controller.destination_identity().map(str::to_owned);
    let trust_retired = controller.trust.is_none();
    let presence = controller.presence_target().cloned();
    let world = app.world().resource::<crate::runtime::world::ClientWorld>();
    assert!(world.stream.is_none(), "the old world survived replacement");
    assert_eq!(
        app.world().resource::<UiRuntimeResource>().session_id(),
        controller.generation
    );
    let audience = app
        .world()
        .resource::<UiRuntimeResource>()
        .experiences
        .audience
        .clone();

    let mut settings = client_state::RuntimeSettings::default();
    let mut user = settings.user_settings_update().1.clone();
    user.video.far_terrain = render_api::far_terrain::FarTerrainMode::On;
    settings.replace_user_settings(user);
    app.world_mut()
        .resource_mut::<crate::runtime::world::ClientWorld>()
        .stream = Some(stream());
    app.insert_resource(settings)
        .insert_resource(crate::far_terrain::FarTerrain::new(None))
        .init_resource::<render_scene::FarTerrainScene>()
        .add_systems(Update, crate::far_terrain::feed_far_terrain);
    app.update();
    let enabled = app
        .world()
        .resource::<crate::runtime::world::ClientWorld>()
        .stream
        .as_ref()
        .unwrap()
        .far_terrain_enabled();
    drop(app);
    drop(root);

    assert_eq!(
        identity.as_deref(),
        Some(expected_identity.as_str()),
        "the selected Gathering lost its destination after retiring the previous world"
    );
    assert!(
        enabled,
        "the selected Gathering disabled the actual far-terrain owner"
    );
    assert!(
        trust_retired,
        "the previous join retained its trust lifetime"
    );
    assert!(
        audience.is_none(),
        "the previous server retained its Experience audience"
    );
    assert!(matches!(presence, Some(rich_presence::Target {
        destination: rich_presence::Destination::Experience,
        join: Some(join), ..
    }) if join == address));
}
