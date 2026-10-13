use super::*;
use crate::review_tests::paint;
use launcher::menu::{MenuView, settings_options::SETTINGS_OPTIONS};
use std::{collections::HashMap, sync::Arc};
use ui::UiVisual;

#[test]
fn instant_detection_has_one_accessible_action() {
    let view = MenuView::new(true, "Player".into());
    let (_, hits, _) = paint(HashMap::new(), |canvas| {
        canvas.capture_focus = true;
        let mut content = Content {
            canvas,
            view: &view,
            span: [100.0, 900.0],
            column_width: 800.0,
            y: 100.0,
            translate: &|_| None,
            nested: false,
        };
        draw(&mut content).unwrap();
        assert!(
            content
                .canvas
                .focus_targets
                .iter()
                .any(|target| target.action == MenuAction::SettingsDetectGraphics)
        );
    });
    assert_eq!(
        hits.iter()
            .filter(|(action, _)| *action == MenuAction::SettingsDetectGraphics)
            .count(),
        1
    );
}

#[test]
fn preset_choices_precede_video_controls_and_fit_narrow_and_wide_panels() {
    for width in [300.0, 800.0] {
        let view = MenuView::new(true, "Player".into());
        let (_, hits, _) =
            paint(HashMap::new(), |canvas| {
                canvas.capture_focus = true;
                let mut content = Content {
                    canvas,
                    view: &view,
                    span: [100.0, 100.0 + width],
                    column_width: width,
                    y: 100.0,
                    translate: &|_| None,
                    nested: false,
                };
                super::super::sections::draw(&mut content, "video_forced_index").unwrap();
                for preset in VideoPreset::ALL {
                    assert!(content.canvas.focus_targets.iter().any(|target| {
                        target.action == MenuAction::SettingsVideoPreset(preset)
                    }));
                }
            });
        let first_option = hits
            .iter()
            .find_map(|(action, bounds)| {
                matches!(action, MenuAction::SettingsOption(..)).then_some(*bounds)
            })
            .unwrap();
        let mut previous_right = None;
        for preset in VideoPreset::ALL {
            let action = MenuAction::SettingsVideoPreset(preset);
            let bounds = hits
                .iter()
                .find_map(|(candidate, bounds)| (*candidate == action).then_some(*bounds))
                .unwrap();
            assert!(bounds.min().x() >= 100.0 && bounds.max().x() <= 100.0 + width);
            assert!(bounds.max().y() < first_option.min().y());
            assert_eq!(
                hits.iter()
                    .filter(|(candidate, _)| *candidate == action)
                    .count(),
                1
            );
            if let Some(right) = previous_right {
                assert!(bounds.min().x() <= right && bounds.max().x() > right);
            }
            previous_right = Some(bounds.max().x());
        }
    }
}

#[test]
fn selection_caption_is_derived_and_description_stays_below_the_choices() {
    let mut view = MenuView::new(true, "Player".into());
    for selected in VideoPreset::ALL.into_iter().map(Some).chain([None]) {
        let settings = Arc::make_mut(&mut view.settings_options);
        settings.apply_video_preset(selected.unwrap_or(VideoPreset::Balanced));
        if selected.is_none() {
            let index = SETTINGS_OPTIONS
                .iter()
                .position(|option| option.name == "max_framerate")
                .unwrap();
            settings.set(index, SETTINGS_OPTIONS[index].min);
        }
        let (_, hits, nodes) = paint(HashMap::new(), |canvas| {
            let mut content = Content {
                canvas,
                view: &view,
                span: [100.0, 900.0],
                column_width: 800.0,
                y: 100.0,
                translate: &|_| None,
                nested: false,
            };
            draw(&mut content).unwrap();
        });
        let labels: String = nodes
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
            .collect::<String>()
            .replace(' ', "");
        let caption = format!(
            "Video preset: {}",
            selected.map_or("Custom", VideoPreset::label)
        )
        .replace(' ', "");
        assert!(labels.contains(&caption));
        assert!(labels.contains("IndividualchangesshowCustom."));
        for preset in VideoPreset::ALL {
            assert!(labels.contains(&preset.label().replace(' ', "")));
        }
        let bottom = hits
            .iter()
            .filter(|(action, _)| matches!(action, MenuAction::SettingsVideoPreset(_)))
            .map(|(_, bounds)| bounds.max().y())
            .fold(0.0_f32, f32::max);
        let description = nodes
            .iter()
            .find(|node| match node.visual() {
                UiVisual::Text { layout, .. } => layout
                    .glyphs()
                    .iter()
                    .map(|glyph| glyph.codepoint)
                    .collect::<String>()
                    .replace(' ', "")
                    .starts_with("Chooseagraphicspreset."),
                _ => false,
            })
            .expect("the preset control explains Custom");
        assert!(description.bounds().min().y() >= bottom);
    }
}
