//! The launcher's text boxes through the engine's Add Server screen: the caret
//! drawn where typing goes, its blink, and a press placing it by character.
//! Owned menu fields render without the installed JSON-UI carrier.

use bevy::prelude::{App, ButtonInput, KeyCode, MouseButton, Vec2, Window};
use ui::{DpiScale, UiNode, UiVisual};

use super::super::UiPresentationResource;
use client_ui::test_support::pack_harness::{drawn_texts, font, menu_nodes};
use client_ui::ui_runtime::UiRuntime;
use {
    crate::ui_runtime::tests::menu_input_tests::{menu_input_app_with, press_key},
    client_shell::menu::{MenuClipboard, MenuRuntime},
    launcher::menu::{MenuAction, MenuField, MenuScreen, worlds_tab::LocalWorldAction},
};

const SIZE: [u32; 2] = [1280, 720];

/// Drives settled OreUI fields through the production menu input system.
fn menu_app() -> (App, bevy::prelude::Entity) {
    let presentation =
        client_ui::ui_runtime::presentation::UiPresentationRuntime::new(font()).unwrap();
    let (mut app, window) = menu_input_app_with(MenuClipboard::default(), presentation);
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::SettingsOption(
            launcher::menu::settings_options::SettingId::ScreenAnimations.index() as u16,
            0,
        ));
    (app, window)
}

/// Builds the menu at `now_millis` (which also sets its hit targets) and returns its nodes.
fn frame(app: &mut App, now_millis: u64) -> Vec<UiNode> {
    let view = app.world().resource::<MenuRuntime>().view();
    app.world_mut().resource_scope(
        |world, mut presentation: bevy::prelude::Mut<UiPresentationResource>| {
            presentation.set_menu_view(Some(view));
            let runtime = UiRuntime::new(1);
            let dpi = DpiScale::new(1.0).unwrap();
            presentation
                .build(
                    world.resource::<client_state::PlayerRuntime>(),
                    &runtime,
                    now_millis,
                    SIZE,
                    dpi,
                )
                .unwrap();
            menu_nodes(&presentation).to_vec()
        },
    )
}

/// Opens the server editor with its name field focused.
fn add_server(app: &mut App) {
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::PlayAddServer);
}

#[test]
fn the_focused_box_draws_the_caret_where_typing_goes_and_blinks() {
    let (mut app, window) = menu_app();
    add_server(&mut app);
    press_key(&mut app, window, KeyCode::KeyA, Some("abc"));
    press_key(&mut app, window, KeyCode::ArrowLeft, None);
    let blink = (json_ui::CARET_BLINK_SECONDS * 1_000.0) as u64;
    let start = 10_000;
    let shown = frame(&mut app, start);
    assert_eq!(caret_byte(&shown, "abc"), Some(2));
    let hidden = frame(&mut app, start + blink + blink / 2);
    assert_eq!(caret_byte(&hidden, "abc"), None, "blinks off");
    assert!(drawn_texts(&hidden).contains(&"abc".to_owned()));
    press_key(&mut app, window, KeyCode::ArrowRight, None);
    let moved = frame(&mut app, start + blink + blink / 2 + 1);
    assert_eq!(
        caret_byte(&moved, "abc"),
        Some(3),
        "a move shows the caret at once"
    );
    press_key(&mut app, window, KeyCode::ShiftLeft, None);
    press_key(&mut app, window, KeyCode::ArrowLeft, None);
    let selected = frame(&mut app, start + blink + blink / 2 + 2);
    assert_eq!(
        app.world().resource::<MenuRuntime>().view().caret.selection,
        Some([2, 3])
    );
    assert_eq!(caret_byte(&selected, "abc"), None);
    let selection = selected
        .iter()
        .find(|node| matches!(node.visual(), UiVisual::InvertedSprite { .. }))
        .expect("the selected character is highlighted");
    let text = selected
        .iter()
        .find(|node| {
            matches!(node.visual(), UiVisual::Text { layout, .. }
        if layout.glyphs().iter().map(|glyph| glyph.codepoint).collect::<String>() == "abc")
        })
        .expect("selected text remains visible");
    assert!(selection.bounds().width() > 0.0);
    assert!(
        selection.bounds().width() < text.bounds().width(),
        "only the selected character is highlighted"
    );
}

/// Resolves a retained node through every clipping ancestor into window coordinates.
fn node_origin(node: &UiNode, nodes: &[UiNode]) -> [f32; 2] {
    let mut origin = [node.bounds().min().x(), node.bounds().min().y()];
    let mut parent = node.parent();
    while let Some(ancestor) = parent.and_then(|id| nodes.iter().find(|node| node.id() == id)) {
        origin[0] += ancestor.bounds().min().x();
        origin[1] += ancestor.bounds().min().y();
        parent = ancestor.parent();
    }
    origin
}

/// Reads the painted insertion position against the same text layout that draws the field.
fn caret_byte(nodes: &[UiNode], text: &str) -> Option<usize> {
    let caret = nodes.iter().find(|node| {
        matches!(node.visual(), UiVisual::Solid { color, .. }
            if *color == ui::oreui_theme::FIELD_CARET)
    })?;
    let label = nodes
        .iter()
        .find(|node| {
            matches!(node.visual(), UiVisual::Text { layout, .. }
            if layout.glyphs().iter().map(|glyph| glyph.codepoint).collect::<String>() == text)
        })
        .expect("the edited text remains visible");
    let UiVisual::Text { layout, .. } = label.visual() else {
        unreachable!()
    };
    let x = node_origin(caret, nodes)[0] - node_origin(label, nodes)[0];
    Some(ui_engine::caret::nearest_boundary(text, x, |prefix| {
        layout.first_line_caret_x_64(prefix.len()) as f32 / 64.0
    }))
}

/// The window-logical rect of each drawn glyph of the text node spelling `wanted`.
fn glyph_rects(nodes: &[UiNode], wanted: &str) -> Vec<(char, [f32; 4])> {
    let node = nodes
        .iter()
        .find(|node| {
            matches!(node.visual(), UiVisual::Text { layout, .. }
            if layout.glyphs().iter().map(|glyph| glyph.codepoint).collect::<String>() == wanted)
        })
        .unwrap_or_else(|| panic!("{wanted:?} in {:?}", drawn_texts(nodes)));
    let origin = node_origin(node, nodes);
    let UiVisual::Text { layout, .. } = node.visual() else {
        unreachable!()
    };
    layout
        .glyphs()
        .iter()
        .map(|glyph| {
            let [left, top, right, bottom] = glyph.bounds_64.map(|edge| edge as f32 / 64.0);
            (
                glyph.codepoint,
                [
                    origin[0] + left,
                    origin[1] + top,
                    origin[0] + right,
                    origin[1] + bottom,
                ],
            )
        })
        .collect()
}

/// Sends one complete physical click at a window-logical point.
fn click(app: &mut App, window: bevy::prelude::Entity, at: [f32; 2]) {
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_cursor_position(Some(Vec2::new(at[0], at[1])));
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    app.update();
}

#[test]
fn a_press_inside_a_box_places_the_caret_at_the_nearest_character() {
    let (mut app, window) = menu_app();
    add_server(&mut app);
    press_key(&mut app, window, KeyCode::KeyA, Some("hello world"));
    // Focus elsewhere so the name draws without a caret.
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::AddAddress);
    let glyphs = glyph_rects(&frame(&mut app, 0), "hello world");
    let (_, w) = glyphs.iter().find(|(glyph, _)| *glyph == 'w').unwrap();
    click(&mut app, window, [w[0] + 1.0, (w[1] + w[3]) / 2.0]);
    let view = app.world().resource::<MenuRuntime>().view();
    assert_eq!(view.field, Some(MenuField::Name));
    assert_eq!(view.caret.byte, "hello ".len(), "before the pressed glyph");
    press_key(&mut app, window, KeyCode::KeyX, Some("X"));
    assert_eq!(
        app.world().resource::<MenuRuntime>().view().name,
        "hello Xworld"
    );

    let (_, h) = glyphs[0];
    let hits =
        client_ui::test_support::menu_hit_targets(app.world().resource::<UiPresentationResource>())
            .to_vec();
    let (_, name_box) = hits
        .iter()
        .find(|(action, _)| *action == MenuAction::AddName)
        .unwrap();
    click(
        &mut app,
        window,
        [name_box.max().x() - 2.0, (h[1] + h[3]) / 2.0],
    );
    assert_eq!(
        app.world().resource::<MenuRuntime>().view().caret.byte,
        "hello Xworld".len(),
        "past the text"
    );
    click(
        &mut app,
        window,
        [name_box.min().x() + 1.0, (h[1] + h[3]) / 2.0],
    );
    assert_eq!(app.world().resource::<MenuRuntime>().view().caret.byte, 0);
}

#[test]
fn a_press_in_the_oreui_world_name_field_places_its_caret() {
    let (mut app, window) = menu_app();
    let mut worlds = client_shell::local_worlds::LocalWorlds::default();
    {
        let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
        menu.activate(MenuAction::Navigate(MenuScreen::Play));
        menu.activate(MenuAction::LocalWorld(LocalWorldAction::BeginCreate));
        menu.sync_local_worlds(&mut worlds, false);
    }
    let name = app
        .world()
        .resource::<MenuRuntime>()
        .view()
        .local
        .create
        .name;
    let glyphs = glyph_rects(&frame(&mut app, 0), &name);
    let (at, (_, glyph)) = glyphs
        .iter()
        .enumerate()
        .find(|(_, (glyph, _))| *glyph == ' ')
        .map(|(at, _)| (at + 1, &glyphs[at + 1]))
        .unwrap();
    click(
        &mut app,
        window,
        [glyph[0] + 1.0, (glyph[1] + glyph[3]) / 2.0],
    );
    let view = app.world().resource::<MenuRuntime>().view();
    assert_eq!(view.field, Some(MenuField::WorldName));
    let expected: usize = name.chars().take(at).map(char::len_utf8).sum();
    assert_eq!(
        view.caret.byte, expected,
        "before the pressed glyph of {name:?}"
    );
}
