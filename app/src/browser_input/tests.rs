use super::*;
use bevy::input::mouse::AccumulatedMouseScroll;
use client_shell::ui_resources::{UiPresentationResource, UiRuntimeResource};

#[test]
fn browser_canvas_capture_gates_raw_hotbar_scroll() {
    let mut app = App::new();
    app.init_resource::<CanvasCapture>()
        .init_resource::<BrowserOptions>()
        .init_resource::<semantic_controls::PendingDeviceFrame>()
        .init_resource::<client_state::GameplayScreenControls>()
        .init_resource::<client_state::ScreenFocus>()
        .init_resource::<client_state::SemanticInputSnapshot>()
        .insert_resource(UiRuntimeResource(client_ui::ui_runtime::UiRuntime::new(1)))
        .insert_resource(UiPresentationResource(
            client_ui::ui_runtime::presentation::UiPresentationRuntime::new(
                ui_engine::test_support::fixture_font(),
            )
            .unwrap(),
        ))
        .insert_resource(world::ClientWorld {
            stream: Some(chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
                dimension: 0,
                local_player_runtime_id: 1,
                local_player_unique_id: 1,
                player_position: [0.0; 3],
                world_spawn_position: [0; 3],
                air_network_id: 0,
                block_network_ids_are_hashes: false,
            })),
            ..Default::default()
        })
        .insert_resource(AccumulatedMouseScroll {
            delta: Vec2::new(0.0, -1.0),
            ..Default::default()
        })
        .add_systems(
            Update,
            (
                sample_input,
                screen_focus,
                client_interaction::hotbar::queue_hotbar_selection,
            )
                .chain(),
        );

    // Capture release and focus loss must block the raw wheel until capture returns.
    for (captured, ui_open, loaded, expected) in [
        (false, false, true, None),
        (true, false, true, Some(1)),
        (false, false, true, None),
        (true, false, true, Some(1)),
        (true, true, true, None),
        (true, false, false, None),
    ] {
        let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
        player.inventory.set_local_selected_slot(0);
        let mut ui = client_ui::ui_runtime::UiRuntime::new(1);
        if ui_open {
            ui.open_chat(&mut player);
        }
        app.insert_resource(player)
            .insert_resource(UiRuntimeResource(ui))
            .insert_non_send(InputSource(Box::new(move || semantic_input::DeviceFrame {
                window_focus_lost: !captured,
                ..Default::default()
            })));
        if !loaded {
            app.world_mut().resource_mut::<world::ClientWorld>().stream = None;
        }
        app.update();
        let player = app.world().resource::<client_state::PlayerRuntime>();
        assert_eq!(
            player.inventory.pending_hotbar_selection(),
            expected,
            "capture={captured}, ui={ui_open}, loaded={loaded}"
        );
        assert_eq!(player.selected_hotbar_slot(), Some(expected.unwrap_or(0)));
    }
}
