//! The side Back button dismisses both supported and diagnostic server forms.

use super::*;

#[test]
fn mouse_back_dismisses_server_forms_without_opening_pause() {
    for unsupported in [false, true] {
        let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
        let (mut app, _) = app_for(&mut player, unsupported);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Back);
        app.update();
        let runtime = app.world().resource::<UiRuntimeResource>();
        assert!(runtime.server_forms().active().is_none());
        assert!(!runtime.chat_focused() && !runtime.inventory_open());
        assert!(!app.world().resource::<MenuRuntime>().is_visible());
        assert!(
            app.world()
                .resource::<ButtonInput<MouseButton>>()
                .get_pressed()
                .next()
                .is_none()
        );
    }
}
