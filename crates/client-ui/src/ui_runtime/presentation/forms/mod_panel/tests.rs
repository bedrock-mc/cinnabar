use super::super::{snapshot, tests::mini_engine_presentation};
use super::*;
use render_model::UiRenderInput;
use ui::DpiScale;

fn panel() -> Panel {
    Panel {
        style: Default::default(),
        title: "Personal controls".into(),
        toggle_key: "ShiftRight".into(),
        dark: true,
        controls: vec![
            Control::Toggle {
                id: "enabled".into(),
                label: "Enabled".into(),
                value: false,
            },
            Control::Slider {
                id: "strength".into(),
                label: "Strength".into(),
                value: 35.0,
                min: 0.0,
                max: 100.0,
                step: 1.0,
            },
            Control::Choice {
                id: "mode".into(),
                label: "Activation".into(),
                index: 0,
                options: vec!["Continuous".into(), "While clicking".into()],
            },
            Control::Button {
                id: "binding".into(),
                label: "Toggle key".into(),
            },
        ],
        sections: Vec::new(),
        capture_key: false,
    }
}

fn frame(presentation: &mut UiPresentationRuntime, size: [u32; 2]) -> UiRenderInput {
    presentation
        .build(
            &player_state::PlayerState::new(1),
            &UiRuntime::new(1),
            0,
            size,
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap()
}

fn point(presentation: &UiPresentationRuntime, action: &str, fraction: f64) -> [f32; 2] {
    let frame = presentation
        .form_presentation
        .mod_panel
        .as_ref()
        .unwrap()
        .frame
        .as_ref()
        .unwrap();
    let region = frame
        .hits
        .iter()
        .find(|hit| hit.pressed.as_deref() == Some(action))
        .unwrap_or_else(|| panic!("missing rendered action {action}; hits={:?}", frame.hits));
    [
        frame.origin[0] + (region.rect.x + region.rect.w * fraction) as f32 * frame.scale,
        frame.origin[1] + (region.rect.y + region.rect.h * 0.5) as f32 * frame.scale,
    ]
}

#[test]
fn absent_or_closed_panel_does_not_change_the_frame() {
    let mut presentation = mini_engine_presentation();
    let base = frame(&mut presentation, [1280, 720]);
    presentation.set_mod_panel(Some(&panel())).unwrap();
    assert!(base == frame(&mut presentation, [1280, 720]));
    presentation.set_mod_panel_open(true);
    let open = frame(&mut presentation, [1280, 720]);
    assert!(snapshot::rasterize(&base) != snapshot::rasterize(&open));
    presentation.set_mod_panel(None).unwrap();
    assert!(!presentation.mod_panel_open());
    assert!(
        presentation
            .mod_panel_events([500.0, 400.0], true, true)
            .is_empty()
    );
    let revoked = frame(&mut presentation, [1280, 720]);
    assert!(snapshot::rasterize(&base) == snapshot::rasterize(&revoked));
    assert!(base.vertices == revoked.vertices);
    assert!(base.indices == revoked.indices);
}

#[test]
fn actual_engine_geometry_routes_all_control_types() {
    let mut presentation = mini_engine_presentation();
    presentation.set_mod_panel(Some(&panel())).unwrap();
    presentation.set_mod_panel_open(true);
    assert!(
        presentation
            .mod_panel_events([500.0, 400.0], true, true)
            .is_empty()
    );
    frame(&mut presentation, [1280, 720]);
    let toggle = point(&presentation, "mod.control:0", 0.5);
    assert_eq!(
        presentation.mod_panel_events(toggle, true, true),
        vec![Event {
            id: "enabled".into(),
            value: 1.0
        }]
    );
    assert!(
        presentation
            .mod_panel_events(toggle, false, true)
            .is_empty()
    );
    let choice = point(&presentation, "mod.control:2", 0.5);
    assert_eq!(
        presentation.mod_panel_events(choice, true, true),
        vec![Event {
            id: "mode".into(),
            value: 1.0
        }]
    );
    let button = point(&presentation, "mod.control:3", 0.5);
    assert_eq!(
        presentation.mod_panel_events(button, true, true),
        vec![Event {
            id: "binding".into(),
            value: 1.0
        }]
    );
    let slider = point(&presentation, "mod.control:1", 0.62);
    assert_eq!(
        presentation.mod_panel_events(slider, true, true),
        vec![Event {
            id: "strength".into(),
            value: 62.0
        }]
    );
    let beyond = [20_000.0, slider[1]];
    assert_eq!(
        presentation.mod_panel_events(beyond, false, true),
        vec![Event {
            id: "strength".into(),
            value: 100.0
        }]
    );
    assert!(
        presentation
            .mod_panel_events(beyond, false, true)
            .is_empty()
    );
    presentation.mod_panel_events(beyond, false, false);
    assert!(
        presentation
            .mod_panel_events(slider, false, true)
            .is_empty()
    );
}

#[test]
fn value_updates_reuse_catalog_and_unchanged_frame_reuses_layout() {
    let mut presentation = mini_engine_presentation();
    let mut settings = panel();
    presentation.set_mod_panel(Some(&settings)).unwrap();
    presentation.set_mod_panel_open(true);
    let original_frame = frame(&mut presentation, [1280, 720]);
    let original = Arc::clone(
        presentation
            .form_presentation
            .mod_panel
            .as_ref()
            .unwrap()
            .catalog
            .as_ref()
            .unwrap(),
    );
    let before = presentation
        .form_presentation
        .mod_panel
        .as_ref()
        .unwrap()
        .screen
        .passes;
    frame(&mut presentation, [1280, 720]);
    assert_eq!(
        before,
        presentation
            .form_presentation
            .mod_panel
            .as_ref()
            .unwrap()
            .screen
            .passes
    );
    if let Control::Slider { value, .. } = &mut settings.controls[1] {
        *value = 64.0;
    }
    presentation.set_mod_panel(Some(&settings)).unwrap();
    let updated_frame = frame(&mut presentation, [1280, 720]);
    assert!(original_frame.vertices != updated_frame.vertices);
    assert!(Arc::ptr_eq(
        &original,
        presentation
            .form_presentation
            .mod_panel
            .as_ref()
            .unwrap()
            .catalog
            .as_ref()
            .unwrap()
    ));
    assert!(presentation.mod_panel_open());
    presentation.set_mod_panel_open(false);
    assert!(
        presentation
            .mod_panel_events([100.0, 100.0], true, true)
            .is_empty()
    );
}

#[test]
fn pagination_keeps_bounded_controls_reachable_on_small_viewports() {
    let mut presentation = mini_engine_presentation();
    let mut settings = panel();
    settings.controls = (0..ui::mod_panel::MAX_PANEL_CONTROLS)
        .map(|index| Control::Toggle {
            id: format!("toggle{index}"),
            label: format!("Control {index}"),
            value: false,
        })
        .collect();
    presentation.set_mod_panel(Some(&settings)).unwrap();
    presentation.set_mod_panel_open(true);
    frame(&mut presentation, [640, 480]);
    let next = point(&presentation, "mod.next", 0.5);
    assert!(presentation.mod_panel_events(next, true, true).is_empty());
    frame(&mut presentation, [640, 480]);
    let retained = presentation.form_presentation.mod_panel.as_ref().unwrap();
    assert_eq!(retained.page, 1);
    let index = retained.rows;
    let target = point(&presentation, &format!("mod.control:{index}"), 0.5);
    assert_eq!(
        presentation.mod_panel_events(target, true, true),
        vec![Event {
            id: format!("toggle{index}"),
            value: 1.0
        }]
    );
    presentation.set_mod_panel_open(false);
    assert!(
        presentation
            .form_presentation
            .mod_panel
            .as_ref()
            .unwrap()
            .frame
            .is_none()
    );
}

#[test]
fn both_themes_render_and_invalid_replacement_preserves_valid_panel() {
    let mut presentation = mini_engine_presentation();
    let mut settings = panel();
    presentation.set_mod_panel(Some(&settings)).unwrap();
    presentation.set_mod_panel_open(true);
    let dark = frame(&mut presentation, [1280, 720]);
    settings.dark = false;
    presentation.set_mod_panel(Some(&settings)).unwrap();
    let light = frame(&mut presentation, [1280, 720]);
    assert!(snapshot::rasterize(&dark) != snapshot::rasterize(&light));
    settings.controls.push(settings.controls[0].clone());
    assert!(presentation.set_mod_panel(Some(&settings)).is_err());
    assert!(light == frame(&mut presentation, [1280, 720]));
    let close = point(&presentation, "mod.close", 0.5);
    presentation.mod_panel_events(close, true, true);
    assert!(!presentation.mod_panel_open());
    assert!(
        presentation
            .form_presentation
            .mod_panel
            .as_ref()
            .unwrap()
            .frame
            .is_none()
    );
}

#[test]
fn grouped_cards_render_solid_chrome_and_categories_route_locally() {
    let mut presentation = mini_engine_presentation();
    let mut settings = panel();
    settings.sections = vec![
        ui::mod_panel::Section {
            icon: Default::default(),
            id: "module".into(),
            label: "Module".into(),
            category: "Combat".into(),
            toggle: Some("enabled".into()),
            controls: vec!["strength".into(), "mode".into()],
        },
        ui::mod_panel::Section {
            icon: Default::default(),
            id: "settings".into(),
            label: "General".into(),
            category: "Settings".into(),
            toggle: None,
            controls: vec!["binding".into()],
        },
    ];
    presentation.set_mod_panel(Some(&settings)).unwrap();
    presentation.set_mod_panel_open(true);
    let input = frame(&mut presentation, [1280, 720]);
    let image = snapshot::rasterize(&input);
    let toggle = point(&presentation, "mod.control:0", 0.5);
    // A blank part of the module card, below its toggle, must be filled. This
    // catches invisible layout-only panels that otherwise leave text floating.
    let pixel = image
        .get_pixel((toggle[0] - 5.0) as u32, (toggle[1] + 25.0) as u32)
        .0;
    assert!(
        pixel[0] < 45 && pixel[1] < 45 && pixel[2] < 45,
        "card background missing: {pixel:?}"
    );
    let tab = point(&presentation, "mod.category:1", 0.5);
    assert!(presentation.mod_panel_events(tab, true, true).is_empty());
    frame(&mut presentation, [1280, 720]);
    assert_eq!(point(&presentation, "mod.category:1", 0.5), tab);
    assert_eq!(
        presentation
            .form_presentation
            .mod_panel
            .as_ref()
            .unwrap()
            .category,
        1
    );
    let binding = point(&presentation, "mod.control:3", 0.5);
    assert_eq!(
        presentation.mod_panel_events(binding, true, true),
        vec![Event {
            id: "binding".into(),
            value: 1.0
        }]
    );
    let retained = presentation.form_presentation.mod_panel.as_ref().unwrap();
    assert!(
        !retained
            .frame
            .as_ref()
            .unwrap()
            .hits
            .iter()
            .any(|hit| hit.pressed.as_deref() == Some("mod.control:0"))
    );
    settings.dark = false;
    presentation.set_mod_panel(Some(&settings)).unwrap();
    frame(&mut presentation, [1280, 720]);
    assert_eq!(
        presentation
            .form_presentation
            .mod_panel
            .as_ref()
            .unwrap()
            .category,
        1
    );
    settings.capture_key = true;
    presentation.set_mod_panel(Some(&settings)).unwrap();
    assert!(
        presentation
            .form_presentation
            .mod_panel
            .as_ref()
            .unwrap()
            .frame
            .is_some()
    );
}

#[test]
fn oversized_group_pages_keep_every_control_reachable_and_inside_the_viewport() {
    let mut presentation = mini_engine_presentation();
    let mut settings = panel();
    settings.controls = (0..24)
        .map(|index| Control::Toggle {
            id: format!("toggle{index}"),
            label: format!("Control {index}"),
            value: false,
        })
        .collect();
    settings.sections = vec![ui::mod_panel::Section {
        icon: Default::default(),
        id: "group".into(),
        label: "Group".into(),
        category: "Controls".into(),
        toggle: None,
        controls: settings
            .controls
            .iter()
            .map(|control| control.id().to_owned())
            .collect(),
    }];
    presentation.set_mod_panel(Some(&settings)).unwrap();
    presentation.set_mod_panel_open(true);
    frame(&mut presentation, [480, 360]);
    let count = presentation
        .form_presentation
        .mod_panel
        .as_ref()
        .unwrap()
        .pages;
    let mut seen = std::collections::HashSet::new();
    for page in 0..count {
        let retained = presentation.form_presentation.mod_panel.as_ref().unwrap();
        let rendered = retained.frame.as_ref().unwrap();
        for region in rendered.hits.iter() {
            if let Some(index) = region
                .pressed
                .as_deref()
                .and_then(|action| action.strip_prefix("mod.control:"))
            {
                seen.insert(index.to_owned());
                let right =
                    rendered.origin[0] + (region.rect.x + region.rect.w) as f32 * rendered.scale;
                let bottom =
                    rendered.origin[1] + (region.rect.y + region.rect.h) as f32 * rendered.scale;
                assert!(
                    right <= 480.0 && bottom <= 360.0,
                    "page {page} control clipped"
                );
            }
        }
        if page + 1 < count {
            let next = point(&presentation, "mod.next", 0.5);
            presentation.mod_panel_events(next, true, true);
            frame(&mut presentation, [480, 360]);
        }
    }
    assert_eq!(seen.len(), 24);
}

#[test]
fn desktop_panel_geometry_stays_stable_across_game_gui_preferences_and_dpi() {
    let mut presentation = mini_engine_presentation();
    presentation.set_mod_panel(Some(&panel())).unwrap();
    presentation.set_mod_panel_open(true);
    presentation.set_gui_scale_preference(Some(2));
    frame(&mut presentation, [1280, 720]);
    let expected = point(&presentation, "mod.control:1", 0.5);
    for preference in [1, 3, 4] {
        presentation.set_gui_scale_preference(Some(preference));
        frame(&mut presentation, [1280, 720]);
        assert_eq!(point(&presentation, "mod.control:1", 0.5), expected);
    }
    presentation
        .build(
            &player_state::PlayerState::new(1),
            &UiRuntime::new(1),
            0,
            [1600, 900],
            DpiScale::new(1.25).unwrap(),
        )
        .unwrap();
    assert_eq!(point(&presentation, "mod.control:1", 0.5), expected);
    assert!(
        !presentation
            .mod_panel_events(expected, true, true)
            .is_empty()
    );
}

#[test]
fn compact_keybind_updates_retain_geometry_and_emit_capture_event() {
    let mut presentation = mini_engine_presentation();
    let mut settings = panel();
    let mut legacy = serde_json::to_value(&settings).unwrap();
    legacy.as_object_mut().unwrap().remove("style");
    let parsed: Panel = serde_json::from_value(legacy).unwrap();
    assert_eq!(parsed.style, ui::mod_panel::Style::Standard);
    settings.style = ui::mod_panel::Style::Compact;
    settings.controls[3] = Control::Keybind {
        id: "binding".into(),
        label: "Keybind".into(),
        key: "KeyR".into(),
        capturing: false,
    };
    presentation.set_mod_panel(Some(&settings)).unwrap();
    presentation.set_mod_panel_open(true);
    frame(&mut presentation, [1280, 720]);
    let retained = presentation.form_presentation.mod_panel.as_ref().unwrap();
    let catalog = retained.catalog.as_ref().unwrap().clone();
    let binding = point(&presentation, "mod.control:3", 0.5);
    let event = presentation.mod_panel_events(binding, true, true);
    assert_eq!(
        event,
        vec![Event {
            id: "binding".into(),
            value: 1.
        }]
    );
    if let Control::Keybind { key, capturing, .. } = &mut settings.controls[3] {
        *key = "KeyV".into();
        *capturing = true;
    }
    presentation.set_mod_panel(Some(&settings)).unwrap();
    let retained = presentation.form_presentation.mod_panel.as_ref().unwrap();
    assert!(Arc::ptr_eq(&catalog, retained.catalog.as_ref().unwrap()));
    frame(&mut presentation, [1280, 720]);
    assert_eq!(point(&presentation, "mod.control:3", 0.5), binding);
}

#[test]
fn compact_module_cards_align_and_wrap_without_losing_bindings() {
    use ui::mod_panel::{Icon, Section, Style};
    let mut settings = panel();
    settings.style = Style::Compact;
    settings.controls.clear();
    settings.sections = (0..3)
        .map(|index| {
            settings.controls.extend([
                Control::Toggle {
                    id: format!("enabled{index}"),
                    label: "Enabled".into(),
                    value: false,
                },
                Control::Keybind {
                    id: format!("binding{index}"),
                    label: "Keybind".into(),
                    key: "F8".into(),
                    capturing: false,
                },
            ]);
            Section {
                id: format!("section{index}"),
                label: "Module".into(),
                category: "Modules".into(),
                icon: Icon::Pointer,
                toggle: Some(format!("enabled{index}")),
                controls: vec![format!("binding{index}")],
            }
        })
        .collect();
    let wide = layout::Layout::new(&settings, [600., 400.], 0, 10);
    assert_eq!(wide.pages.len(), 1);
    assert_eq!(wide.pages[0].len(), 3);
    let baseline = wide.pages[0][0].offset[1] + wide.pages[0][0].height;
    assert!(
        wide.pages[0]
            .iter()
            .all(|card| card.offset[1] + card.height == baseline)
    );
    let narrow = layout::Layout::new(&settings, [260., 200.], 0, 3);
    let bindings: Vec<_> = narrow
        .pages
        .iter()
        .flatten()
        .flat_map(|card| &card.controls)
        .collect();
    assert_eq!(bindings.len(), 3);
    assert!(
        narrow
            .pages
            .iter()
            .flatten()
            .all(|card| card.offset[0] + narrow.card_width <= narrow.width)
    );
}

#[test]
fn keycaps_use_readable_ascii_names_for_modifiers_and_extended_keys() {
    for (physical, visible) in [
        ("KeyR", "R"),
        ("Digit9", "9"),
        ("ControlRight", "RCtrl"),
        ("ShiftLeft", "LShift"),
        ("NumpadDivide", "Num/"),
        ("ArrowRight", "Right"),
        ("PrintScreen", "PrtSc"),
        ("Numpad9", "Num9"),
    ] {
        assert_eq!(data::key_label(physical), visible);
        assert!(visible.is_ascii());
    }
}

#[test]
fn compact_short_pages_keep_mixed_controls_inside_the_viewport_and_separate() {
    for size in [[640, 480], [800, 600]] {
        let mut presentation = mini_engine_presentation();
        let mut settings = panel();
        settings.style = ui::mod_panel::Style::Compact;
        settings.controls[3] = Control::Keybind {
            id: "binding".into(),
            label: "Keybind".into(),
            key: "KeyR".into(),
            capturing: false,
        };
        settings.sections = vec![ui::mod_panel::Section {
            id: "module".into(),
            label: "Module".into(),
            category: "Modules".into(),
            icon: ui::mod_panel::Icon::Crosshair,
            toggle: Some("enabled".into()),
            controls: vec!["strength".into(), "mode".into(), "binding".into()],
        }];
        presentation.set_mod_panel(Some(&settings)).unwrap();
        presentation.set_mod_panel_open(true);
        frame(&mut presentation, size);
        let count = presentation
            .form_presentation
            .mod_panel
            .as_ref()
            .unwrap()
            .pages;
        let mut seen = std::collections::HashSet::new();
        for page in 0..count {
            let rendered = presentation
                .form_presentation
                .mod_panel
                .as_ref()
                .unwrap()
                .frame
                .as_ref()
                .unwrap();
            let controls: Vec<_> = rendered
                .hits
                .iter()
                .filter(|hit| {
                    hit.pressed
                        .as_deref()
                        .is_some_and(|action| action.starts_with("mod.control:"))
                })
                .collect();
            for (index, hit) in controls.iter().enumerate() {
                seen.insert(hit.pressed.as_deref().unwrap().to_owned());
                let right = rendered.origin[0] + (hit.rect.x + hit.rect.w) as f32 * rendered.scale;
                let bottom = rendered.origin[1] + (hit.rect.y + hit.rect.h) as f32 * rendered.scale;
                assert!(
                    right <= size[0] as f32 && bottom <= size[1] as f32,
                    "clipped control on {size:?} page{page}"
                );
                for other in &controls[index + 1..] {
                    if hit.pressed == other.pressed {
                        continue;
                    }
                    let overlap_x = hit.rect.x < other.rect.x + other.rect.w
                        && other.rect.x < hit.rect.x + hit.rect.w;
                    let overlap_y = hit.rect.y < other.rect.y + other.rect.h
                        && other.rect.y < hit.rect.y + hit.rect.h;
                    assert!(
                        !(overlap_x && overlap_y),
                        "overlapping controls on {size:?} page{page}"
                    );
                }
            }
            if page + 1 < count {
                let next = point(&presentation, "mod.next", 0.5);
                assert!(next[0] < size[0] as f32 && next[1] < size[1] as f32);
                presentation.mod_panel_events(next, true, true);
                frame(&mut presentation, size);
            }
        }
        assert_eq!(
            seen.len(),
            settings.controls.len(),
            "unreachable control on {size:?}"
        );
    }
}

#[test]
fn standard_keybind_displays_assigned_key_and_capture_state_without_rebuilding() {
    let mut presentation = mini_engine_presentation();
    let mut settings = panel();
    settings.controls = vec![Control::Keybind {
        id: "binding".into(),
        label: "Keybind".into(),
        key: "ControlRight".into(),
        capturing: false,
    }];
    presentation.set_mod_panel(Some(&settings)).unwrap();
    presentation.set_mod_panel_open(true);
    let assigned = frame(&mut presentation, [1280, 720]);
    let catalog = presentation
        .form_presentation
        .mod_panel
        .as_ref()
        .unwrap()
        .catalog
        .as_ref()
        .unwrap()
        .clone();
    let keycap = point(&presentation, "mod.control:0", 0.5);
    assert_eq!(
        presentation.mod_panel_events(keycap, true, true),
        vec![Event {
            id: "binding".into(),
            value: 1.
        }]
    );
    if let Control::Keybind { capturing, .. } = &mut settings.controls[0] {
        *capturing = true;
    }
    presentation.set_mod_panel(Some(&settings)).unwrap();
    let capture = frame(&mut presentation, [1280, 720]);
    assert_ne!(
        assigned.vertices, capture.vertices,
        "keycap must render its changing bound value"
    );
    assert!(Arc::ptr_eq(
        &catalog,
        presentation
            .form_presentation
            .mod_panel
            .as_ref()
            .unwrap()
            .catalog
            .as_ref()
            .unwrap()
    ));
    assert_eq!(point(&presentation, "mod.control:0", 0.5), keycap);
}

#[test]
fn smallest_compact_viewport_keeps_four_category_targets_and_close_usable() {
    let mut settings = panel();
    settings.style = ui::mod_panel::Style::Compact;
    settings.controls = (0..4)
        .map(|index| Control::Toggle {
            id: format!("toggle{index}"),
            label: "Enabled".into(),
            value: false,
        })
        .collect();
    settings.sections = (0..4)
        .map(|index| ui::mod_panel::Section {
            id: format!("section{index}"),
            label: "Module".into(),
            category: format!("Category {index}"),
            icon: ui::mod_panel::Icon::Crosshair,
            toggle: Some(format!("toggle{index}")),
            controls: vec![],
        })
        .collect();
    let mut presentation = mini_engine_presentation();
    presentation.set_mod_panel(Some(&settings)).unwrap();
    presentation.set_mod_panel_open(true);
    frame(&mut presentation, [240, 640]);
    let rendered = presentation
        .form_presentation
        .mod_panel
        .as_ref()
        .unwrap()
        .frame
        .as_ref()
        .unwrap();
    let navigation: Vec<_> = rendered
        .hits
        .iter()
        .filter(|hit| {
            hit.pressed
                .as_deref()
                .is_some_and(|action| action.starts_with("mod.category:") || action == "mod.close")
        })
        .collect();
    assert_eq!(navigation.len(), 5);
    for (index, hit) in navigation.iter().enumerate() {
        assert!(
            hit.rect.w >= 12. && hit.rect.h >= 20.,
            "navigation target too small"
        );
        let left = rendered.origin[0] + hit.rect.x as f32 * rendered.scale;
        let right = left + hit.rect.w as f32 * rendered.scale;
        assert!(left >= 0. && right <= 240., "navigation clipped");
        for other in &navigation[index + 1..] {
            assert!(
                hit.rect.x + hit.rect.w <= other.rect.x
                    || other.rect.x + other.rect.w <= hit.rect.x,
                "navigation targets overlap"
            );
        }
    }
    for category in 0..4 {
        let target = point(&presentation, &format!("mod.category:{category}"), 0.5);
        presentation.mod_panel_events(target, true, true);
        frame(&mut presentation, [240, 640]);
        assert_eq!(
            presentation
                .form_presentation
                .mod_panel
                .as_ref()
                .unwrap()
                .category,
            category
        );
    }
    let close = point(&presentation, "mod.close", 0.5);
    presentation.mod_panel_events(close, true, true);
    assert!(!presentation.mod_panel_open());
}
