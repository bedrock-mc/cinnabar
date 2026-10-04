//! A left click during gameplay must reach the attack producers through the production input chain.
use crate::player_runtime::PlayerRuntime;
use bevy::{
    input::{ButtonInput, keyboard::KeyboardInput, mouse::AccumulatedMouseMotion, touch::Touches},
    prelude::{App, MouseButton},
    time::{Real, Time},
    window::{CursorGrabMode, CursorOptions, PrimaryWindow, Window},
};
use semantic_input::Action;

use bevy::prelude::{IntoScheduleConfigs, Update};

use crate::{
    app::{ClientFrameSet, configure_client_frame_schedule},
    menu::{MenuClipboard, MenuRuntime, drive_menu_input},
    runtime::world::ClientWorld,
    semantic_controls::{
        SemanticInputSnapshot, collect_raw_input, finalize_semantic_input_after_ui_authority,
        route_semantic_input, synchronize_semantic_input_authority,
    },
    settings_runtime::RuntimeSettings,
    ui_runtime::{
        drain_inventory_authority, drive_chat_keyboard_input, drive_chat_ui_actions,
        drive_inventory_ui_actions, drive_server_form_input, drive_sign_editor,
        drive_world_inventory_keys, gameplay_touch::drive_gameplay_touch_targets,
        presentation::tests::fixture_font,
    },
};
use client_ui::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};

fn gameplay_app(menu_visible: bool) -> App {
    let mut player_runtime = PlayerRuntime::new(1);
    let mut app = App::new();
    configure_client_frame_schedule(&mut app);
    // The production RawInput/SemanticSample/UiAuthority/SemanticFinalize registrations.
    app.add_message::<bevy::input::mouse::MouseWheel>()
        .add_systems(
            Update,
            (drive_gameplay_touch_targets, collect_raw_input)
                .chain()
                .in_set(ClientFrameSet::RawInput),
        )
        .add_systems(
            Update,
            route_semantic_input.in_set(ClientFrameSet::SemanticSample),
        )
        .add_systems(
            Update,
            (
                drive_sign_editor,
                drive_server_form_input,
                drive_chat_ui_actions,
                drain_inventory_authority,
                drive_chat_keyboard_input,
                drive_menu_input,
                drive_inventory_ui_actions,
                // Connection and store drivers own no input and are left out.
                synchronize_semantic_input_authority,
                drive_world_inventory_keys,
            )
                .chain()
                .in_set(ClientFrameSet::UiAuthority),
        )
        .add_systems(
            Update,
            finalize_semantic_input_after_ui_authority.in_set(ClientFrameSet::SemanticFinalize),
        );
    let mut runtime = UiRuntime::new(1);
    runtime
        .publish_local_runtime_id(&mut player_runtime, 1, 42)
        .unwrap();
    app.init_resource::<Time<Real>>()
        .init_resource::<Time>()
        .init_resource::<ButtonInput<bevy::input::keyboard::KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<Touches>()
        .init_resource::<MenuClipboard>()
        .init_resource::<crate::semantic_controls::SemanticInputRuntime>()
        .init_resource::<SemanticInputSnapshot>()
        .init_resource::<crate::semantic_controls::PendingDeviceFrame>()
        .init_resource::<crate::semantic_controls::SemanticRouteState>()
        .init_resource::<crate::semantic_controls::SemanticTouchTargets>()
        .init_resource::<RuntimeSettings>()
        .init_resource::<ClientWorld>()
        .init_resource::<crate::local_player::LocalPlayerFrameCarrier>()
        .init_resource::<crate::local_player::InteractionOriginSnapshot>()
        .add_message::<KeyboardInput>()
        .insert_resource(runtime)
        .insert_resource(player_runtime.clone())
        .insert_resource(UiPresentationRuntime::new(fixture_font()).unwrap())
        .insert_resource(MenuRuntime::new(menu_visible, 2, "Tester".to_owned()));
    app.world_mut().spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        CursorOptions {
            grab_mode: CursorGrabMode::Locked,
            visible: false,
            ..Default::default()
        },
        PrimaryWindow,
    ));
    app
}

fn click_phase(app: &mut App) -> semantic_input::ActionPhase {
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    app.world()
        .resource::<SemanticInputSnapshot>()
        .phase(Action::Attack)
}

/// A gameplay click becomes an attack press; with a menu open it does not.
#[test]
fn a_gameplay_click_reaches_attack_only_without_a_menu() {
    assert!(click_phase(&mut gameplay_app(false)).pressed);
    assert!(!click_phase(&mut gameplay_app(true)).pressed);
}

#[test]
fn consent_discards_semantic_input_collected_before_approval() {
    let mut app = gameplay_app(false);
    app.update();
    app.insert_resource(crate::server_experiences::input::ConsentInput(true));
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    let snapshot = app.world().resource::<SemanticInputSnapshot>();
    assert!(!snapshot.phase(Action::Attack).pressed);
    assert!(!snapshot.phase(Action::Attack).held);
    assert_eq!(snapshot.movement(), [0.0; 2]);
}
