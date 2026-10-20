use super::*;
use bevy::prelude::{App, Update};
use client_shell::menu::test_support as menu_fixture;
use client_shell::ui_resources::UiRuntimeResource;
use client_ui::ui_runtime::UiRuntime;

/// Transfer discovery is scoped to that handoff; a fresh join uses the launch selection.
#[test]
fn native_lan_transfer_selects_addressed_transport_after_tearing_down_the_original_session() {
    let layout = launcher::test_support::scratch("native-lan-transfer");
    let mut menu = MenuRuntime::new_with_layout(
        true,
        Some(2),
        "Fixture".into(),
        layout,
        client_shell::player_skin::LocalPlayerSkin::generated_default("Fixture"),
    );
    menu_fixture::suppress_catalog(&mut menu);
    menu.show_world();
    let cache = BlobCache::default();
    let mut controller = SessionController::default()
        .with_native_transport(Some(protocol::NativeTransport::NetherNetLan))
        .with_client_random_id(Some(protocol::ClientRandomId::new(1).unwrap()));
    let old_generation = controller.generation;
    let JoinStage::Native(original) = controller
        .start_core(&menu, &cache, "127.0.0.1:19132", None, old_generation)
        .unwrap()
    else {
        panic!("original directed LAN join retains its native owner");
    };
    assert_eq!(original.transport, protocol::NativeTransport::NetherNetLan);
    let mut app = App::new();
    app.insert_resource(menu)
        .insert_resource(controller)
        .insert_resource(NetworkHandle::<PackApplication>::disconnected())
        .insert_resource(cache)
        .insert_resource(client_runtime::pack_admission::ResourcePackAdmissionState::default())
        .insert_resource(crate::runtime::network::session_registries::tests::collision_resource())
        .insert_resource(UiRuntimeResource(UiRuntime::new(old_generation)))
        .insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
            1,
        )))
        .insert_resource(crate::runtime::world::ClientWorld {
            transfer_notice: Some(TransferNotice {
                host: "127.0.0.1".into(),
                port: 19133,
            }),
            ..Default::default()
        })
        .init_resource::<crate::runtime::shutdown::FatalError>()
        .insert_resource(client_interaction::movement::MovementTicker::default())
        .insert_resource(client_interaction::movement::LocalPhysicsController::default())
        .insert_resource(client_presentation::local_player::LocalPlayerFrameCarrier::default())
        .insert_resource(client_presentation::local_player::InteractionOriginSnapshot::default())
        .add_systems(Update, follow_server_transfer);
    app.update();
    let controller = app.world().resource::<SessionController>();
    assert_ne!(controller.generation, old_generation);
    let join = controller
        .join
        .as_ref()
        .expect("transfer retained the native join");
    let JoinStage::Native(target) = &join.stage else {
        panic!("native transfer started a Core owner");
    };
    assert_eq!(target.address, "127.0.0.1:19133");
    assert_eq!(target.transport, protocol::NativeTransport::Addressed);
    target.validate().unwrap();
    assert!(controller.directory.is_none());
    assert!(controller.core.id().is_none());
    assert!(
        app.world()
            .resource::<crate::runtime::world::ClientWorld>()
            .transfer_notice
            .is_none()
    );

    let mut controller = app
        .world_mut()
        .remove_resource::<SessionController>()
        .unwrap();
    controller.begin_fresh_transfer_chain();
    let generation = controller.generation;
    let JoinStage::Native(next) = controller
        .start_core(
            app.world().resource::<MenuRuntime>(),
            app.world().resource::<BlobCache>(),
            "127.0.0.1:19134",
            None,
            generation,
        )
        .unwrap()
    else {
        panic!("the next user join must retain its native owner");
    };
    next.validate().unwrap();
    drop(controller);
    drop(app);
    assert_eq!(
        next.transport,
        protocol::NativeTransport::NetherNetLan,
        "a completed transfer must not replace the launch transport for the next user join"
    );
}
