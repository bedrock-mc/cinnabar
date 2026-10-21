//! Inventory controls retain their action and ownership across input dispatch paths.

use super::*;
use bevy::{
    input::{ButtonState, keyboard::Key, mouse::AccumulatedMouseMotion},
    prelude::*,
    window::{CursorOptions, PrimaryWindow, WindowEvent},
};
use client_shell::ui_resources::{UiPresentationResource, UiRuntimeResource};
use client_ui::ui_runtime::book_screen::{BookSource, BookState};
use std::sync::Arc;
use ui_screens::container_actions::ReaderButton;

const SIZE: [u32; 2] = [800, 600];

#[test]
fn mouse_back_closes_inventory_with_or_without_ordered_pointer_events() {
    use super::super::{drive_chat_keyboard_input, drive_inventory_ui_actions};
    use bevy::input::{keyboard::KeyboardInput, mouse::MouseButtonInput};
    for ordered in [false, true] {
        let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
        let mut runtime = client_ui::test_support::inventory_session(&mut player);
        runtime.toggle_inventory(&mut player, None);
        let presentation = presentation(&player, &runtime);
        let mut app = App::new();
        app.add_message::<KeyboardInput>()
            .add_message::<WindowEvent>()
            .init_resource::<Time<Real>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<AccumulatedMouseMotion>()
            .insert_resource(player)
            .insert_resource(UiRuntimeResource(runtime))
            .insert_resource(UiPresentationResource(presentation))
            .add_systems(
                Update,
                (drive_chat_keyboard_input, drive_inventory_ui_actions).chain(),
            );
        let window = app
            .world_mut()
            .spawn((
                Window {
                    focused: true,
                    ..Default::default()
                },
                CursorOptions::default(),
                PrimaryWindow,
            ))
            .id();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Back);
        if ordered {
            app.world_mut()
                .write_message(WindowEvent::MouseButtonInput(MouseButtonInput {
                    button: MouseButton::Back,
                    state: ButtonState::Pressed,
                    window,
                }));
        }
        app.update();
        assert!(
            !app.world().resource::<UiRuntimeResource>().inventory_open(),
            "ordered={ordered}"
        );
        assert!(
            !app.world()
                .resource::<ButtonInput<MouseButton>>()
                .just_pressed(MouseButton::Back)
        );
        app.update();
        assert!(!app.world().resource::<UiRuntimeResource>().inventory_open());
    }
}

/// Draws original inventory and book fixtures with a close control on each screen.
fn presentation(
    player: &client_state::PlayerRuntime,
    runtime: &UiRuntime,
) -> UiPresentationRuntime {
    let files = [
        ("ui/_global_variables.json".to_owned(), b"{}".to_vec()),
        (
            "ui/_ui_defs.json".to_owned(),
            br#"{"ui_defs":["ui/crafting.json","ui/book.json"]}"#.to_vec(),
        ),
    ]
    .into_iter()
    .chain(
        [
            // The catalog carrier requires paths in sorted order.
            ("book", "book_screen", "button.book_exit"),
            ("crafting", "inventory_screen", "button.menu_exit"),
        ]
        .map(|(namespace, root, action)| {
            let source = serde_json::json!({
                "namespace": namespace,
                (root): {
                    "type": "screen", "size": [160, 100],
                    "button_mappings": [{"from_button_id": "button.menu_cancel",
                        "to_button_id": action, "mapping_type": "global"}],
                    "controls": [{"close": {
                        "type": "button", "size": [20, 20],
                        "button_mappings": [{"from_button_id": "button.menu_select",
                            "to_button_id": action, "mapping_type": "pressed"}]
                    }}]
                }
            });
            (
                format!("ui/{namespace}.json"),
                serde_json::to_vec(&source).unwrap(),
            )
        }),
    )
    .map(|(path, bytes)| assets::UiFile {
        path: path.into(),
        bytes: bytes.into(),
    })
    .collect::<Vec<_>>();
    let page = assets::UiAtlasPage {
        width: 4,
        height: 4,
        rgba8: vec![255; 64].into(),
    };
    let bytes = assets::encode_ui_catalog([1; 32], &[page], &[], &[], &files).unwrap();
    let mut presentation = UiPresentationRuntime::with_hud(
        ui_engine::test_support::fixture_font(),
        client_ui::test_support::fixture_hud(),
    )
    .unwrap();
    presentation
        .enable_json_ui(Arc::new(assets::RuntimeUiAssets::decode(&bytes).unwrap()))
        .unwrap();
    redraw(&mut presentation, player, runtime);
    presentation
}

/// Publishes the current inventory and mod screen frames with deterministic geometry.
fn redraw(
    presentation: &mut UiPresentationRuntime,
    player: &client_state::PlayerRuntime,
    runtime: &UiRuntime,
) {
    presentation
        .build(player, runtime, 0, SIZE, ui::DpiScale::new(1.0).unwrap())
        .unwrap();
}

/// Locates a control by its routed action in window coordinates.
fn control_point(presentation: &UiPresentationRuntime, action: InventoryCellHit) -> Vec2 {
    let frame = presentation
        .engine_container_frame()
        .expect("fixture container frame");
    let gui = frame
        .hits
        .iter()
        .find_map(|hit| {
            let gui = [
                (hit.rect.x + hit.rect.w / 2.0) as f32,
                (hit.rect.y + hit.rect.h / 2.0) as f32,
            ];
            (presentation.engine_container_hit(gui) == Some(action)).then_some(gui)
        })
        .expect("fixture control is reachable");
    window_point(presentation, gui)
}

/// Converts a GUI hit point into logical window coordinates.
fn window_point(presentation: &UiPresentationRuntime, gui: [f32; 2]) -> Vec2 {
    let origin = presentation
        .inventory_gui_point(UiPoint::new(0.0, 0.0).unwrap(), SIZE, 1.0)
        .unwrap();
    let unit = presentation
        .inventory_gui_point(UiPoint::new(1.0, 1.0).unwrap(), SIZE, 1.0)
        .unwrap();
    Vec2::new(
        (gui[0] - origin[0]) / (unit[0] - origin[0]),
        (gui[1] - origin[1]) / (unit[1] - origin[1]),
    )
}

/// Creates a focused primary window positioned over a drawn control.
fn window(at: Vec2) -> Window {
    let mut window = Window {
        focused: true,
        ..Default::default()
    };
    window.resolution.set_scale_factor_override(Some(1.0));
    window.resolution.set_physical_resolution(SIZE[0], SIZE[1]);
    window.set_cursor_position(Some(at));
    window
}

/// Applies one primary pointer edge without a platform backend.
fn pointer_edge(
    player: &mut client_state::PlayerRuntime,
    runtime: &mut UiRuntime,
    window: &Window,
    presentation: &UiPresentationRuntime,
    pressed: bool,
    released: bool,
) {
    let mut buttons = ButtonInput::default();
    if pressed {
        buttons.press(MouseButton::Left);
    }
    apply_inventory_pointer(
        player,
        runtime,
        window,
        presentation,
        None,
        &mut buttons,
        (released, false),
        (Vec::new(), false, false),
        &[],
        None,
        0,
    );
}

#[test]
fn fallback_recipe_hits_follow_the_controller_layout() {
    use client_ui::ui_runtime::presentation::screens;

    let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
    let mut runtime = client_ui::test_support::inventory_session(&mut player);
    runtime.toggle_inventory(&mut player, None);
    let mut presentation = UiPresentationRuntime::with_hud(
        ui_engine::test_support::fixture_font(),
        client_ui::test_support::fixture_hud(),
    )
    .unwrap();
    let gui_size = presentation
        .inventory_gui_point(
            UiPoint::new(SIZE[0] as f32, SIZE[1] as f32).unwrap(),
            SIZE,
            1.0,
        )
        .unwrap();
    let origin = screens::panel_origin(InventoryScreen::Personal, gui_size);
    let cell = screens::book_cell_origin(4, screens::book_origin(origin));
    let window = window(window_point(&presentation, [cell[0] + 1.0, cell[1] + 1.0]));
    for (layout, expected) in [
        (2, Some(InventoryCellHit::Widget(Widget::BookRecipe(4)))),
        (1, None),
        (2, Some(InventoryCellHit::Widget(Widget::BookRecipe(4)))),
    ] {
        runtime.perform_pointer_action(
            &mut player,
            client_ui::ui_runtime::inventory_drag::PointerAction::Click(InventoryCellHit::Widget(
                Widget::InventoryLayout(layout),
            )),
        );
        redraw(&mut presentation, &player, &runtime);
        assert!(presentation.engine_container_frame().is_none());
        pointer_edge(
            &mut player,
            &mut runtime,
            &window,
            &presentation,
            false,
            false,
        );
        assert_eq!(runtime.screen_state().hover, expected);
    }
}

#[test]
fn book_exit_retains_written_pages_before_closing() {
    let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
    let mut runtime = client_ui::test_support::inventory_session(&mut player);
    let mut book = BookState::new(
        BookSource::Held(0),
        vec!["old".into()],
        true,
        String::new(),
        String::new(),
    );
    book.type_text(" edit");
    runtime.open_book(book);
    let presentation = presentation(&player, &runtime);
    let at = control_point(
        &presentation,
        InventoryCellHit::Widget(Widget::Reader(ReaderButton::Done)),
    );
    let window = window(at);
    pointer_edge(
        &mut player,
        &mut runtime,
        &window,
        &presentation,
        true,
        false,
    );
    pointer_edge(
        &mut player,
        &mut runtime,
        &window,
        &presentation,
        false,
        true,
    );
    assert!(!runtime.inventory_open());
    assert!(runtime.screen_state().book.is_none());
    assert_eq!(
        runtime.take_client_packet(),
        protocol::book_edit_packet(
            0,
            &protocol::BookEdit::ReplacePage {
                page: 0,
                text: "old edit".into()
            }
        )
    );
    assert!(runtime.take_client_packet().is_none());
}

#[test]
fn hidden_inventory_close_control_does_not_take_a_mod_views_pointer() {
    let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
    let mut runtime = client_ui::test_support::inventory_session(&mut player);
    runtime.toggle_inventory(&mut player, None);
    let mut presentation = presentation(&player, &runtime);
    let at = control_point(&presentation, InventoryCellHit::Widget(Widget::Close));
    let window = window(at);
    // Also discard a press armed before the view replaced the visible inventory.
    for arm_first in [false, true] {
        if arm_first {
            pointer_edge(
                &mut player,
                &mut runtime,
                &window,
                &presentation,
                true,
                false,
            );
        }
        let view = "ui/view.json".to_owned();
        let files = Arc::new(server_experience::screen::Files {
            namespace: "pointer_test".into(),
            templates: [(
                view.clone(),
                br#"{"namespace":"pointer_test","view":{"type":"panel","size":["100%","100%"]}}"#
                    .to_vec(),
            )]
            .into(),
            textures: Vec::new(),
        });
        presentation.set_mod_screens(Some(mod_ui::mod_screens::ModScreensInput {
            id: "pointer_test",
            files: &files,
            overlay: None,
            view: Some(&view),
            focus: None,
            data: &server_experience::screen::Modal::default(),
        }));
        redraw(&mut presentation, &player, &runtime);
        assert!(presentation.engine_container_frame().is_some());
        assert!(presentation.mod_ui().mod_screens_own(at.to_array()));
        pointer_edge(
            &mut player,
            &mut runtime,
            &window,
            &presentation,
            !arm_first,
            false,
        );
        pointer_edge(
            &mut player,
            &mut runtime,
            &window,
            &presentation,
            false,
            true,
        );
        assert!(runtime.inventory_open());
        presentation.set_mod_screens(None);
        redraw(&mut presentation, &player, &runtime);
        pointer_edge(
            &mut player,
            &mut runtime,
            &window,
            &presentation,
            false,
            true,
        );
        assert!(
            runtime.inventory_open(),
            "the mod-owned release must discard the earlier press"
        );
    }
}

#[test]
fn ordered_close_press_survives_keyboard_dispatch_until_frame_release() {
    use crate::ui_runtime::interaction::{drive_chat_keyboard_input, drive_inventory_ui_actions};
    use bevy::input::{keyboard::KeyboardInput, mouse::MouseButtonInput};
    let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
    let mut runtime = client_ui::test_support::inventory_session(&mut player);
    runtime.toggle_inventory(&mut player, None);
    let presentation = presentation(&player, &runtime);
    let at = control_point(&presentation, InventoryCellHit::Widget(Widget::Close));
    let mut app = App::new();
    app.add_message::<KeyboardInput>()
        .add_message::<MouseButtonInput>()
        .add_message::<WindowEvent>()
        .init_resource::<Time<Real>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .insert_resource(player)
        .insert_resource(UiRuntimeResource(runtime))
        .insert_resource(UiPresentationResource(presentation))
        .add_systems(
            Update,
            (drive_chat_keyboard_input, drive_inventory_ui_actions).chain(),
        );
    let entity = app
        .world_mut()
        .spawn((window(at), CursorOptions::default(), PrimaryWindow))
        .id();
    let mouse = MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Pressed,
        window: entity,
    };
    let key = KeyboardInput {
        key_code: KeyCode::KeyR,
        logical_key: Key::Character("r".into()),
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window: entity,
    };
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.world_mut().write_message(mouse);
    app.world_mut()
        .write_message(WindowEvent::MouseButtonInput(mouse));
    app.world_mut().write_message(key.clone());
    app.world_mut()
        .write_message(WindowEvent::KeyboardInput(key));
    app.update();
    assert!(app.world().resource::<UiRuntimeResource>().inventory_open());
    app.world_mut().write_message(MouseButtonInput {
        state: ButtonState::Released,
        ..mouse
    });
    app.update();
    assert!(!app.world().resource::<UiRuntimeResource>().inventory_open());
}

/// Plays one touch through the pointer emulation, delivering each edge at its own position.
fn batched_tap(
    player: &mut client_state::PlayerRuntime,
    runtime: &mut UiRuntime,
    presentation: &UiPresentationRuntime,
    (down, up): (Vec2, Vec2),
) {
    use crate::ui_runtime::touch_pointer::{PointerEdge, TouchPointer};
    use bevy::input::touch::TouchPhase;

    let mut pointer = TouchPointer::default();
    pointer.queue(true, 1, TouchPhase::Started, down);
    pointer.queue(true, 1, TouchPhase::Ended, up);
    while let (Some(Some(at)), Some(edge)) = pointer.next_frame() {
        let pressed = edge == PointerEdge::Press;
        pointer_edge(
            player,
            runtime,
            &window(at),
            presentation,
            pressed,
            !pressed,
        );
    }
}

/// A touch that lands off the close button and lifts over it in one batch never presses it,
/// because each emulated edge is hit-tested where it happened.
#[test]
fn a_batched_touch_lifted_over_close_does_not_close() {
    let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
    let mut runtime = client_ui::test_support::inventory_session(&mut player);
    runtime.toggle_inventory(&mut player, None);
    let presentation = presentation(&player, &runtime);
    let close = control_point(&presentation, InventoryCellHit::Widget(Widget::Close));
    let off_close = Vec2::new(1.0, 1.0);
    batched_tap(&mut player, &mut runtime, &presentation, (off_close, close));
    assert!(runtime.inventory_open());
    batched_tap(&mut player, &mut runtime, &presentation, (close, close));
    assert!(!runtime.inventory_open(), "a tap on close still closes");
}
