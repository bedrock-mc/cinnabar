use crate::{review_tests::paint, test_support::solids};
use std::collections::HashMap;
use {
    super::*,
    launcher::menu::settings_options::{SETTINGS_OPTIONS, layout::SettingPanel},
    launcher::menu::{MenuAction, MenuView},
};

/// Omits the Advanced heading when the Video section has no advanced rows.
#[test]
fn video_section_omits_an_empty_advanced_heading() {
    let view = MenuView::new(true, "Player".into());
    for width in [400.0, 800.0] {
        let (_, _, nodes) = paint(HashMap::new(), |canvas| {
            let mut content = Content {
                canvas,
                view: &view,
                span: [100.0, 100.0 + width],
                column_width: width,
                y: 100.0,
                translate: &|_| None,
                nested: false,
            };
            sections::draw(&mut content, SettingSection::Video).unwrap();
        });
        let advanced = nodes.iter().any(|node| match node.visual() {
            ui::UiVisual::Text { layout, .. } => layout
                .glyphs()
                .iter()
                .map(|glyph| glyph.codepoint)
                .collect::<String>()
                .eq_ignore_ascii_case("Advanced"),
            _ => false,
        });
        assert_eq!(
            advanced,
            SettingPanel::VideoAdvanced.options().next().is_some()
        );
    }
}

#[test]
fn keyboard_rows_display_observed_layout_labels() {
    use launcher::menu::settings_options::KEY_BINDINGS;
    use semantic_input::{Action, PhysicalControl};
    let mut view = MenuView::new(true, "Player".into());
    let row = KEY_BINDINGS
        .iter()
        .position(|(action, _)| *action == Action::Hotbar1)
        .unwrap();
    let settings = std::sync::Arc::make_mut(&mut view.settings_options);
    assert!(settings.remap(row, PhysicalControl::KeyboardUsage(0x14)));
    settings.set_key_label(0x14, "a");
    let (_, _, nodes) = paint(HashMap::new(), |canvas| {
        let mut content = Content {
            canvas,
            view: &view,
            span: [100.0, 1100.0],
            column_width: 1000.0,
            y: 100.0,
            translate: &|_| None,
            nested: false,
        };
        sections::draw(&mut content, SettingSection::KeyboardAndMouse).unwrap();
    });
    let labels: Vec<_> = nodes
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
        .collect();
    assert!(labels.iter().any(|label| label == "A"));
    assert!(!labels.iter().any(|label| label == "Q"));
    assert_eq!(
        view.settings_options.key_control(row),
        Some(PhysicalControl::KeyboardUsage(0x14))
    );
}

#[test]
fn keyboard_overlay_rows_show_readable_bindings_and_reset_controls() {
    use launcher::menu::settings_options::{
        DEBUG_OVERLAY_KEY, EXTRA_KEYS, KEY_BINDINGS, PLAYER_LIST_KEY,
    };
    use semantic_input::PhysicalControl;
    use std::sync::Arc;

    let row = |name| {
        KEY_BINDINGS
            .iter()
            .map(|(_, name)| *name)
            .chain(EXTRA_KEYS.iter().map(|(name, _)| *name))
            .position(|candidate| candidate == name)
            .unwrap() as u16
    };
    let mut view = MenuView::new(true, "Player".into());
    for remapped in [false, true] {
        if remapped {
            assert!(Arc::make_mut(&mut view.settings_options).remap(
                row(DEBUG_OVERLAY_KEY) as usize,
                PhysicalControl::KeyboardUsage(0x3d)
            ));
            assert!(Arc::make_mut(&mut view.settings_options).remap(
                row(PLAYER_LIST_KEY) as usize,
                PhysicalControl::MouseButton(4)
            ));
        }
        let (_, hits, nodes) = paint(HashMap::new(), |canvas| {
            let mut content = Content {
                canvas,
                view: &view,
                span: [100.0, 1100.0],
                column_width: 1000.0,
                y: 100.0,
                translate: &|_| None,
                nested: false,
            };
            sections::draw(
                &mut content,
                launcher::menu::settings_sections::SettingSection::KeyboardAndMouse,
            )
            .unwrap();
        });
        let labels: Vec<_> = nodes
            .iter()
            .filter_map(|node| match node.visual() {
                ui::UiVisual::Text { layout, .. } => Some(
                    layout
                        .glyphs()
                        .iter()
                        .map(|glyph| glyph.codepoint)
                        .collect::<String>()
                        .replace(' ', ""),
                ),
                _ => None,
            })
            .collect();
        for label in ["DebugOverlay", "PlayerList"] {
            assert!(labels.iter().any(|text| text == label));
        }
        for name in [DEBUG_OVERLAY_KEY, PLAYER_LIST_KEY] {
            assert!(
                hits.iter()
                    .any(|(action, _)| *action == MenuAction::SettingsKey(row(name)))
            );
            assert_eq!(
                hits.iter()
                    .any(|(action, _)| *action == MenuAction::SettingsResetKey(row(name))),
                remapped
            );
        }
        for key in if remapped {
            ["F4", "Mouse4"]
        } else {
            ["F3", "Tab"]
        } {
            assert!(labels.iter().any(|text| text == key));
        }
    }
}

#[test]
fn passive_clients_have_no_update_settings_actions() {
    use launcher::menu::settings_options::{
        AUTO_UPDATE_OPTION, INSTALL_ON_QUIT_OPTION, UPDATE_CHANNEL_OPTION,
    };
    let mut view = MenuView::new(true, "Player".into());
    view.updates.active = false;
    let (_, hits, _) = paint(HashMap::new(), |canvas| {
        let mut content = Content {
            canvas,
            view: &view,
            span: [100.0, 900.0],
            column_width: 800.0,
            y: 100.0,
            translate: &|_| None,
            nested: false,
        };
        sections::draw(
            &mut content,
            launcher::menu::settings_sections::SettingSection::General,
        )
        .unwrap();
    });
    let update_options = [
        AUTO_UPDATE_OPTION,
        INSTALL_ON_QUIT_OPTION,
        UPDATE_CHANNEL_OPTION,
    ];
    assert!(!hits.iter().any(|(action, _)| {
        match action {
            MenuAction::SettingsOption(index, _) => update_options
                .iter()
                .any(|option| SETTINGS_OPTIONS[*index as usize].name == option.name),
            MenuAction::Update(_) => true,
            _ => false,
        }
    }));
    assert!(!hits.is_empty());
}

/// General ends with an Advanced heading holding both diagnostics toggles, off by default,
/// and the capture caption says what a recording holds and that it stays local.
#[test]
fn general_advanced_rows_toggle_diagnostics_and_explain_the_capture() {
    use launcher::menu::settings_options::SettingId;
    let view = MenuView::new(true, "Player".into());
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
        sections::draw(
            &mut content,
            launcher::menu::settings_sections::SettingSection::General,
        )
        .unwrap();
    });
    let text: String = nodes
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
    for expected in [
        "Verboselogging",
        "Recordnetworksession",
        "containschatmessagesandplayernames",
        "neveruploaded",
    ] {
        assert!(text.contains(expected), "missing {expected}");
    }
    // Section headings draw in capitals.
    let heading = text.find("ADVANCED").expect("missing the Advanced heading");
    assert!(
        heading < text.find("Verboselogging").unwrap(),
        "the toggles sit under the Advanced heading"
    );
    for id in [SettingId::VerboseLogging, SettingId::RecordNetworkSession] {
        assert!(
            hits.iter()
                .any(|(action, _)| *action == MenuAction::SettingsOption(id.index() as u16, 1)),
            "{id:?} toggles on from its default"
        );
    }
}

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
                sections::draw(
                    &mut content,
                    launcher::menu::settings_sections::SettingSection::Video,
                )
                .unwrap();
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
        content
            .option(launcher::menu::settings_options::SettingId::RenderClouds)
            .unwrap();
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
            .option(launcher::menu::settings_options::SettingId::ShowExactServerPing)
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
    let index = launcher::menu::settings_options::SETTINGS_OPTIONS
        .iter()
        .position(|d| d.name == launcher::menu::settings_options::SHOW_EXACT_SERVER_PING)
        .unwrap();
    assert!(
        hits.iter()
            .any(|(action, _)| *action == MenuAction::SettingsOption(index as u16, 1))
    );
}

#[test]
fn command_macro_invalid_draft_shows_a_chat_limit_error() {
    let mut view = MenuView::new(true, "Player".into());
    view.macro_commands = Some(core::array::from_fn(|slot| {
        if slot == 0 {
            "x".repeat(ui::MAX_CHAT_INPUT_BYTES)
        } else {
            String::new()
        }
    }));
    let (_, _, nodes) = paint(HashMap::new(), |canvas| {
        let mut content = Content {
            canvas,
            view: &view,
            span: [100.0, 900.0],
            column_width: 800.0,
            y: 100.0,
            translate: &|_| None,
            nested: false,
        };
        sections::draw(
            &mut content,
            launcher::menu::settings_sections::SettingSection::KeyboardAndMouse,
        )
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
    assert!(labels.contains("Commandistoolong."));
    assert!(labels.contains(&format!("{}-bytechatlimit.", ui::MAX_CHAT_INPUT_BYTES)));
}

/// Every registered chat option exposes its real input action, including reset.
#[test]
fn chat_section_draws_all_chat_panel_options_and_reset() {
    let mut view = MenuView::new(true, "Player".into());
    std::sync::Arc::make_mut(&mut view.settings_options).set(SettingId::ChatTypeface.index(), 1);
    for width in [400.0, 800.0] {
        let (_, hits, nodes) = paint(HashMap::new(), |canvas| {
            let mut content = Content {
                canvas,
                view: &view,
                span: [100.0, 100.0 + width],
                column_width: width,
                y: 100.0,
                translate: &|_| None,
                nested: false,
            };
            sections::draw(&mut content, SettingSection::Chat).unwrap();
        });
        for option in SettingPanel::Chat.options() {
            assert!(
                hits.iter().any(|(action, _)| match action {
                    MenuAction::SettingsOption(index, _) | MenuAction::SettingsDropdown(index) =>
                        usize::from(*index) == option.id.index(),
                    _ => false,
                }),
                "missing control for {:?}",
                option.id
            );
        }
        assert!(hits.iter().any(|(action, _)| *action
            == MenuAction::SettingsResetGroup(
                launcher::menu::settings_options::SettingsGroup::Chat
            )));
        let labels = rendered_text(&nodes);
        assert!(labels.contains("ResetChatSettings"));
        assert!(
            labels.contains(
                &assets::carriers::FONT
                    .font_face
                    .unwrap()
                    .name
                    .replace(' ', "")
            )
        );
        assert!(!labels.contains("Mojangles"));
        assert!(labels.contains("100%"));
        assert!(!labels.contains("chat.settings."));
        assert!(!labels.contains("typeface."));
        assert!(!labels.contains("color."));
        assert!(!labels.contains(".description"));
    }
}

/// Collects visible glyphs without depending on the painter's text-node splitting.
fn rendered_text(nodes: &[ui::UiNode]) -> String {
    nodes
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
        .replace(' ', "")
}

/// Missing help text must remain absent; known fallbacks and translations still render.
#[test]
fn option_descriptions_do_not_expose_missing_translation_keys() {
    let view = MenuView::new(true, "Player".into());
    for translated in [false, true] {
        let (_, _, nodes) = paint(HashMap::new(), |canvas| {
            let translate = |key: &str| {
                (translated && key == "options.vsync.description")
                    .then_some("Translated help".into())
            };
            let mut content = Content {
                canvas,
                view: &view,
                span: [100.0, 900.0],
                column_width: 800.0,
                y: 100.0,
                translate: &translate,
                nested: false,
            };
            content.option(SettingId::Vsync).unwrap();
            content.option(SettingId::ChatBackgroundOpacity).unwrap();
        });
        let labels = rendered_text(&nodes);
        assert!(!labels.contains(".description"), "{labels}");
        assert_eq!(labels.contains("Translatedhelp"), translated);
        assert!(labels.contains("Adjusttheopacityofthechatbackgroundbox"));
    }
}

/// The pack's size template substitutes the percentage instead of printing its placeholder.
#[test]
fn chat_font_size_formats_translated_values_and_disables_unsupported_scaling() {
    for (typeface, locale, enabled) in
        [(0, "en_US", false), (1, "en_US", true), (1, "ja_JP", false)]
    {
        for value in [5, 10, 20] {
            let mut view = MenuView::new(true, "Player".into());
            let options = std::sync::Arc::make_mut(&mut view.settings_options);
            options.set(SettingId::ChatTypeface.index(), typeface);
            options.set(SettingId::ChatFontSize.index(), value);
            options.set_language(locale);
            let (_, hits, nodes) = paint(HashMap::new(), |canvas| {
                let translate =
                    |key: &str| (key == "chat.settings.fontSize").then_some("Size: %s".into());
                let mut content = Content {
                    canvas,
                    view: &view,
                    span: [100.0, 900.0],
                    column_width: 800.0,
                    y: 100.0,
                    translate: &translate,
                    nested: false,
                };
                content.option(SettingId::ChatFontSize).unwrap();
            });
            let labels = rendered_text(&nodes);
            assert!(
                labels.contains(&format!("Size:{}%", value * 10)),
                "{labels}"
            );
            assert!(!labels.contains("%s"));
            assert_eq!(labels.matches(&format!("{}%", value * 10)).count(), 1);
            assert_eq!(hits.iter().any(|(action, _)| matches!(action,
                MenuAction::SettingsOption(index, _) if usize::from(*index) == SettingId::ChatFontSize.index())), enabled);
        }
    }
}
