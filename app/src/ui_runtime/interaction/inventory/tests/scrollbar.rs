//! Scrollbar ownership keeps inventory gesture edges and cancellation intact.

use super::*;

/// Supplies a scrollable view with an independently positioned thumb and track.
fn frame() -> ui_engine::frame::EngineFrame {
    let rect = json_ui::RectOut {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
    };
    let region = json_ui::HitRegion {
        key: "scroll".into(),
        name: "scroll".into(),
        kind: json_ui::HitKind::ScrollView,
        rect,
        clip: rect,
        layer: 0,
        order: 0,
        pressed: None,
        control_name: None,
        collection_index: None,
        collection: None,
        enabled: true,
        checked: None,
        max_length: None,
        group_index: None,
        renderer: None,
        drag_axes: [false; 2],
        sound: None,
        input: Default::default(),
        focus: None,
        collections: Vec::new(),
        widget: Default::default(),
        modal_root: None,
        transform: None,
    };
    let mut track = region.clone();
    track.key = "scroll/track".into();
    track.kind = json_ui::HitKind::ScrollTrack;
    track.pressed = Some("track".into());
    track.rect = json_ui::RectOut {
        x: 95.0,
        y: 0.0,
        w: 5.0,
        h: 100.0,
    };
    let mut thumb = track.clone();
    thumb.key = "scroll/thumb".into();
    thumb.kind = json_ui::HitKind::ScrollBox;
    thumb.rect.h = 50.0;
    ui_engine::frame::EngineFrame {
        hits: Arc::from([region, track, thumb]),
        report: json_ui::LayoutReport {
            scrolls: [(
                "scroll".to_owned(),
                json_ui::ScrollMetrics {
                    content: 200.0,
                    viewport: 100.0,
                    viewport_rect: Some([0.0, 0.0, 100.0, 100.0]),
                    track: Some([95.0, 0.0, 5.0, 100.0]),
                    thumb: Some([95.0, 0.0, 5.0, 50.0]),
                    thumb_key: Some("scroll/thumb".into()),
                    track_key: Some("scroll/track".into()),
                    box_drag: json_ui::Draggable::Vertical,
                    track_button: Some("track".into()),
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        },
        cancel_target: None,
        origin: [0.0; 2],
        scale: 1.0,
        panel: None,
        edit_texts: Vec::new(),
        top: Vec::new(),
    }
}

#[test]
fn engine_scrollbar_drag_moves_the_container_without_clicking_a_slot() {
    let frame = frame();
    let mut runtime = UiRuntime::new(1);
    assert!(scroll_container(
        &mut runtime,
        &frame,
        [97.0, 25.0],
        &[],
        true,
        false,
    ));
    assert_eq!(
        runtime.screen_state().container_scroll_drag,
        Some(("scroll".into(), 25.0))
    );
    assert!(scroll_container(
        &mut runtime,
        &frame,
        [97.0, 35.0],
        &[],
        false,
        false,
    ));
    assert_eq!(runtime.screen_state().container_scroll["scroll"], 20.0);
    assert!(scroll_container(
        &mut runtime,
        &frame,
        [97.0, 45.0],
        &[],
        false,
        false,
    ));
    assert_eq!(runtime.screen_state().container_scroll["scroll"], 40.0);
    assert!(scroll_container(
        &mut runtime,
        &frame,
        [97.0, 45.0],
        &[],
        false,
        true,
    ));
    assert!(runtime.screen_state().container_scroll_drag.is_none());
}

#[test]
fn wheel_scrolling_leaves_inventory_button_edges_available() {
    let mut runtime = UiRuntime::new(1);
    assert!(!scroll_container(
        &mut runtime,
        &frame(),
        [50.0, 50.0],
        &[(-20.0, MouseScrollUnit::Pixel)],
        false,
        true,
    ));
    assert!(runtime.screen_state().container_scroll["scroll"] > 0.0);
}

#[test]
fn a_complete_click_does_not_retain_scrollbar_capture() {
    let mut runtime = UiRuntime::new(1);
    assert!(scroll_container(
        &mut runtime,
        &frame(),
        [97.0, 25.0],
        &[],
        true,
        true
    ));
    assert!(runtime.screen_state().container_scroll_drag.is_none());
    assert!(!scroll_container(
        &mut runtime,
        &frame(),
        [97.0, 35.0],
        &[],
        false,
        false
    ));
}

#[test]
fn a_thumb_outside_the_track_and_viewport_can_be_dragged() {
    let mut frame = frame();
    let metrics = frame.report.scrolls.get_mut("scroll").unwrap();
    metrics.track = Some([95.0, 50.0, 5.0, 50.0]);
    metrics.viewport_rect = Some([0.0, 50.0, 90.0, 50.0]);
    let mut runtime = UiRuntime::new(1);
    assert!(scroll_container(
        &mut runtime,
        &frame,
        [97.0, 25.0],
        &[],
        true,
        false
    ));
    assert!(runtime.screen_state().container_scroll_drag.is_some());
}

#[test]
fn losing_window_focus_cancels_scrollbar_capture() {
    let (mut app, entity) = ordered::personal_app(2);
    app.update();
    app.world_mut()
        .resource_mut::<UiRuntimeResource>()
        .screen_state_mut()
        .container_scroll_drag = Some(("scroll".into(), 25.0));
    app.world_mut().get_mut::<Window>(entity).unwrap().focused = false;
    app.update();
    assert!(
        app.world()
            .resource::<UiRuntimeResource>()
            .screen_state()
            .container_scroll_drag
            .is_none()
    );
}

#[test]
fn withdrawing_the_pointer_cancels_scrollbar_capture() {
    let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
    let mut runtime = client_ui::test_support::inventory_session(&mut player);
    runtime.toggle_inventory(&mut player, None);
    runtime.observe_inventory_window(&player);
    let presentation = presentation(&player, &runtime);
    runtime.screen_state_mut().container_scroll_drag = Some(("scroll".into(), 25.0));
    let mut window = window(Vec2::ZERO);
    window.set_cursor_position(None);
    pointer_edge(
        &mut player,
        &mut runtime,
        &window,
        &presentation,
        false,
        true,
    );
    assert!(runtime.screen_state().container_scroll_drag.is_none());
}

#[test]
fn hidden_disabled_and_clipped_scrollbars_do_not_capture_presses() {
    for case in 0..3 {
        let mut frame = frame();
        match case {
            0 => Arc::make_mut(&mut frame.hits)[0].enabled = false,
            1 => frame.report.scrolls.get_mut("scroll").unwrap().bar_visible = Some(false),
            _ => {
                for region in Arc::make_mut(&mut frame.hits) {
                    region.clip.w = 90.0;
                }
            }
        }
        let mut runtime = UiRuntime::new(1);
        assert!(!scroll_container(
            &mut runtime,
            &frame,
            [97.0, 25.0],
            &[],
            true,
            false,
        ));
        assert!(runtime.screen_state().container_scroll_drag.is_none());
    }
}

#[test]
fn releasing_a_moved_thumb_applies_its_final_position() {
    let mut runtime = UiRuntime::new(1);
    let frame = frame();
    assert!(scroll_container(
        &mut runtime,
        &frame,
        [97.0, 25.0],
        &[],
        true,
        false,
    ));
    assert!(scroll_container(
        &mut runtime,
        &frame,
        [97.0, 35.0],
        &[],
        false,
        true,
    ));
    assert_eq!(
        runtime.screen_state().container_scroll.get("scroll"),
        Some(&20.0)
    );
    assert!(runtime.screen_state().container_scroll_drag.is_none());
}

#[test]
fn foreground_controls_and_disabled_thumbs_keep_their_pointer_ownership() {
    for foreground in [false, true] {
        let mut frame = frame();
        if foreground {
            let mut regions = frame.hits.to_vec();
            let mut button = regions.last().unwrap().clone();
            button.kind = json_ui::HitKind::Button;
            button.key = "foreground".into();
            regions.push(button);
            frame.hits = regions.into();
        } else {
            Arc::make_mut(&mut frame.hits)[2].enabled = false;
        }
        let mut runtime = UiRuntime::new(1);
        assert!(!scroll_container(
            &mut runtime,
            &frame,
            [97.0, 25.0],
            &[],
            true,
            false
        ));
        assert!(runtime.screen_state().container_scroll_drag.is_none());
    }
}

/// Builds a live scrollbar fixture and establishes its inventory window state.
fn scrollbar_app() -> (App, Entity, Vec2) {
    let (mut app, entity) = ordered::personal_app(2);
    let extra = serde_json::json!({"scroll": {
        "type": "scroll_view", "size": [100,100],
        "scroll_view_port": "viewport", "scroll_content": "content",
        "scrollbar_track": "track", "scrollbar_box": "thumb",
        "scroll_box_and_track_panel": "bars",
        "controls": [
            {"viewport": {"type": "panel", "size": [90,100],
                "controls": [{"content": {"type": "panel", "size": [90,200]}}]}},
            {"bars": {"type": "panel", "size": [10,100],
                "anchor_from": "right_middle", "anchor_to": "right_middle",
                "controls": [
                    {"track": {"type": "scroll_track", "size": [10,100]}},
                    {"thumb": {"type": "scrollbar_box", "size": [10,50],
                        "anchor_from": "top_middle", "anchor_to": "top_middle",
                        "draggable": "vertical"}}
                ]}}
        ]
    }});
    let presentation = presentation_with_controls(
        app.world().resource::<client_state::PlayerRuntime>(),
        &app.world().resource::<UiRuntimeResource>().0,
        &[extra],
    );
    let metrics = presentation
        .engine_container_frame()
        .unwrap()
        .report
        .scrolls
        .values()
        .next()
        .unwrap();
    let thumb = metrics.thumb.unwrap();
    let start = window_point(
        &presentation,
        [
            (thumb[0] + thumb[2] * 0.5) as f32,
            (thumb[1] + thumb[3] * 0.5) as f32,
        ],
    );
    app.world_mut()
        .insert_resource(UiPresentationResource(presentation));
    app.world_mut()
        .get_mut::<Window>(entity)
        .unwrap()
        .set_cursor_position(Some(start));
    app.update();
    (app, entity, start)
}

#[test]
fn a_release_before_a_new_press_keeps_the_new_scrollbar_drag_held() {
    use bevy::input::mouse::MouseButtonInput;
    let (mut app, entity, start) = scrollbar_app();
    for state in [ButtonState::Released, ButtonState::Pressed] {
        let mut buttons = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        match state {
            ButtonState::Released => {
                buttons.press(MouseButton::Left);
                buttons.release(MouseButton::Left);
            }
            ButtonState::Pressed => buttons.press(MouseButton::Left),
        }
        app.world_mut().write_message(MouseButtonInput {
            button: MouseButton::Left,
            state,
            window: entity,
        });
    }
    app.update();
    assert!(
        app.world()
            .resource::<UiRuntimeResource>()
            .screen_state()
            .container_scroll_drag
            .is_some()
    );
    app.world_mut()
        .get_mut::<Window>(entity)
        .unwrap()
        .set_cursor_position(Some(start + Vec2::new(0.0, 10.0)));
    app.update();
    assert!(
        app.world()
            .resource::<UiRuntimeResource>()
            .screen_state()
            .container_scroll
            .values()
            .any(|offset| *offset > 0.0)
    );
}

#[test]
fn a_new_press_replaces_an_older_scrollbar_capture() {
    let mut runtime = UiRuntime::new(1);
    runtime.screen_state_mut().container_scroll_drag = Some(("scroll".into(), 25.0));
    assert!(!scroll_container(
        &mut runtime,
        &frame(),
        [50.0, 50.0],
        &[],
        true,
        false,
    ));
    assert!(runtime.screen_state().container_scroll_drag.is_none());
}

#[test]
fn track_presses_follow_the_configured_button_mapping() {
    for mapping in [None, Some("other"), Some("track")] {
        let mut frame = frame();
        Arc::make_mut(&mut frame.hits)[1].pressed = mapping.map(str::to_owned);
        let mut runtime = UiRuntime::new(1);
        assert_eq!(
            scroll_container(&mut runtime, &frame, [97.0, 75.0], &[], true, false),
            mapping == Some("track")
        );
        assert_eq!(
            runtime
                .screen_state()
                .container_scroll
                .contains_key("scroll"),
            mapping == Some("track")
        );
    }
}

#[test]
fn consent_input_cancels_scrollbar_capture_before_ordinary_input_resumes() {
    let (mut app, _, _) = scrollbar_app();
    let key = app
        .world()
        .resource::<UiPresentationResource>()
        .engine_container_frame()
        .unwrap()
        .report
        .scrolls
        .keys()
        .next()
        .unwrap()
        .clone();
    app.world_mut()
        .resource_mut::<UiRuntimeResource>()
        .screen_state_mut()
        .container_scroll_drag = Some((key, 25.0));
    app.world_mut()
        .insert_resource(client_state::ConsentInput(true));
    app.update();
    assert!(
        app.world()
            .resource::<UiRuntimeResource>()
            .screen_state()
            .container_scroll_drag
            .is_none()
    );
    app.world_mut()
        .resource_mut::<client_state::ConsentInput>()
        .0 = false;
    app.update();
    assert!(
        app.world()
            .resource::<UiRuntimeResource>()
            .screen_state()
            .container_scroll
            .is_empty()
    );
}

#[test]
fn a_declined_track_press_still_handles_wheel_input() {
    let mut frame = frame();
    Arc::make_mut(&mut frame.hits)[1].pressed = None;
    let mut runtime = UiRuntime::new(1);
    assert!(!scroll_container(
        &mut runtime,
        &frame,
        [97.0, 75.0],
        &[(-20.0, MouseScrollUnit::Pixel)],
        true,
        false
    ));
    assert_eq!(
        runtime.screen_state().container_scroll.get("scroll"),
        Some(&20.0)
    );
}

#[test]
fn a_cancelled_close_press_clears_an_older_scrollbar_capture() {
    let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
    let mut runtime = client_ui::test_support::inventory_session(&mut player);
    runtime.toggle_inventory(&mut player, None);
    runtime.observe_inventory_window(&player);
    let presentation = presentation(&player, &runtime);
    let close = control_point(&presentation, InventoryCellHit::Widget(Widget::Close));
    runtime.screen_state_mut().container_scroll_drag = Some(("scroll".into(), 25.0));
    let mut window = window(close);
    pointer_edge(
        &mut player,
        &mut runtime,
        &window,
        &presentation,
        true,
        false,
    );
    window.set_cursor_position(Some(Vec2::new(1.0, 1.0)));
    pointer_edge(
        &mut player,
        &mut runtime,
        &window,
        &presentation,
        false,
        true,
    );
    assert!(runtime.inventory_open());
    assert!(runtime.screen_state().container_scroll_drag.is_none());
}
