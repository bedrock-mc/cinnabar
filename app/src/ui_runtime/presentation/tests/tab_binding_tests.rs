use bevy::{
    ecs::schedule::IntoScheduleConfigs,
    input::ButtonInput,
    input::mouse::AccumulatedMouseMotion,
    input::touch::Touches,
    prelude::{App, KeyCode, MouseButton, Time, Update},
    time::Real,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow, Window},
};
use semantic_input::Action;

use super::fixture_hud;
use crate::{
    semantic_controls::{
        PendingDeviceFrame, SemanticInputRuntime, SemanticInputSnapshot, SemanticRouteState,
        SemanticTouchTargets, collect_raw_input, finalize_semantic_input_after_ui_authority,
        route_semantic_input,
    },
    ui_runtime::{
        UiRuntime,
        presentation::{UiPresentationRuntime, publish::observe_mount_jump_input},
    },
};

#[test]
fn the_default_tab_binding_drives_the_player_list_overlay_end_to_end() {
    let mut app = App::new();
    app.init_resource::<Time<Real>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<Touches>()
        .init_resource::<PendingDeviceFrame>()
        .init_resource::<SemanticRouteState>()
        .init_resource::<SemanticInputRuntime>()
        .init_resource::<SemanticTouchTargets>()
        .init_resource::<SemanticInputSnapshot>();
    app.add_systems(
        Update,
        (
            collect_raw_input,
            route_semantic_input,
            finalize_semantic_input_after_ui_authority,
            observe_mount_jump_input,
        )
            .chain(),
    );
    let mut runtime = UiRuntime::new(1);
    runtime.refresh_raw_text_identities(|_| None, Vec::new());
    app.insert_resource(runtime);
    app.insert_resource(
        UiPresentationRuntime::with_hud(super::fixture_font(), fixture_hud()).unwrap(),
    );
    app.world_mut().spawn((
        Window {
            focused: true,
            ..Window::default()
        },
        CursorOptions {
            grab_mode: CursorGrabMode::Locked,
            visible: false,
            ..CursorOptions::default()
        },
        PrimaryWindow,
    ));
    app.update();
    assert!(
        !app.world()
            .resource::<UiPresentationRuntime>()
            .hud_frame()
            .tab_list_open
    );

    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Tab);
    app.update();

    assert!(
        app.world()
            .resource::<SemanticInputSnapshot>()
            .phase(Action::PlayerList)
            .held,
        "the default Tab binding must reach the gameplay snapshot"
    );
    assert!(
        app.world()
            .resource::<UiPresentationRuntime>()
            .hud_frame()
            .tab_list_open,
        "held Tab must open the player-list overlay through the real input chain"
    );

    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.release(KeyCode::Tab);
        keys.clear();
    }
    app.update();

    assert!(
        !app.world()
            .resource::<UiPresentationRuntime>()
            .hud_frame()
            .tab_list_open,
        "releasing Tab must close the overlay on the same publication path"
    );
}
