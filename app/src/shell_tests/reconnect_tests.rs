use client_shell::menu::MenuRuntime;
use launcher::menu::{MenuAction, MenuScreen};
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
    menu.suppress_catalog_for_test();
    menu.use_missing_core_for_test();
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
        .insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
            1,
        )))
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
