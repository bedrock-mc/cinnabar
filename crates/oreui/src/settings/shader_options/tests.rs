//! The Shader Pack Settings grid stays where the player scrolled it, as Iris keeps it.

use std::{collections::HashMap, sync::Arc};

use launcher::menu::{MenuAction, MenuView};
use launcher::shader_options::{Action, Control, Element, Input, Status, View};
use ui::{DpiScale, SafeArea, TextLayoutCache};
use ui_engine::{
    engine::screen::{PaintFrame, Screen},
    menu_scroll::MenuScrolls,
    test_support::fixture_font,
    text_metrics::TextMetrics,
};

use crate::{
    OreUiScreen,
    menu_screen::{MenuGeometry, MenuInput},
    test_support::settings_view,
};

const SIZE: [f32; 2] = [1280.0, 720.0];

/// Paints the menu as the client does, revealing the focused control through `scrolls`.
fn paint(screen: &mut OreUiScreen, view: &MenuView, scrolls: &mut MenuScrolls<MenuAction>) {
    let font = fixture_font();
    let mut layouts = TextLayoutCache::new(64, 1024 * 1024);
    let (mut nodes, mut next) = (Vec::new(), 1);
    let artwork = HashMap::new();
    let mut frame = PaintFrame {
        layouts: &mut layouts,
        font: &font,
        metrics: TextMetrics::for_viewport(
            SIZE.map(|value| value as u32),
            DpiScale::new(1.0).unwrap(),
            Some(2),
        ),
        solid_page: 0,
        safe_area: SafeArea::ZERO,
        content: SIZE,
        translate: &|_| None,
        language: [0; 3],
        now: 0.0,
        nodes: &mut nodes,
        next: &mut next,
        renderers: &mut [],
    };
    screen
        .paint(
            MenuInput {
                view,
                size: SIZE,
                portrait: None,
                translate: &|_| None,
                artwork: &artwork,
                title_artwork: None,
                scrolls,
                geometry: MenuGeometry::default(),
            },
            &mut frame,
        )
        .unwrap();
    scrolls.set_areas(screen.paint.as_ref().unwrap().scrolls.clone());
}

/// How far the option grid is scrolled.
fn grid_offset(scrolls: &MenuScrolls<MenuAction>) -> f32 {
    scrolls
        .offsets()
        .iter()
        .find_map(|(key, offset)| key.contains("/shader_options/").then_some(*offset))
        .unwrap_or(0.0)
}

#[test]
fn footer_buttons_leave_the_grid_scrolled_where_it_was() {
    let mut view = settings_view("video_forced_index");
    view.shader_options = Some(Arc::new(View {
        pack: "Fixture".into(),
        title: "Lighting".into(),
        screen: 1,
        nested: true,
        columns: 2,
        elements: (0..60)
            .map(|index| Element {
                label: format!("Screen {index}"),
                comment: None,
                control: Control::Link,
                modified: false,
            })
            .collect(),
        changed: true,
        status: Status::Ready,
    }));
    let mut screen = OreUiScreen::default();
    let mut scrolls = MenuScrolls::default();
    paint(&mut screen, &view, &mut scrolls);
    view.focused_action = Some(MenuAction::ShaderOptions(Action::Element {
        screen: 1,
        index: 30,
        input: Input::Next,
    }));
    paint(&mut screen, &view, &mut scrolls);
    let scrolled = grid_offset(&scrolls);
    assert!(scrolled > 0.0, "focusing a lower cell scrolls the grid");
    for action in [
        Action::Apply,
        Action::Reset,
        Action::Cancel,
        Action::Done,
        Action::Back,
    ] {
        view.focused_action = Some(MenuAction::ShaderOptions(action));
        paint(&mut screen, &view, &mut scrolls);
        assert_eq!(grid_offset(&scrolls), scrolled, "{action:?}");
    }
    Arc::make_mut(view.shader_options.as_mut().unwrap()).status = Status::Saving;
    paint(&mut screen, &view, &mut scrolls);
    assert_eq!(
        grid_offset(&scrolls),
        scrolled,
        "saving keeps the grid extent"
    );
    Arc::make_mut(view.shader_options.as_mut().unwrap()).status = Status::Ready;
    paint(&mut screen, &view, &mut scrolls);
    assert_eq!(
        grid_offset(&scrolls),
        scrolled,
        "the saved screen stays scrolled"
    );
}

#[test]
fn hovered_option_help_remains_scrollable_after_the_pointer_leaves_the_option() {
    let mut view = settings_view("video_forced_index");
    let mut options = View::loading("Fixture");
    options.status = Status::Ready;
    options.elements.push(Element {
        label: "Lighting".into(),
        comment: Some("Long option help. ".repeat(100)),
        control: Control::Link,
        modified: false,
    });
    view.shader_options = Some(Arc::new(options));
    view.navigation_focus_visible = false;
    view.hovered = Some(MenuAction::ShaderOptions(Action::Element {
        screen: 0,
        index: 0,
        input: Input::Next,
    }));
    let mut screen = OreUiScreen::default();
    let mut scrolls = MenuScrolls::default();
    paint(&mut screen, &view, &mut scrolls);
    let area = screen
        .take_paint()
        .unwrap()
        .scrolls
        .into_iter()
        .find(|area| area.key.starts_with("shader_option_comment/") && area.max > 0.0)
        .expect("long help needs a scroll viewport");
    let point = crate::test_support::centre(area.viewport);
    scrolls.set_areas(vec![area.clone()]);
    scrolls.wheel(point, -3.0, false);
    let offset = scrolls.offsets()[&area.key];
    assert!(offset > 0.0);
    view.hovered = None;
    paint(&mut screen, &view, &mut scrolls);
    let retained = screen
        .take_paint()
        .unwrap()
        .scrolls
        .into_iter()
        .find(|candidate| candidate.key == area.key)
        .expect("help remains present while it is scrolled");
    assert_eq!(retained.offset, offset);
}

#[test]
fn long_comments_scroll_above_the_footer_buttons() {
    let view = settings_view("video_forced_index");
    let element = Element {
        label: "Lighting".into(),
        comment: Some("Long option help. ".repeat(100)),
        control: Control::Link,
        modified: false,
    };
    let mut options = View::loading("Fixture");
    options.elements = vec![element.clone(), element];
    let draw = |index, canvas: &mut crate::paint::Canvas<'_>| {
        let height = super::footer_height(canvas);
        super::footer(
            canvas,
            &view,
            &options,
            [300.0, SIZE[1] - height, 1200.0, SIZE[1]],
            Some((index, &options.elements[index])),
        )
        .unwrap();
    };
    let (areas, hits, _) = crate::review_tests::paint(HashMap::new(), |canvas| draw(0, canvas));
    let area = areas
        .iter()
        .find(|area| area.max > 0.0)
        .expect("long help needs a scroll viewport");
    let button_top = hits
        .iter()
        .map(|(_, bounds)| bounds.min().y())
        .reduce(f32::min)
        .unwrap();
    assert!(area.viewport.max().y() < button_top);
    let (scrolled, _, _) =
        crate::review_tests::paint(HashMap::from([(area.key.clone(), area.max)]), |canvas| {
            draw(0, canvas)
        });
    let scrolled = scrolled
        .iter()
        .find(|scrolled| scrolled.key == area.key)
        .unwrap();
    assert_eq!(
        scrolled.offset, area.max,
        "the last help lines can be revealed"
    );
    let (other, _, _) =
        crate::review_tests::paint(HashMap::from([(area.key.clone(), area.max)]), |canvas| {
            draw(1, canvas)
        });
    let other = other.iter().find(|other| other.max > 0.0).unwrap();
    assert_eq!(
        other.offset, 0.0,
        "options with equal labels scroll independently"
    );
}
