use super::*;
use bevy::{ecs::system::RunSystemOnce, prelude::*};

/// Publishes the shell observation from the fixture's current screen owners.
fn publish(app: &mut App) -> ScreenFocus {
    app.world_mut()
        .run_system_once(publish_screen_focus)
        .unwrap();
    *app.world().resource::<ScreenFocus>()
}

#[test]
fn focus_tracks_menu_and_ui_changes_without_retaining_stale_ownership() {
    let mut app = App::new();
    app.init_resource::<ScreenFocus>()
        .insert_resource(PlayerRuntime(player_state::PlayerState::new(1)))
        .insert_resource(UiRuntime::new(1))
        .insert_resource(MenuRuntime::new(false, 2, "Focus".into()));
    assert!(!publish(&mut app).absorbs_input);

    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .set_visible(true);
    let focus = publish(&mut app);
    assert!(focus.menu_visible && focus.absorbs_input);

    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .set_visible(false);
    app.world_mut()
        .resource_scope(|world, mut player: Mut<PlayerRuntime>| {
            world.resource_mut::<UiRuntime>().open_chat(&mut player);
        });
    let focus = publish(&mut app);
    assert!(!focus.menu_visible && focus.ui_focused && focus.absorbs_input);

    app.world_mut().resource_mut::<UiRuntime>().close_chat();
    let focus = publish(&mut app);
    assert!(!focus.menu_visible && !focus.ui_focused && !focus.absorbs_input);
}

#[test]
fn shell_controls_publish_pick_binding_and_clear_the_next_frame() {
    let mut app = App::new();
    app.init_resource::<GameplayScreenControls>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .insert_resource(MenuRuntime::new(false, 2, "Controls".into()));
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Middle);
    app.world_mut()
        .run_system_once(publish_gameplay_screen_controls)
        .unwrap();
    assert!(
        app.world()
            .resource::<GameplayScreenControls>()
            .pick_block_pressed
    );
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.world_mut()
        .run_system_once(publish_gameplay_screen_controls)
        .unwrap();
    assert!(
        !app.world()
            .resource::<GameplayScreenControls>()
            .pick_block_pressed
    );
}
