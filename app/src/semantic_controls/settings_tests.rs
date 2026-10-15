//! Settings handoff coverage with held physical gameplay input.

use bevy::{input::keyboard::KeyboardInput, prelude::*, window::PrimaryWindow};
use client_shell::{
    menu::{HOTKEY_OPTIONS, MenuClipboard, MenuRuntime, drive_menu_input},
    ui_resources::{UiPresentationResource, UiRuntimeResource},
};
use client_state::{PlayerRuntime, RuntimeSettings, ScreenFocus, SemanticInputSnapshot};
use semantic_input::{Action, PhysicalControl, ReleaseReason};

use super::{
    PendingDeviceFrame, SemanticInputRuntime, SemanticRouteState, SemanticTouchTargets,
    collect_raw_input, finalize_semantic_input_after_ui_authority, route_semantic_input,
    synchronize_semantic_input_authority,
};

/// Publishes menu edits through the same settings handoff as the client shell.
fn publish_settings(mut menu: ResMut<MenuRuntime>, settings: ResMut<RuntimeSettings>) {
    menu.sync_user_settings(Some(settings));
}

/// Builds the input and settings path without a renderer or installed carriers.
fn input_app() -> App {
    let layout = launcher::test_support::scratch("held-input-settings");
    let skin = client_shell::player_skin::LocalPlayerSkin::generated_default("Player");
    let mut menu = MenuRuntime::new_with_layout(false, Some(2), "Player".into(), layout, skin);
    menu.set_transient_toggles(true);
    let mut app = App::new();
    app.add_message::<KeyboardInput>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<bevy::input::touch::Touches>()
        .init_resource::<bevy::input::mouse::AccumulatedMouseMotion>()
        .init_resource::<MenuClipboard>()
        .init_resource::<RuntimeSettings>()
        .init_resource::<ScreenFocus>()
        .init_resource::<SemanticInputRuntime>()
        .init_resource::<SemanticTouchTargets>()
        .init_resource::<PendingDeviceFrame>()
        .init_resource::<SemanticRouteState>()
        .init_resource::<SemanticInputSnapshot>()
        .insert_resource(PlayerRuntime(player_state::PlayerState::new(1)))
        .insert_resource(menu)
        .insert_resource(UiRuntimeResource(client_ui::ui_runtime::UiRuntime::new(1)))
        .insert_resource(UiPresentationResource(
            client_ui::test_support::mini_engine_presentation(),
        ))
        .add_systems(
            Update,
            (
                collect_raw_input,
                route_semantic_input,
                drive_menu_input,
                publish_settings,
                synchronize_semantic_input_authority,
                finalize_semantic_input_after_ui_authority,
            )
                .chain(),
        );
    app.world_mut().spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        bevy::window::CursorOptions {
            grab_mode: bevy::window::CursorGrabMode::Locked,
            visible: false,
            ..Default::default()
        },
        PrimaryWindow,
    ));
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyW);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Space);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    clear_edges(&mut app);
    app
}

/// Advances physical input edges without releasing held controls.
fn clear_edges(app: &mut App) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
}

/// Checks that display changes neither release nor press held gameplay actions again.
fn assert_continuously_held(app: &App) {
    let input = app.world().resource::<SemanticInputSnapshot>();
    assert_eq!(input.movement(), [0.0, 1.0]);
    let snapshot = input.snapshot().unwrap();
    assert!(!snapshot.authority_released);
    for action in [Action::MoveForward, Action::Jump, Action::Attack] {
        let phase = input.phase(action);
        assert!(phase.held, "{action:?}");
        assert!(!phase.pressed && !phase.released, "{action:?}");
        assert_eq!(snapshot.release_reasons[action as usize], None);
    }
}

#[test]
fn hud_hotkeys_preserve_held_gameplay_input_when_hidden_and_shown() {
    for (key, option) in HOTKEY_OPTIONS {
        let mut app = input_app();
        let initial = app
            .world()
            .resource::<MenuRuntime>()
            .settings_snapshot()
            .0
            .value(option);
        for expected in [1 - initial, initial] {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(key);
            app.update();
            assert_eq!(
                app.world()
                    .resource::<MenuRuntime>()
                    .settings_snapshot()
                    .0
                    .value(option),
                expected,
            );
            assert_continuously_held(&app);
            clear_edges(&mut app);
            app.update();
            assert_continuously_held(&app);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .release(key);
            app.update();
            assert_continuously_held(&app);
            clear_edges(&mut app);
        }
    }
}

#[test]
fn changed_controls_still_release_and_quarantine_held_input() {
    let mut app = input_app();
    let row = launcher::menu::settings_options::KEY_BINDINGS
        .iter()
        .position(|(action, _)| *action == Action::Jump)
        .unwrap();
    let jump = client_shell::physical_input::keyboard_usage(KeyCode::KeyR).unwrap();
    assert!(client_shell::menu::test_support::remap_control(
        &mut app.world_mut().resource_mut::<MenuRuntime>(),
        row,
        PhysicalControl::KeyboardUsage(jump),
    ));
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .set_session_option(
            launcher::menu::settings_options::SettingId::HideHud,
            Some(1),
        );
    app.update();
    let input = app.world().resource::<SemanticInputSnapshot>();
    assert_eq!(input.movement(), [0.0; 2]);
    assert_eq!(
        input.snapshot().unwrap().release_reasons[Action::MoveForward as usize],
        Some(ReleaseReason::BindingChanged),
    );
    clear_edges(&mut app);
    app.update();
    assert_eq!(
        app.world().resource::<SemanticInputSnapshot>().movement(),
        [0.0; 2]
    );
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::KeyW);
    app.update();
    clear_edges(&mut app);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyW);
    app.update();
    assert_eq!(
        app.world().resource::<SemanticInputSnapshot>().movement(),
        [0.0, 1.0]
    );
    assert!(
        !app.world()
            .resource::<SemanticInputSnapshot>()
            .phase(Action::Jump)
            .held
    );
    clear_edges(&mut app);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyR);
    app.update();
    assert!(
        app.world()
            .resource::<SemanticInputSnapshot>()
            .phase(Action::Jump)
            .pressed
    );
}
