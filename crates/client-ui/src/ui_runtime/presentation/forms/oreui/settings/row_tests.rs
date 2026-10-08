use super::*;
use crate::ui_runtime::presentation::forms::oreui::review_tests::{paint, solids};
use std::collections::HashMap;

#[test]
fn graphics_group_rows_keep_the_same_horizontal_gutters_as_regular_rows() {
    let index = |name: &str| {
        SETTINGS_OPTIONS
            .iter()
            .position(|option| option.name == name)
            .unwrap()
    };
    for graphics in [0, 1] {
        for width in [400.0, 800.0] {
            let mut view = MenuView::new(true, "Player".into());
            std::sync::Arc::make_mut(&mut view.settings_options)
                .set(index("graphics_mode"), graphics);
            let mut tracks = HashMap::new();
            let (_, hits, _) = paint(HashMap::new(), |canvas| {
                let mut content = Content {
                    canvas,
                    view: &view,
                    span: [100.0, 100.0 + width],
                    column_width: width,
                    y: 100.0,
                    translate: &|_| None,
                    nested: false,
                };
                sections::draw(&mut content, "video_forced_index").unwrap();
                assert_eq!(content.span, [100.0, 100.0 + width]);
                assert!(!content.nested);
                tracks.extend(
                    content
                        .canvas
                        .slider_tracks
                        .iter()
                        .map(|(index, track, _)| (*index, *track)),
                );
            });
            let row = |name| {
                let index = index(name);
                let action =
                    MenuAction::SettingsOption(index as u16, 1 - view.settings_options.get(index));
                hits.iter()
                    .find(|(candidate, _)| *candidate == action)
                    .unwrap()
                    .1
            };
            let (nested, regular) = (row("smooth_lighting"), row("render_clouds"));
            assert_eq!(nested.min().x(), regular.min().x());
            assert_eq!(nested.max().x(), regular.max().x());
            let nested = tracks[&(index("gamma") as u16)];
            let regular = tracks[&(index("field_of_view") as u16)];
            assert_eq!(nested.min().x(), regular.min().x());
            assert_eq!(nested.max().x(), regular.max().x());
            assert_eq!(nested.min().x() - 100.0, 100.0 + width - nested.max().x());
        }
    }
}

#[test]
fn every_detail_row_keeps_its_own_edges_before_the_next_heading() {
    let action = MenuAction::SettingsFullscreen(true);
    for (hovered, pressed) in [(false, false), (true, false), (true, true)] {
        let mut view = MenuView::new(true, "Player".into());
        view.hovered = hovered.then_some(action);
        view.pressed = pressed.then_some(action);
        let mut rows = Vec::new();
        let mut edge = 0.0;
        let (_, _, nodes) = paint(HashMap::new(), |canvas| {
            edge = canvas.r(EDGE);
            let mut content = Content {
                canvas,
                view: &view,
                span: [100.0, 900.0],
                column_width: 800.0,
                y: 100.0,
                translate: &|_| None,
                nested: false,
            };
            rows.push(
                content
                    .row("Only allow trusted skins", "", 700.0, Some(action), 0.0)
                    .unwrap(),
            );
            rows.push(
                content
                    .row("Filter profanity", "Description", 700.0, Some(action), 0.0)
                    .unwrap(),
            );
            content.heading("Pause", "Description").unwrap();
        });
        let fills = solids(&nodes);
        for [left, top, right, bottom] in rows {
            assert!(fills.contains(&([left, top, right, bottom], NEUTRAL.fill)));
            assert!(fills.contains(&([left, top, right, top + edge], [255, 255, 255, 26])));
            assert!(fills.contains(&([left, bottom - edge, right, bottom], [0, 0, 0, 77])));
        }
    }
}

#[test]
fn a_setting_without_description_keeps_the_switch_row_minimum_height() {
    let view = MenuView::new(true, "Player".into());
    paint(HashMap::new(), |canvas| {
        let expected = canvas.r(6.4);
        let mut content = Content {
            canvas,
            view: &view,
            span: [100.0, 900.0],
            column_width: 800.0,
            y: 100.0,
            translate: &|_| None,
            nested: false,
        };
        let bounds = content.row("Title", "", 700.0, None, 0.0).unwrap();
        assert!((bounds[3] - bounds[1] - expected).abs() < 0.001);
    });
}

#[test]
fn enabled_boolean_keyboard_focus_outlines_only_the_thumb() {
    let action = MenuAction::SettingsFullscreen(true);
    let mut view = MenuView::new(true, "Player".into());
    view.focused_action = Some(action);
    let mut rem = 0.0;
    let (_, _, nodes) = paint(HashMap::new(), |canvas| {
        rem = canvas.rem;
        let mut content = Content {
            canvas,
            view: &view,
            span: [100.0, 900.0],
            column_width: 800.0,
            y: 100.0,
            translate: &|_| None,
            nested: false,
        };
        content
            .boolean("Title", "Description", false, action, true)
            .unwrap();
    });
    let widths: Vec<_> = solids(&nodes)
        .into_iter()
        .filter_map(|(bounds, color)| (color == theme::OUTLINE).then_some(bounds[2] - bounds[0]))
        .collect();
    assert!(
        widths
            .iter()
            .any(|width| *width > 3.0 * rem && *width < 5.0 * rem),
        "keyboard focus outlines the moving thumb"
    );
    assert!(
        widths.iter().all(|width| *width < 10.0 * rem),
        "the enabled switch's enclosing row has no focus frame"
    );
}

#[test]
fn cloud_option_has_a_readable_title_and_description_without_locale_data() {
    use ui::UiVisual;
    let view = MenuView::new(true, "Player".into());
    paint(HashMap::new(), |canvas| {
        let mut content = Content {
            canvas,
            view: &view,
            span: [20.0, 1000.0],
            column_width: 980.0,
            y: 100.0,
            translate: &|_| None,
            nested: false,
        };
        content.option("render_clouds").unwrap();
        let texts: Vec<_> = content
            .canvas
            .nodes
            .iter()
            .filter_map(|node| match node.visual() {
                UiVisual::Text { layout, .. } => Some(
                    layout
                        .glyphs()
                        .iter()
                        .map(|glyph| glyph.codepoint)
                        .collect::<String>(),
                ),
                _ => None,
            })
            .collect();
        let readable: Vec<_> = texts.iter().map(|text| text.replace(' ', "")).collect();
        assert!(readable.iter().any(|text| text == "RenderClouds"));
        assert!(readable.iter().any(|text| text == "Showcloudsinthesky."));
        assert!(!texts.iter().any(|text| text.starts_with("options.")));
    });
}

#[test]
fn exact_server_ping_setting_has_a_title_description_and_toggle() {
    let view = MenuView::new(true, "Player".into());
    let (_, hits, nodes) = paint(HashMap::new(), |canvas| {
        let mut content = Content {
            canvas,
            view: &view,
            span: [20.0, 1000.0],
            column_width: 980.0,
            y: 100.0,
            translate: &|_| None,
            nested: false,
        };
        content
            .option(crate::menu::settings_options::SHOW_EXACT_SERVER_PING)
            .unwrap();
    });
    let labels: String = nodes
        .iter()
        .filter_map(|node| match node.visual() {
            ui::UiVisual::Text { layout, .. } => Some(
                layout
                    .glyphs()
                    .iter()
                    .map(|glyph| glyph.codepoint)
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect::<String>()
        .replace(' ', "");
    assert!(labels.contains("Showexactserverping"));
    assert!(labels.contains("milliseconds"));
    assert!(!labels.contains("options."));
    let index = crate::menu::settings_options::SETTINGS_OPTIONS
        .iter()
        .position(|d| d.name == crate::menu::settings_options::SHOW_EXACT_SERVER_PING)
        .unwrap();
    assert!(
        hits.iter()
            .any(|(action, _)| *action == MenuAction::SettingsOption(index as u16, 1))
    );
}
