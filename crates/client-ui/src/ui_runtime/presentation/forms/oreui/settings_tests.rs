//! Settings navigation and editing through the OreUI host without installed carriers.

use super::*;
use crate::menu::settings_options::{SETTINGS_OPTIONS, SettingKind, SettingsGroup};
use crate::ui_runtime::presentation::menu_scroll::{MenuScrolls, ScrollArea};
use crate::ui_runtime::presentation::tests::fixture_font;
use std::collections::HashMap;

struct Frame {
    areas: Vec<ScrollArea>,
    hits: Vec<(MenuAction, UiRect)>,
}

fn paint(view: &MenuView, size: [f32; 2], offsets: HashMap<String, f32>) -> Frame {
    let (areas, hits, _) = super::review_tests::paint(offsets, |canvas| {
        settings::draw(canvas, view, size, &|_| None, None).unwrap();
    });
    Frame { areas, hits }
}

fn panel(frame: &Frame, sidebar: bool) -> &ScrollArea {
    let compare = |left: &&ScrollArea, right: &&ScrollArea| {
        left.viewport.min().x().total_cmp(&right.viewport.min().x())
    };
    let area = if sidebar {
        frame.areas.iter().min_by(compare)
    } else {
        frame.areas.iter().max_by(compare)
    };
    area.expect("settings needs separate sidebar and content scroll views")
}

fn hit(hits: &[(MenuAction, UiRect)], point: UiPoint) -> Option<MenuAction> {
    hits.iter()
        .rev()
        .find_map(|(action, bounds)| bounds.contains(point).then_some(*action))
}

fn settings_view(section: &str) -> MenuView {
    let mut view = MenuView::new(true, "Player".into());
    view.screen = MenuScreen::Settings;
    view.settings_section = super::super::menu_screens::SETTINGS_SECTIONS
        .iter()
        .find_map(|(name, index)| (*name == section).then_some(*index))
        .expect("known settings section");
    view
}

#[test]
fn native_video_settings_exposes_animation_choices_and_crosshair_preferences() {
    use crate::menu::settings_options::{
        ANIMATIONS_OPTION, INVERT_CROSSHAIR_OPTION, THIRD_PERSON_CROSSHAIR_OPTION,
    };

    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = settings_view("video_forced_index");
    let index = |name| {
        SETTINGS_OPTIONS
            .iter()
            .position(|option| option.name == name)
            .unwrap()
    };
    let animation = index(ANIMATIONS_OPTION.name);
    for value in [0, 1] {
        Arc::make_mut(&mut view.settings_options).set(animation, value);
        for option in [THIRD_PERSON_CROSSHAIR_OPTION, INVERT_CROSSHAIR_OPTION] {
            Arc::make_mut(&mut view.settings_options).set(index(option.name), value);
        }
        present(&mut presentation, &view, [1280.0, 720.0]);
        let actions: Vec<_> = presentation.visible_menu_actions().collect();
        for choice in ANIMATIONS_OPTION.min..=ANIMATIONS_OPTION.max {
            assert!(actions.contains(&MenuAction::SettingsOption(animation as u16, choice)));
        }
        for option in [THIRD_PERSON_CROSSHAIR_OPTION, INVERT_CROSSHAIR_OPTION] {
            assert!(
                actions.contains(&MenuAction::SettingsOption(
                    index(option.name) as u16,
                    1 - value
                )),
                "{} must remain reachable in both saved states",
                option.name,
            );
        }
    }
}

#[test]
fn native_video_settings_exposes_smaa_and_supported_msaa_stops_together() {
    use crate::menu::settings_options::SMAA_OPTION;
    let mut view = settings_view("video_forced_index");
    Arc::make_mut(&mut view.settings_options)
        .set_anti_aliasing_support(ui::AntiAliasingSupport::from_counts([1, 4]));
    let index = |name| {
        SETTINGS_OPTIONS
            .iter()
            .position(|option| option.name == name)
            .unwrap() as u16
    };
    let size = [1280.0, 720.0];
    let mut frame = paint(&view, size, HashMap::new());
    let mut scrolls = MenuScrolls::default();
    for _ in 0..64 {
        let viewport = panel(&frame, false).viewport;
        let revealed = [
            MenuAction::SettingsOption(index("msaa"), 1),
            MenuAction::SettingsOption(index("msaa"), 4),
            MenuAction::SettingsOption(index(SMAA_OPTION.name), ui::SmaaMode::Off as i32),
            MenuAction::SettingsOption(index(SMAA_OPTION.name), ui::SmaaMode::Smaa as i32),
        ]
        .into_iter()
        .all(|desired| {
            frame
                .hits
                .iter()
                .any(|(action, bounds)| *action == desired && contains(viewport, *bounds))
        });
        if revealed {
            break;
        }
        scrolls.set_areas(frame.areas.clone());
        assert!(
            scrolls.wheel(centre(viewport), -1.0, false),
            "anti-aliasing controls must be reachable by scrolling the Video pane"
        );
        frame = paint(&view, size, scrolls.offsets().clone());
    }
    let actions: Vec<_> = frame.hits.iter().map(|(action, _)| *action).collect();
    for samples in [1, 4] {
        assert!(actions.contains(&MenuAction::SettingsOption(index("msaa"), samples)));
    }
    for samples in [2, 3, 5, 6, 7, 8] {
        assert!(!actions.contains(&MenuAction::SettingsOption(index("msaa"), samples)));
    }
    for mode in [ui::SmaaMode::Off, ui::SmaaMode::Smaa] {
        assert!(actions.contains(&MenuAction::SettingsOption(
            index(SMAA_OPTION.name),
            mode as i32
        )));
    }
    let viewport = panel(&frame, false).viewport;
    for (action, bounds) in &frame.hits {
        if matches!(action, MenuAction::SettingsOption(option, _) if *option == index("msaa") || *option == index(SMAA_OPTION.name))
        {
            assert!(
                contains(viewport, *bounds),
                "both anti-aliasing controls must be visible in the same Video pane"
            );
        }
    }
}

fn centre(bounds: UiRect) -> UiPoint {
    let (min, max) = (bounds.min(), bounds.max());
    UiPoint::new((min.x() + max.x()) * 0.5, (min.y() + max.y()) * 0.5).unwrap()
}

fn present(
    presentation: &mut UiPresentationRuntime,
    view: &MenuView,
    size: [f32; 2],
) -> Vec<ui::UiNode> {
    let metrics = TextMetrics::for_viewport(
        [size[0] as u32, size[1] as u32],
        ui::DpiScale::new(1.0).unwrap(),
        Some(2),
    );
    let (mut nodes, mut next) = (Vec::new(), 1);
    presentation.menu_hit_targets = presentation
        .append_oreui_screen(view, &mut nodes, &mut next, metrics, size, None, &|_| None)
        .unwrap()
        .expect("settings must draw through the OreUI host");
    nodes
}

#[test]
fn settings_oreui_body_and_heading_choose_separate_runtime_fonts() {
    use crate::ui_runtime::oreui_fonts::OreUiFont;

    let base = fixture_font();
    let font = base
        .with_named_font(OreUiFont::Seven.name(), &base)
        .unwrap()
        .with_named_font(OreUiFont::Ten.name(), &base)
        .unwrap();
    let body_page = font
        .font_named(OreUiFont::Seven.name())
        .glyph('0')
        .unwrap()
        .page;
    let heading_page = font
        .font_named(OreUiFont::Ten.name())
        .glyph('0')
        .unwrap()
        .page;
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let (mut nodes, mut next, mut layouts) = (Vec::new(), 1, ui::TextLayoutCache::new(8, 65536));
    let mut canvas = Canvas::new(&mut nodes, &mut next, &mut layouts, &font, metrics, 0, None);
    for (style, expected_page) in [
        (theme::BODY, body_page),
        (theme::SECTION_HEADER, heading_page),
    ] {
        canvas
            .text("0", [0.0, 0.0], 200.0, style, theme::TEXT, false)
            .unwrap();
        let ui::UiVisual::Text { layout, .. } = canvas.nodes.last().unwrap().visual() else {
            panic!("the label publishes its glyph layout");
        };
        assert!(
            layout
                .glyphs()
                .iter()
                .all(|glyph| glyph.page == expected_page),
            "OreUI semantic text must use its installed font face"
        );
    }
    assert_ne!(body_page, heading_page);
    assert_eq!(
        font.glyph('0'),
        base.glyph('0'),
        "runtime aliases preserve the HUD font"
    );
}

#[test]
fn settings_small_text_selects_native_rasters_by_physical_size() {
    use crate::ui_runtime::oreui_fonts::OreUiFont;
    use assets::{FontLineMetrics, FontRendering};
    use std::collections::BTreeMap;

    let base = fixture_font();
    let face = |pixels: u32, rendering| {
        base.as_ref()
            .clone()
            .with_line_metrics(FontLineMetrics {
                em_64: pixels * 64,
                ascent_64: pixels * 48,
                descent_64: pixels * 8,
            })
            .unwrap()
            .with_rendering(rendering)
    };
    let sdf = face(52, FontRendering::NativeSdf);
    let sizes = BTreeMap::from([
        (7, face(7, FontRendering::NativeCoverage)),
        (8, face(8, FontRendering::NativeCoverage)),
    ]);
    let font = base
        .with_named_font_sizes(OreUiFont::Seven.name(), &sdf, &sizes)
        .unwrap()
        .with_named_font_sizes(OreUiFont::Ten.name(), &sdf, &sizes)
        .unwrap();
    for (gui, dpi, style, rendering, em) in [
        (1, 1.0, theme::CAPTION, FontRendering::NativeCoverage, 7),
        (1, 2.0, theme::CAPTION, FontRendering::NativeCoverage, 7),
        (1, 2.0, theme::BODY, FontRendering::NativeCoverage, 8),
        (
            1,
            2.0,
            theme::SECTION_HEADER,
            FontRendering::NativeCoverage,
            8,
        ),
        (1, 1.0, theme::HEADER5, FontRendering::NativeSdf, 52),
        (2, 2.0, theme::CAPTION, FontRendering::NativeSdf, 52),
    ] {
        let metrics =
            TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(dpi).unwrap(), Some(gui));
        let (mut nodes, mut next, mut layouts) =
            (Vec::new(), 1, ui::TextLayoutCache::new(8, 65536));
        let canvas = Canvas::new(&mut nodes, &mut next, &mut layouts, &font, metrics, 0, None);
        let request = canvas.text_request("0", 200 * 64, style).unwrap();
        assert_eq!(
            request.font.rendering(),
            rendering,
            "GUI {gui}, DPI {dpi}, size {}",
            style.size
        );
        assert_eq!(request.font.line_metrics().unwrap().em_64, em * 64);
        let drawn = request.font.line_metrics().unwrap().em_64 as f32 / 64.0 * request.scale.get();
        assert!(
            (drawn - canvas.r(style.size)).abs() < 0.001,
            "native selection preserves the CSS font size"
        );
    }
}

#[test]
fn settings_native_font_keeps_css_line_boxes_and_pointer_caret_in_sync() {
    use crate::menu::MenuField;
    use crate::ui_runtime::oreui_fonts::OreUiFont;

    let base = fixture_font();
    let native = base
        .as_ref()
        .clone()
        .with_line_metrics(assets::FontLineMetrics {
            em_64: 32 * 64,
            ascent_64: 26 * 64,
            descent_64: 3 * 64,
        })
        .unwrap();
    let font = base
        .with_named_font(OreUiFont::Seven.name(), &native)
        .unwrap();
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let (mut nodes, mut next, mut layouts) = (Vec::new(), 1, ui::TextLayoutCache::new(16, 65536));
    let mut canvas = Canvas::new(&mut nodes, &mut next, &mut layouts, &font, metrics, 0, None);
    assert_eq!(
        canvas.measure_height("0\n0", 200.0, theme::BODY).unwrap(),
        canvas.r(theme::BODY.line) * 2.0
    );
    canvas
        .text_centred(
            "00",
            [0.0, 0.0, 100.0, 40.0],
            theme::BODY,
            theme::TEXT,
            false,
        )
        .unwrap();
    let label = canvas.nodes.last().unwrap();
    assert_eq!(
        (label.bounds().min().y() + label.bounds().max().y()) * 0.5,
        20.0
    );
    let ui::UiVisual::Text { layout, .. } = label.visual() else {
        panic!("centered label publishes glyphs");
    };
    assert_eq!(
        layout.key().wrap.letter_spacing_64,
        (canvas.r(theme::LETTER_SPACING) * 64.0).round() as i32
    );
    let view = settings_view("video_forced_index");
    widgets::text_field(
        &mut canvas,
        &view,
        [0.0, 0.0, 200.0, 50.0],
        "0000",
        "",
        false,
        Some(MenuAction::AddName),
    )
    .unwrap();
    let point = UiPoint::new(
        canvas.spots[0].left + canvas.measure("00", theme::BODY).unwrap(),
        25.0,
    )
    .unwrap();
    let spots = std::mem::take(&mut canvas.spots);
    let mut presentation = UiPresentationRuntime::new(std::sync::Arc::new(font)).unwrap();
    presentation.add_menu_text_spots(spots);
    assert_eq!(
        presentation.menu_caret_at(point, MenuField::Name, "0000"),
        Some(2),
        "pointer caret uses the same native face, scale and spacing as the label"
    );
}

#[test]
fn settings_route_draws_oreui_and_publishes_navigation_hits() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let view = settings_view("accessibility_forced_index");
    let nodes = present(&mut presentation, &view, [1280.0, 720.0]);
    assert!(!nodes.is_empty(), "settings must publish a visible screen");
    for action in [
        MenuAction::AddBack,
        MenuAction::SettingsSection(view.settings_section),
    ] {
        let target = presentation
            .menu_hit_targets
            .iter()
            .find_map(|(found, bounds)| (*found == action).then_some(*bounds))
            .expect("settings navigation must remain interactive");
        assert_eq!(presentation.hit_test_menu(centre(target)), Some(action));
    }
}

#[test]
fn settings_header_reads_the_active_pack_language() {
    let view = settings_view("accessibility_forced_index");
    let title = "EINSTELLUNGEN";
    let translate =
        |key: &str| (key == "menu.settings.caps").then(|| std::sync::Arc::<str>::from(title));
    let (_, _, nodes) = super::review_tests::paint(HashMap::new(), |canvas| {
        settings::draw(canvas, &view, [1280.0, 720.0], &translate, None).unwrap();
    });
    assert!(
        super::super::pack_harness::drawn_texts(&nodes)
            .iter()
            .any(|text| text == title),
        "the native title uses the installed language when the OreUI string is absent"
    );
}

#[test]
fn settings_focus_includes_offscreen_edits_and_reveals_the_selected_control() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = settings_view("accessibility_forced_index");
    let size = [1280.0, 720.0];
    present(&mut presentation, &view, size);
    let action = MenuAction::SettingsResetGroup(SettingsGroup::Accessibility);
    assert!(
        presentation
            .menu_hit_targets
            .iter()
            .all(|(target, _)| *target != action),
        "the regression needs a control below the viewport"
    );
    assert!(
        presentation
            .visible_menu_actions()
            .any(|target| target == action),
        "keyboard/controller navigation must include controls below the viewport"
    );
    view.focused_action = Some(action);
    present(&mut presentation, &view, size);
    let bounds = presentation
        .menu_hit_targets
        .iter()
        .find_map(|(target, bounds)| (*target == action).then_some(*bounds))
        .expect("focusing an offscreen control must reveal it on that frame");
    assert_eq!(presentation.hit_test_menu(centre(bounds)), Some(action));
}

#[test]
fn settings_focus_returns_from_header_and_reveals_the_same_body_control() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = settings_view("accessibility_forced_index");
    let size = [1280.0, 720.0];
    let action = MenuAction::SettingsResetGroup(SettingsGroup::Accessibility);
    view.focused_action = Some(action);
    present(&mut presentation, &view, size);
    let bounds = presentation
        .menu_hit_targets
        .iter()
        .find_map(|(target, bounds)| (*target == action).then_some(*bounds))
        .expect("focusing the body control must reveal it");
    view.focused_action = Some(MenuAction::AddBack);
    present(&mut presentation, &view, size);
    assert!(presentation.scroll_menu(centre(bounds), 1000.0, false));
    presentation.menu_seconds += 0.2;
    present(&mut presentation, &view, size);
    assert!(
        presentation
            .menu_hit_targets
            .iter()
            .all(|(target, _)| *target != action)
    );
    view.focused_action = Some(action);
    present(&mut presentation, &view, size);
    let bounds = presentation
        .menu_hit_targets
        .iter()
        .find_map(|(target, bounds)| (*target == action).then_some(*bounds))
        .expect("returning from the header must reveal the previous body control");
    assert_eq!(presentation.hit_test_menu(centre(bounds)), Some(action));
}

#[test]
fn settings_header_focus_preserves_a_manually_scrolled_narrow_sidebar() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = settings_view("accessibility_forced_index");
    let size = [640.0, 360.0];
    present(&mut presentation, &view, size);
    let bounds = presentation
        .menu_hit_targets
        .iter()
        .find_map(|(action, bounds)| {
            matches!(action, MenuAction::SettingsSection(_)).then_some(*bounds)
        })
        .expect("the sidebar must accept scrolling");
    assert!(presentation.scroll_menu(centre(bounds), -1000.0, false));
    presentation.menu_seconds += 0.2;
    present(&mut presentation, &view, size);
    let offsets = presentation.menu_scrolls.offsets().clone();
    assert!(offsets.values().any(|offset| *offset > 0.0));
    view.focused_action = Some(MenuAction::AddBack);
    present(&mut presentation, &view, size);
    assert_eq!(presentation.menu_scrolls.offsets(), &offsets);
    view.focused_action = Some(MenuAction::SettingsSection(view.settings_section));
    present(&mut presentation, &view, size);
    assert!(
        presentation
            .menu_hit_targets
            .iter()
            .any(|(action, _)| { *action == MenuAction::SettingsSection(view.settings_section) }),
        "returning from the header must reveal the first category"
    );
}

#[test]
fn settings_picker_close_focus_preserves_the_scrolled_choice_list() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = settings_view("creator_forced_index");
    let index = option_index("content_log_gui_level");
    let option = &SETTINGS_OPTIONS[index];
    view.settings_dropdown = Some(index as u16);
    view.focused_action = Some(MenuAction::SettingsOption(index as u16, option.max));
    let size = [640.0, 120.0];
    present(&mut presentation, &view, size);
    let offsets = presentation.menu_scrolls.offsets().clone();
    assert!(
        offsets.values().any(|offset| *offset > 0.0),
        "the regression needs an overflowing picker"
    );
    view.focused_action = Some(MenuAction::SettingsDropdown(index as u16));
    present(&mut presentation, &view, size);
    assert_eq!(presentation.menu_scrolls.offsets(), &offsets);
}

fn window_bounds(nodes: &[ui::UiNode], node: &ui::UiNode) -> UiRect {
    let mut origin = [0.0; 2];
    let mut parent = node.parent();
    while let Some(ancestor) = parent.and_then(|id| nodes.iter().find(|node| node.id() == id)) {
        let min = ancestor.bounds().min();
        origin[0] += min.x();
        origin[1] += min.y();
        parent = ancestor.parent();
    }
    let bounds = node.bounds();
    super::super::super::rect(
        bounds.min().x() + origin[0],
        bounds.min().y() + origin[1],
        bounds.max().x() + origin[0],
        bounds.max().y() + origin[1],
    )
    .unwrap()
}

#[test]
fn settings_wrapped_inline_choice_labels_fit_their_selectable_controls() {
    let view = settings_view("video_forced_index");
    let index = option_index("third_person");
    let SettingKind::Dropdown(choices) = SETTINGS_OPTIONS[index].kind else {
        panic!("camera perspective choices");
    };
    let labels = [
        "First Person Camera Perspective",
        "Third Person Camera Perspective Back",
        "Third Person Camera Perspective Front",
    ];
    let translate = |key: &str| {
        choices
            .iter()
            .position(|choice| choice.label == key)
            .map(|choice| std::sync::Arc::<str>::from(labels[choice]))
    };
    let (_, hits, nodes) = super::review_tests::paint(HashMap::new(), |canvas| {
        settings::draw(canvas, &view, [1280.0, 720.0], &translate, None).unwrap();
    });
    let mut wrapped = false;
    for choice in 0..choices.len() {
        let action = MenuAction::SettingsOption(index as u16, choice as i32);
        let bounds = hits
            .iter()
            .find_map(|(found, bounds)| (*found == action).then_some(*bounds))
            .expect("each inline camera perspective must remain selectable");
        let label = nodes
            .iter()
            .find_map(|node| {
                let ui::UiVisual::Text { layout, .. } = node.visual() else {
                    return None;
                };
                let text = window_bounds(&nodes, node);
                (text.min().x() >= bounds.min().x()
                    && text.max().x() <= bounds.max().x()
                    && text.min().y() >= bounds.min().y()
                    && text.min().y() < bounds.max().y())
                .then_some((text, layout))
            })
            .expect("each selectable choice needs a label");
        assert!(
            contains(bounds, label.0),
            "choice {choice} text must fit its button"
        );
        wrapped |= label.1.line_count() > 1;
    }
    assert!(wrapped, "the regression needs a label that wraps");
}

#[test]
fn settings_long_choice_picker_owns_pointer_input_and_scroll_views() {
    let mut view = settings_view("video_forced_index");
    let index = option_index("graphics_mode");
    let SettingKind::Dropdown(choices) = SETTINGS_OPTIONS[index].kind else {
        panic!("graphics mode choices");
    };
    view.settings_dropdown = Some(index as u16);
    let translate = |key: &str| {
        choices.iter().any(|choice| choice.label == key).then(|| {
            std::sync::Arc::<str>::from(
                "A graphics mode label long enough to require the native option picker",
            )
        })
    };
    let (areas, hits, _) = super::review_tests::paint(HashMap::new(), |canvas| {
        settings::draw(canvas, &view, [1280.0, 720.0], &translate, None).unwrap();
    });
    assert_eq!(
        areas.len(),
        1,
        "the picker must own wheel and scrollbar input"
    );
    assert!(
        hits.iter().all(|(action, _)| match action {
            MenuAction::SettingsOption(at, _) | MenuAction::SettingsDropdown(at) =>
                usize::from(*at) == index,
            _ => false,
        }),
        "the picker must exclude background navigation and editing actions"
    );
    let close = MenuAction::SettingsDropdown(index as u16);
    assert_eq!(hit(&hits, UiPoint::new(1.0, 1.0).unwrap()), Some(close));
    for choice in 0..choices.len() {
        let action = MenuAction::SettingsOption(index as u16, choice as i32);
        let bounds = hits
            .iter()
            .find_map(|(found, bounds)| (*found == action).then_some(*bounds))
            .expect("each picker choice must commit its own value");
        assert_eq!(hit(&hits, centre(bounds)), Some(action));
    }
}

#[test]
fn settings_slider_capture_clamps_outside_the_bar_on_both_axes() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let view = settings_view("video_forced_index");
    present(&mut presentation, &view, [1280.0, 720.0]);
    let index = option_index("field_of_view");
    let option = &SETTINGS_OPTIONS[index];
    for (x, y, expected) in [(-1000.0, -1000.0, option.min), (4000.0, 4000.0, option.max)] {
        assert_eq!(
            presentation.settings_slider_drag_action(index as u16, UiPoint::new(x, y).unwrap()),
            Some(MenuAction::SettingsOption(index as u16, expected)),
        );
    }
    assert_eq!(
        presentation.settings_slider_drag_action(
            option_index("main_volume") as u16,
            UiPoint::new(600.0, 600.0).unwrap()
        ),
        None,
        "a slider in another section cannot receive the capture",
    );
}

#[test]
fn settings_scale_choices_do_not_publish_slider_capture_geometry() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = settings_view("accessibility_forced_index");
    view.gui_scale_choices = ui::DesktopGuiScale::for_window([1280, 720])
        .choices()
        .collect();
    view.focused_action = Some(MenuAction::SettingsScale(-1));
    present(&mut presentation, &view, [1280.0, 720.0]);
    assert!(
        presentation
            .menu_hit_targets
            .iter()
            .any(|(action, _)| *action == MenuAction::SettingsScale(-1))
    );
    for point in [
        UiPoint::new(-1000.0, -1000.0).unwrap(),
        UiPoint::new(4000.0, 4000.0).unwrap(),
    ] {
        assert_eq!(
            presentation.gui_scale_drag_action(point),
            None,
            "native option buttons never acquire slider capture"
        );
    }
    assert_eq!(presentation.gui_scale_slider_track(), None);
}

#[test]
fn settings_scale_picker_owns_input_and_keeps_each_modifier_action() {
    let mut view = settings_view("video_forced_index");
    view.gui_scale_choices = ui::DesktopGuiScale::for_window([1920, 1080])
        .choices()
        .collect();
    view.settings_scale_picker = true;
    let frame = paint(&view, [640.0, 720.0], HashMap::new());
    assert_eq!(frame.areas.len(), 1);
    assert!(frame.hits.iter().all(|(action, _)| matches!(
        action,
        MenuAction::SettingsScale(_) | MenuAction::SettingsScalePicker
    )));
    for choice in &view.gui_scale_choices {
        let action = MenuAction::SettingsScale(choice.offset);
        let bounds = frame
            .hits
            .iter()
            .find_map(|(candidate, bounds)| (*candidate == action).then_some(*bounds))
            .expect("each GUI modifier is independently selectable");
        assert_eq!(hit(&frame.hits, centre(bounds)), Some(action));
    }
}

#[test]
fn settings_sidebar_and_content_take_scroll_input_independently() {
    let view = settings_view("accessibility_forced_index");
    let size = [1280.0, 360.0];
    let initial = paint(&view, size, HashMap::new());
    let (side, content) = (panel(&initial, true), panel(&initial, false));
    assert_ne!(side.key, content.key);
    assert!(side.max > 0.0 && content.max > 0.0);
    let (side_key, content_key) = (side.key.clone(), content.key.clone());
    let mut scrolls = MenuScrolls::default();
    scrolls.set_areas(initial.areas.clone());
    assert!(scrolls.wheel(centre(side.viewport), -1000.0, false));
    assert_eq!(scrolls.offsets()[&side_key], side.max);
    assert_eq!(scrolls.offsets().get(&content_key), None);
    let moved_side = paint(&view, size, scrolls.offsets().clone());
    let controls = |frame: &Frame| {
        frame
            .hits
            .iter()
            .copied()
            .filter(|(action, _)| !matches!(action, MenuAction::SettingsSection(_)))
            .collect::<Vec<_>>()
    };
    assert_eq!(controls(&initial), controls(&moved_side));
    assert_ne!(initial.hits, moved_side.hits);
    scrolls.set_areas(moved_side.areas.clone());
    assert!(scrolls.wheel(centre(content.viewport), -1000.0, false));
    assert_eq!(scrolls.offsets()[&side_key], side.max);
    assert_eq!(scrolls.offsets()[&content_key], content.max);
    let moved_both = paint(&view, size, scrolls.offsets().clone());
    let categories = |frame: &Frame| {
        frame
            .hits
            .iter()
            .copied()
            .filter(|(action, _)| matches!(action, MenuAction::SettingsSection(_)))
            .collect::<Vec<_>>()
    };
    assert_eq!(categories(&moved_side), categories(&moved_both));
    assert_ne!(controls(&moved_side), controls(&moved_both));
}

fn contains(outer: UiRect, inner: UiRect) -> bool {
    let (min, max) = (inner.min(), inner.max());
    min.x() >= outer.min().x()
        && min.y() >= outer.min().y()
        && max.x() <= outer.max().x()
        && max.y() <= outer.max().y()
}

#[test]
fn settings_scrolling_never_publishes_offscreen_or_cross_panel_hits() {
    let view = settings_view("accessibility_forced_index");
    for size in [[640.0, 360.0], [1280.0, 720.0], [2560.0, 1440.0]] {
        let initial = paint(&view, size, HashMap::new());
        let offsets = initial
            .areas
            .iter()
            .map(|area| (area.key.clone(), area.max))
            .collect();
        for frame in [initial, paint(&view, size, offsets)] {
            let window = super::super::super::rect(0.0, 0.0, size[0], size[1]).unwrap();
            let (side, content) = (panel(&frame, true), panel(&frame, false));
            assert!(contains(window, side.viewport));
            assert!(contains(window, content.viewport));
            assert!(side.viewport.max().x() <= content.viewport.min().x());
            for (action, bounds) in &frame.hits {
                assert!(contains(window, *bounds), "{size:?}: {action:?}");
                match action {
                    MenuAction::SettingsSection(_) => assert!(contains(side.viewport, *bounds)),
                    MenuAction::AddBack => {}
                    _ => assert!(contains(content.viewport, *bounds), "{size:?}: {action:?}"),
                }
                assert_eq!(hit(&frame.hits, centre(*bounds)), Some(*action));
            }
        }
    }
}

#[test]
fn settings_resize_clamps_scroll_before_drawing_the_last_rows() {
    let view = settings_view("accessibility_forced_index");
    let short = paint(&view, [1280.0, 360.0], HashMap::new());
    let offsets = short
        .areas
        .iter()
        .map(|area| (area.key.clone(), area.max))
        .collect();
    let size = [1280.0, 720.0];
    let tall = paint(&view, size, HashMap::new());
    let clamped = tall
        .areas
        .iter()
        .map(|area| (area.key.clone(), area.max))
        .collect();
    let resized = paint(&view, size, offsets);
    let expected = paint(&view, size, clamped);
    assert_eq!(resized.areas, expected.areas);
    assert_eq!(resized.hits, expected.hits);
}

fn option_index(name: &str) -> usize {
    SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == name)
        .expect("registered option")
}

fn centered_option(view: &MenuView, index: usize, size: [f32; 2]) -> HashMap<String, f32> {
    let initial = paint(view, size, HashMap::new());
    let area = panel(&initial, false);
    let steps = (area.max / (area.viewport.height() * 0.5)).ceil() as usize;
    for step in 0..=steps {
        let offset = (step as f32 * area.viewport.height() * 0.5).min(area.max);
        let frame = paint(view, size, HashMap::from([(area.key.clone(), offset)]));
        if let Some(bounds) = frame.hits.iter().find_map(|(action, bounds)| {
            matches!(action, MenuAction::SettingsOption(at, _) | MenuAction::SettingsDropdown(at) if usize::from(*at) == index)
                .then_some(*bounds)
        }) {
            let offset = (offset + centre(bounds).y() - centre(area.viewport).y())
                .clamp(0.0, area.max);
            return HashMap::from([(area.key.clone(), offset)]);
        }
    }
    panic!(
        "{} must have an interactive row",
        SETTINGS_OPTIONS[index].name
    );
}

#[test]
fn settings_choice_rows_select_their_own_values() {
    let size = [1280.0, 720.0];
    for (section, name) in [
        ("video_forced_index", "graphics_mode"),
        ("video_forced_index", "third_person"),
        ("accessibility_forced_index", "toast_notification_duration"),
        ("accessibility_forced_index", "chat_message_duration"),
        ("creator_forced_index", "content_log_gui_level"),
    ] {
        let mut view = settings_view(section);
        let index = option_index(name);
        let SettingKind::Dropdown(choices) = SETTINGS_OPTIONS[index].kind else {
            panic!("dropdown option");
        };
        let offsets = centered_option(&view, index, size);
        view.settings_dropdown = Some(index as u16);
        let frame = paint(&view, size, offsets);
        for choice in 0..choices.len() {
            let action = MenuAction::SettingsOption(index as u16, choice as i32);
            let bounds = frame
                .hits
                .iter()
                .find_map(|(found, bounds)| (*found == action).then_some(*bounds))
                .unwrap_or_else(|| panic!("{name} choice {choice} must be visible"));
            assert_eq!(hit(&frame.hits, centre(bounds)), Some(action), "{name}");
        }
    }
}

#[test]
fn settings_sliders_reach_both_limits_and_choose_the_nearest_stop() {
    let size = [1280.0, 720.0];
    for (section, name) in [
        ("video_forced_index", "render_distance"),
        ("video_forced_index", "gamma"),
        ("video_forced_index", "field_of_view"),
        (
            "keyboard_and_mouse_forced_index",
            "keyboard_mouse_sensitivity",
        ),
        (
            "controller_and_switch_forced_index",
            "controller_sensitivity",
        ),
        ("sound_forced_index", "main_volume"),
        ("accessibility_forced_index", "texttospeech_volume"),
    ] {
        let mut view = settings_view(section);
        view.settings_advanced_graphics = section == "video_forced_index";
        let index = option_index(name);
        let option = &SETTINGS_OPTIONS[index];
        assert!(matches!(option.kind, SettingKind::Slider));
        let frame = paint(&view, size, centered_option(&view, index, size));
        let mut stops = frame
            .hits
            .iter()
            .filter_map(|(action, bounds)| match action {
                MenuAction::SettingsOption(at, value) if usize::from(*at) == index => {
                    Some((*value, *bounds))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        stops.sort_by_key(|(value, _)| *value);
        let expected = (option.min..=option.max)
            .step_by(option.step as usize)
            .collect::<Vec<_>>();
        assert_eq!(
            stops.iter().map(|(value, _)| *value).collect::<Vec<_>>(),
            expected,
            "{name}"
        );
        for (value, bounds) in &stops {
            assert_eq!(
                hit(&frame.hits, centre(*bounds)),
                Some(MenuAction::SettingsOption(index as u16, *value)),
                "{name} value {value}"
            );
        }
        let first = stops.first().unwrap();
        let interval = centre(stops[2].1).x() - centre(stops[1].1).x();
        let left = centre(stops[1].1).x() - interval;
        let right = centre(stops[stops.len() - 2].1).x() + interval;
        let y = centre(first.1).y();
        for (x, value) in [
            (left + interval * 0.25, option.min),
            (left + interval * 0.75, option.min + option.step),
            (right - interval * 0.75, option.max - option.step),
            (right - interval * 0.25, option.max),
        ] {
            assert_eq!(
                hit(&frame.hits, UiPoint::new(x, y).unwrap()),
                Some(MenuAction::SettingsOption(index as u16, value)),
                "{name} near a track endpoint"
            );
        }
    }
}

#[test]
fn settings_language_scroll_reveals_the_last_language() {
    let mut view = settings_view("language_forced_index");
    view.language_choices = (0..40)
        .map(|index| (format!("language_{index}"), format!("Language {index}")))
        .collect::<Vec<_>>()
        .into();
    let size = [1280.0, 720.0];
    let initial = paint(&view, size, HashMap::new());
    let content = panel(&initial, false);
    assert!(content.max > 0.0);
    let frame = paint(
        &view,
        size,
        HashMap::from([(content.key.clone(), content.max)]),
    );
    let action = MenuAction::SettingsLanguage((view.language_choices.len() - 1) as u16);
    let bounds = frame
        .hits
        .iter()
        .find_map(|(found, bounds)| (*found == action).then_some(*bounds))
        .expect("scrolling must reveal the last language");
    assert_eq!(hit(&frame.hits, centre(bounds)), Some(action));
}
