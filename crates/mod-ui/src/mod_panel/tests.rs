use super::*;
use crate::test_support::sample_panel as panel;

/// Prepares two authored routes with distinct wrapper facts for each owner or revision.
fn prepared_panels(title: &str) -> ([Panel; 2], Arc<prepared::PreparedPanelUi>) {
    let mut content = panel();
    content.title = title.into();
    content.surface = Some(ui::mod_panel::Surface {
        screen: "extension.screen".into(),
        document: serde_json::json!({
            "namespace": "extension",
            "screen": {"type": "screen", "size": ["100%", "100%"], "controls": []}
        })
        .to_string(),
        bindings: Default::default(),
    });
    let mut other = content.clone();
    other.surface.as_mut().unwrap().document = serde_json::json!({
        "namespace": "extension",
        "screen": {"type": "screen", "size": [80, 60], "controls": []}
    })
    .to_string();
    let bundle = Arc::new(ui::mod_panel::PanelBundle {
        panel: content.clone(),
        surfaces: std::collections::BTreeMap::from([(
            "other".into(),
            other.surface.as_ref().unwrap().clone(),
        )]),
    });
    let prepared = prepared::PreparedPanelUi::prepare(bundle).unwrap();
    ([content, other], prepared)
}

/// Replacing a registration retires its old bundles and preserves an active environment route.
#[test]
fn registration_replacements_preserve_environment_prepared_routes() {
    use prepared::PanelUiOwner;

    let ([environment, _], prepared_environment) = prepared_panels("Environment");
    let environment_route = prepared_environment.route(&environment).unwrap();
    let mut ui = ModUi::default();
    ui.set_prepared_panel_ui(PanelUiOwner::Environment(0), Some(prepared_environment));
    ui.set_mod_panel(PanelUiOwner::Environment(0), Some(&environment))
        .unwrap();
    let mut previous: Option<std::sync::Weak<prepared::PreparedPanelUi>> = None;
    for revision in 0..5 {
        let ([registration, _], prepared_registration) =
            prepared_panels(&format!("Registration {revision}"));
        let current = Arc::downgrade(&prepared_registration);
        ui.set_prepared_panel_ui(PanelUiOwner::Registration, Some(prepared_registration));
        if let Some(previous) = previous {
            assert!(previous.upgrade().is_none(), "superseded bundle is retired");
        }
        previous = Some(current);
        ui.set_mod_panel(PanelUiOwner::Registration, Some(&registration))
            .unwrap();
        assert!(ui.mod_panel.as_ref().unwrap().prepared.is_some());
    }

    ui.set_mod_panel(PanelUiOwner::Environment(0), Some(&environment))
        .unwrap();
    assert!(Arc::ptr_eq(
        ui.mod_panel.as_ref().unwrap().prepared.as_ref().unwrap(),
        &environment_route,
    ));
    ui.set_prepared_panel_ui(PanelUiOwner::Registration, None);
    assert!(previous.unwrap().upgrade().is_none());
    ui.set_mod_panel(PanelUiOwner::Environment(0), None)
        .unwrap();
    ui.set_mod_panel(PanelUiOwner::Environment(0), Some(&environment))
        .unwrap();
    assert!(Arc::ptr_eq(
        ui.mod_panel.as_ref().unwrap().prepared.as_ref().unwrap(),
        &environment_route,
    ));
}

/// Lays out the selected route in its owner's cache without a renderer or game assets.
fn render_prepared_route(ui: &mut ModUi, owner: PanelUiOwner, content: &Panel) -> usize {
    ui.set_mod_panel(owner, Some(content)).unwrap();
    let panel = ui.mod_panel.as_mut().unwrap();
    panel.viewport = [600.0, 400.0];
    panel.refresh_data();
    let route = panel.prepared.as_ref().unwrap();
    let screen = ui
        .prepared_panel_ui
        .get_mut(&panel.owner)
        .unwrap()
        .screen(route);
    assert!(
        screen
            .render(
                SCREEN,
                &route.catalog,
                &Context::default(),
                Arc::clone(&panel.data),
                (panel.viewport, 1.0, [0; 3]),
                &json_ui::LayoutEnv {
                    text: &InputArt,
                    textures: &InputArt,
                },
            )
            .is_some()
    );
    screen.passes
}

/// Retiring an owner releases all route generations while other owners keep their rendered caches.
#[test]
fn ten_registration_replacements_release_routes_and_preserve_environment_caches() {
    use prepared::PanelUiOwner;

    let (environment, prepared_environment) = prepared_panels("Environment");
    let mut ui = ModUi::default();
    ui.set_prepared_panel_ui(PanelUiOwner::Environment(0), Some(prepared_environment));
    let passes: Vec<_> = environment
        .iter()
        .map(|panel| render_prepared_route(&mut ui, PanelUiOwner::Environment(0), panel))
        .collect();
    let mut retired: Vec<std::sync::Weak<Catalog>> = Vec::new();
    for revision in 0..10 {
        let (registration, prepared_registration) =
            prepared_panels(&format!("Registration {revision}"));
        let catalogs: Vec<_> = registration
            .iter()
            .map(|panel| {
                let route = prepared_registration.route(panel).unwrap();
                Arc::downgrade(&route.catalog)
            })
            .collect();
        ui.set_prepared_panel_ui(PanelUiOwner::Registration, Some(prepared_registration));
        assert!(retired.iter().all(|catalog| catalog.upgrade().is_none()));
        for panel in &registration {
            render_prepared_route(&mut ui, PanelUiOwner::Registration, panel);
        }
        let registered = &ui.prepared_panel_ui[&PanelUiOwner::Registration];
        assert_eq!(registered.screens.len(), registration.len());
        assert_eq!(
            registered
                .screens
                .iter()
                .filter(|screen| screen.is_some())
                .count(),
            registration.len(),
        );
        for (panel, passes) in environment.iter().zip(&passes) {
            assert_eq!(
                render_prepared_route(&mut ui, PanelUiOwner::Environment(0), panel),
                *passes
            );
        }
        assert_eq!(
            ui.prepared_panel_ui
                .values()
                .map(|owner| owner.screens.len())
                .sum::<usize>(),
            environment.len() + registration.len(),
        );
        render_prepared_route(&mut ui, PanelUiOwner::Registration, &registration[0]);
        retired.extend(catalogs);
    }

    ui.set_prepared_panel_ui(PanelUiOwner::Registration, None);
    assert!(ui.mod_panel.is_none());
    assert!(retired.iter().all(|catalog| catalog.upgrade().is_none()));
    for (panel, passes) in environment.iter().zip(&passes) {
        assert_eq!(
            render_prepared_route(&mut ui, PanelUiOwner::Environment(0), panel),
            *passes
        );
    }
}

/// Identical documents never transfer an environment panel's ownership to a registration.
#[test]
fn identical_registration_replacements_preserve_environment_pointer_capture() {
    let ([environment, _], prepared_environment) = prepared_panels("Identical");
    let ([registration, _], prepared_registration) = prepared_panels("Identical");
    assert_eq!(environment, registration);
    let environment_route = prepared_environment.route(&environment).unwrap();
    let mut ui = ModUi::default();
    ui.set_prepared_panel_ui(PanelUiOwner::Environment(0), Some(prepared_environment));
    ui.set_prepared_panel_ui(PanelUiOwner::Registration, Some(prepared_registration));
    ui.set_mod_panel(PanelUiOwner::Registration, Some(&registration))
        .unwrap();
    ui.set_mod_panel(PanelUiOwner::Environment(0), Some(&environment))
        .unwrap();
    ui.set_mod_panel_open(true);
    {
        let current = ui.mod_panel.as_mut().unwrap();
        current.view.scroll.insert("list".into(), 42.0);
        current.view.pressed = Some("slider".into());
        current.pointer = Some([70.0, 20.0]);
        current.held = true;
        current.drag = Some(1);
    }
    let data = Arc::clone(&ui.mod_panel.as_ref().unwrap().data);
    let (_, replacement) = prepared_panels("Identical");
    for replacement in [Some(replacement), None] {
        ui.set_prepared_panel_ui(PanelUiOwner::Registration, replacement);
        ui.set_mod_panel(PanelUiOwner::Registration, None).unwrap();
        ui.set_mod_panel(PanelUiOwner::Environment(0), Some(&environment))
            .unwrap();
        let current = ui
            .mod_panel
            .as_ref()
            .expect("environment publisher stays selected");
        assert_eq!(current.owner, PanelUiOwner::Environment(0));
        assert!(current.open);
        assert!(Arc::ptr_eq(
            current.prepared.as_ref().unwrap(),
            &environment_route
        ));
        assert!(Arc::ptr_eq(&current.data, &data));
        assert_eq!(current.view.scroll_offset("list"), 42.0);
        assert_eq!(current.view.pressed.as_deref(), Some("slider"));
        assert_eq!(current.pointer, Some([70.0, 20.0]));
        assert!(current.held);
        assert_eq!(current.drag, Some(1));
    }
    ui.set_mod_panel(PanelUiOwner::Environment(1), Some(&environment))
        .unwrap();
    assert!(ui.mod_panel.as_ref().unwrap().prepared.is_none());
}

struct InputArt;

impl json_ui::TextMeasure for InputArt {
    /// Supplies empty text measurements for tests of authored input geometry.
    fn extent(&self, _: &str) -> [f64; 2] {
        [0.0; 2]
    }
}

impl json_ui::TextureSource for InputArt {
    /// Uses no textures when laying out the native panel's input regions.
    fn texture(&self, _: &str) -> Option<json_ui::TextureMeta> {
        None
    }
}

/// Resolves the native panel's input regions without a renderer or local game assets.
fn panel_frame(panel: &ModPanel) -> EngineFrame {
    let (catalog, _) = template::catalog(
        &panel.panel,
        panel.viewport,
        panel.category,
        panel.page,
        panel.rows,
        panel.edit.as_ref(),
    )
    .unwrap();
    let root = json_ui::resolve(&catalog, SCREEN, &Context::default())
        .control
        .unwrap();
    let bound = json_ui::bind(&root, &panel.data, &json_ui::EmptyLibrary);
    let (laid, report) = json_ui::layout_with(
        &bound,
        panel.viewport,
        &json_ui::LayoutEnv {
            text: &InputArt,
            textures: &InputArt,
        },
        &panel.view,
    );
    EngineFrame {
        hits: json_ui::hit_regions(&laid).into(),
        report,
        cancel_target: None,
        origin: [0.0; 2],
        scale: 1.0,
        panel: None,
        edit_texts: Vec::new(),
        top: Vec::new(),
    }
}

/// The close button clears scroll and motion immediately, while later close calls reuse data.
#[test]
fn close_button_resets_scroll_and_feedback_before_reopening() {
    for motion_enabled in [false, true] {
        let mut settings = panel();
        settings.motion = motion_enabled;
        let mut ui = ModUi::default();
        ui.set_mod_panel(PanelUiOwner::Environment(0), Some(&settings))
            .unwrap();
        let settled = Arc::clone(&ui.mod_panel.as_ref().unwrap().data);
        ui.set_mod_panel_open(true);
        let close_at = {
            let current = ui.mod_panel.as_mut().unwrap();
            current.viewport = [600.0, 400.0];
            current.rows = 10;
            current.view.scroll.insert("list".into(), 40.0);
            if motion_enabled {
                current
                    .motion
                    .advance(&current.panel, &mut current.view, Some(0), 1.0);
                current.motion.scroll("list", 40.0, 30.0, 100.0);
                current
                    .motion
                    .advance(&current.panel, &mut current.view, Some(0), 1.03);
                current
                    .motion
                    .write(&current.panel, Arc::make_mut(&mut current.data));
                assert!(current.motion.active());
                assert_ne!(current.data, settled);
            }
            let frame = panel_frame(current);
            let close = frame
                .hits
                .iter()
                .find(|hit| hit.pressed.as_deref() == Some("mod.close"))
                .expect("native panel has a close button");
            let at = [
                (close.rect.x + close.rect.w * 0.5) as f32,
                (close.rect.y + close.rect.h * 0.5) as f32,
            ];
            current.frame = Some(frame);
            at
        };

        assert!(ui.mod_panel_events(close_at, true, true).is_empty());
        assert!(!ui.mod_panel_open());
        let closed = ui.mod_panel.as_ref().unwrap();
        assert_eq!(closed.view.scroll_offset("list"), 0.0);
        assert!(!closed.motion.active());
        assert!(closed.view.hovered.is_none());
        assert!(closed.frame.is_none());
        assert_eq!(closed.data, settled);
        let closed_data = Arc::clone(&closed.data);
        for _ in 0..3 {
            ui.set_mod_panel_open(false);
            assert!(Arc::ptr_eq(
                &ui.mod_panel.as_ref().unwrap().data,
                &closed_data
            ));
        }

        ui.set_mod_panel_open(true);
        let reopened = ui.mod_panel.as_ref().unwrap();
        assert_eq!(reopened.view.scroll_offset("list"), 0.0);
        assert!(!reopened.motion.active());
        assert!(reopened.view.hovered.is_none());
        assert_eq!(reopened.data, settled);
    }
}

/// Same-state calls and closing without animated feedback retain the published data source.
#[test]
fn closed_and_settled_panels_reuse_feedback_data() {
    for motion_enabled in [false, true] {
        let mut settings = panel();
        settings.motion = motion_enabled;
        let mut ui = ModUi::default();
        ui.set_mod_panel(PanelUiOwner::Environment(0), Some(&settings))
            .unwrap();
        let data = Arc::clone(&ui.mod_panel.as_ref().unwrap().data);
        for _ in 0..3 {
            ui.set_mod_panel_open(false);
            assert!(Arc::ptr_eq(&ui.mod_panel.as_ref().unwrap().data, &data));
        }

        ui.set_mod_panel_open(true);
        if motion_enabled {
            let current = ui.mod_panel.as_mut().unwrap();
            current
                .motion
                .advance(&current.panel, &mut current.view, None, 1.0);
            current.motion.scroll("list", 0.0, 30.0, 100.0);
            current
                .motion
                .advance(&current.panel, &mut current.view, None, 1.03);
            assert!(current.motion.active());
        }
        for _ in 0..3 {
            ui.set_mod_panel_open(false);
            assert!(Arc::ptr_eq(&ui.mod_panel.as_ref().unwrap().data, &data));
        }
    }
}

/// A completed hover tween still needs its highlight removed when the panel closes.
#[test]
fn closing_panel_restores_settled_hover_bindings() {
    let mut settings = panel();
    settings.motion = true;
    let mut ui = ModUi::default();
    ui.set_mod_panel(PanelUiOwner::Environment(0), Some(&settings))
        .unwrap();
    let settled = Arc::clone(&ui.mod_panel.as_ref().unwrap().data);
    ui.set_mod_panel_open(true);
    {
        let current = ui.mod_panel.as_mut().unwrap();
        current
            .motion
            .advance(&current.panel, &mut current.view, Some(0), 1.0);
        current
            .motion
            .advance(&current.panel, &mut current.view, Some(0), 1.2);
        current
            .motion
            .write(&current.panel, Arc::make_mut(&mut current.data));
        assert!(!current.motion.active());
        assert_ne!(current.data, settled);
    }

    ui.set_mod_panel_open(false);
    ui.set_mod_panel_open(true);
    assert_eq!(ui.mod_panel.as_ref().unwrap().data, settled);
}

/// Reopening unchanged content clears settled hover and restores a toggle closed mid-transition.
#[test]
fn reopening_panel_restores_settled_feedback_bindings() {
    use ui::mod_panel::Theme;

    for theme in [Theme::Default, Theme::Monochrome] {
        for enabled in [false, true] {
            let mut settings = panel();
            settings.motion = true;
            settings.theme = theme;
            if let Control::Toggle { value, .. } = &mut settings.controls[0] {
                *value = !enabled;
            }
            let mut ui = ModUi::default();
            ui.set_mod_panel(PanelUiOwner::Environment(0), Some(&settings))
                .unwrap();
            ui.set_mod_panel_open(true);
            {
                let current = ui.mod_panel.as_mut().unwrap();
                current.viewport = [600.0, 400.0];
                current
                    .motion
                    .advance(&current.panel, &mut current.view, Some(0), 1.0);
                current
                    .motion
                    .advance(&current.panel, &mut current.view, Some(0), 1.2);
                current
                    .motion
                    .write(&current.panel, Arc::make_mut(&mut current.data));
            }
            if let Control::Toggle { value, .. } = &mut settings.controls[0] {
                *value = enabled;
            }
            ui.set_mod_panel(PanelUiOwner::Environment(0), Some(&settings))
                .unwrap();
            let settled = Arc::clone(&ui.mod_panel.as_ref().unwrap().data);
            {
                let current = ui.mod_panel.as_mut().unwrap();
                current
                    .motion
                    .advance(&current.panel, &mut current.view, Some(0), 1.2);
                current
                    .motion
                    .advance(&current.panel, &mut current.view, Some(0), 1.24);
                current
                    .motion
                    .write(&current.panel, Arc::make_mut(&mut current.data));
                assert_ne!(current.data, settled);
            }
            assert!(ui.mod_panel_motion_active());

            ui.set_mod_panel_open(false);
            let closed_data = Arc::clone(&ui.mod_panel.as_ref().unwrap().data);
            ui.set_mod_panel_open(false);
            assert!(Arc::ptr_eq(
                &ui.mod_panel.as_ref().unwrap().data,
                &closed_data
            ));
            ui.set_mod_panel_open(true);
            ui.set_mod_panel(PanelUiOwner::Environment(0), Some(&settings))
                .unwrap();

            let reopened = ui.mod_panel.as_ref().unwrap();
            assert_eq!(reopened.viewport, [600.0, 400.0]);
            assert_eq!(reopened.data, settled);
        }
    }
}

/// Bound labels are values rather than static geometry and cannot interrupt a captured drag.
#[test]
fn relabeled_controls_keep_the_retained_catalog_shape() {
    let original = panel();
    let mut changed = original.clone();
    if let Control::Toggle { label, value, .. } = &mut changed.controls[0] {
        *label = "Updated title".into();
        *value = true;
    }
    assert!(template::same_shape(&original, &changed));
    assert!(changed.validate_values().is_ok());
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
fn long_numeric_draft_keeps_caret_visible_at_both_ends() {
    let text = "123456789012345678901234";
    assert_eq!(edit::visible_draft(text, false, 0, 42.), "|123456");
    assert_eq!(edit::visible_draft(text, false, 24, 42.), "901234|");
}
