//! The launcher's text boxes through the engine's Add Server screen: the caret
//! drawn where typing goes, its blink, and a press placing it by character.
//! Needs the gitignored UI carrier; each test skips when it is absent.

use bevy::prelude::{App, ButtonInput, KeyCode, MouseButton, Vec2, Window};
use ui::{DpiScale, UiNode, UiVisual};

use super::super::forms::pack_harness::{drawn_texts, engine_presentation};
use super::super::{TextMetrics, UiPresentationRuntime};
use crate::{
    menu::{LocalWorldAction, MenuAction, MenuClipboard, MenuField, MenuRuntime, MenuScreen},
    ui_runtime::{
        UiRuntime,
        tests::menu_input_tests::{menu_input_app_with, press_key},
    },
};

const SIZE: [u32; 2] = [1280, 720];

/// Builds the menu at `now_millis` (which also sets its hit targets) and returns its nodes.
fn frame(app: &mut App, now_millis: u64) -> Vec<UiNode> {
    let view = app.world().resource::<MenuRuntime>().view();
    let mut presentation = app.world_mut().resource_mut::<UiPresentationRuntime>();
    presentation.set_menu_view(Some(view));
    let runtime = UiRuntime::new(1);
    let dpi = DpiScale::new(1.0).unwrap();
    presentation.build(&runtime, now_millis, SIZE, dpi).unwrap();
    let metrics = TextMetrics::for_viewport(SIZE, dpi, None);
    let (mut nodes, mut next) = (Vec::new(), 1);
    presentation
        .append_menu(
            &runtime,
            &mut nodes,
            &mut next,
            metrics,
            SIZE[0] as f32,
            SIZE[1] as f32,
        )
        .unwrap();
    nodes
}

fn add_server(app: &mut App) {
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::PlayAddServer);
}

#[test]
#[ignore = "requires installed local carriers (make assets)"]
fn the_focused_box_draws_the_caret_where_typing_goes_and_blinks() {
    let presentation =
        engine_presentation().expect("required offline fixture; see the ignore reason");
    let (mut app, window) = menu_input_app_with(MenuClipboard::default(), presentation);
    add_server(&mut app);
    press_key(&mut app, window, KeyCode::KeyA, Some("abc"));
    press_key(&mut app, window, KeyCode::ArrowLeft, None);
    let caret = json_ui::CARET_GLYPH;
    let blink = (json_ui::CARET_BLINK_SECONDS * 1_000.0) as u64;
    let start = 10_000;
    let shown = drawn_texts(&frame(&mut app, start));
    assert!(shown.contains(&format!("ab{caret}c")), "{shown:?}");
    let hidden = drawn_texts(&frame(&mut app, start + blink + blink / 2));
    assert!(hidden.contains(&"abc".to_owned()), "blinks off: {hidden:?}");
    press_key(&mut app, window, KeyCode::ArrowRight, None);
    let moved = drawn_texts(&frame(&mut app, start + blink + blink / 2 + 1));
    assert!(
        moved.contains(&format!("abc{caret}")),
        "a move shows the caret at once: {moved:?}"
    );
    press_key(&mut app, window, KeyCode::ShiftLeft, None);
    press_key(&mut app, window, KeyCode::ArrowLeft, None);
    let selected = frame(&mut app, start + blink + blink / 2 + 2);
    assert_eq!(
        app.world().resource::<MenuRuntime>().view().caret.selection,
        Some([2, 3])
    );
    assert!(
        !drawn_texts(&selected)
            .iter()
            .any(|text| text.contains(caret))
    );
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

/// The window-logical rect of each drawn glyph of the text node spelling `wanted`.
fn glyph_rects(nodes: &[UiNode], wanted: &str) -> Vec<(char, [f32; 4])> {
    let node = nodes
        .iter()
        .find(|node| {
            matches!(node.visual(), UiVisual::Text { layout, .. }
            if layout.glyphs().iter().map(|glyph| glyph.codepoint).collect::<String>() == wanted)
        })
        .unwrap_or_else(|| panic!("{wanted:?} in {:?}", drawn_texts(nodes)));
    let clip = nodes
        .iter()
        .find(|other| Some(other.id()) == node.parent())
        .map_or([0.0; 2], |clip| {
            [clip.bounds().min().x(), clip.bounds().min().y()]
        });
    let origin = [
        clip[0] + node.bounds().min().x(),
        clip[1] + node.bounds().min().y(),
    ];
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
#[ignore = "requires installed local carriers (make assets)"]
fn a_press_inside_a_box_places_the_caret_at_the_nearest_character() {
    let presentation =
        engine_presentation().expect("required offline fixture; see the ignore reason");
    let (mut app, window) = menu_input_app_with(MenuClipboard::default(), presentation);
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
    let hits = app
        .world()
        .resource::<UiPresentationRuntime>()
        .menu_hit_targets
        .clone();
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
#[ignore = "requires installed local carriers (make assets)"]
fn a_press_in_the_oreui_world_name_field_places_its_caret() {
    let presentation =
        engine_presentation().expect("required offline fixture; see the ignore reason");
    let (mut app, window) = menu_input_app_with(MenuClipboard::default(), presentation);
    let mut worlds = crate::local_worlds::LocalWorlds::default();
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
