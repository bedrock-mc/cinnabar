use super::*;
use crate::test_support::sample_panel as panel;

/// Custom artwork follows both toggle states through the actual global binding pipeline.
#[test]
fn authored_toggle_faces_follow_native_control_values() {
    let surface = ui::mod_panel::Surface { previews: Default::default(), collections: Default::default(),
        screen: "extension.screen".into(), bindings: Default::default(), transitions: Default::default(),
        document: serde_json::json!({"namespace":"extension","screen":{"type":"screen","controls":[
            {"enabled":{"type":"panel","bindings":[{"binding_name":"#row_0_toggle_enabled","binding_name_override":"#visible"}]}},
            {"disabled":{"type":"panel","bindings":[{"binding_name":"#row_0_toggle_disabled","binding_name_override":"#visible"}]}}
        ]}}).to_string(),
    };
    let catalog = super::surface::catalog(&surface).unwrap();
    let root = json_ui::resolve(&catalog, &surface.screen, &Context::default())
        .control
        .unwrap();
    let mut settings = panel();
    for enabled in [false, true, false] {
        if let Control::Toggle { value, .. } = &mut settings.controls[0] {
            *value = enabled;
        }
        let data = data::control_data(&settings);
        let bound = json_ui::bind(&root, &data, &json_ui::EmptyLibrary);
        assert_eq!(
            bound.child("enabled").unwrap().properties.get("visible"),
            Some(&serde_json::json!(enabled))
        );
        assert_eq!(
            bound.child("disabled").unwrap().properties.get("visible"),
            Some(&serde_json::json!(!enabled))
        );
    }
}

/// Prepares two authored routes with distinct wrapper facts for each owner or revision.
fn prepared_panels(title: &str) -> ([Panel; 2], Arc<prepared::PreparedPanelUi>) {
    let mut content = panel();
    content.title = title.into();
    content.surface = Some(ui::mod_panel::Surface {
        collections: Default::default(),
        previews: Default::default(),
        transitions: Default::default(),
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
        control_schemas: Default::default(),
        surfaces: std::collections::BTreeMap::from([(
            "other".into(),
            other.surface.as_ref().unwrap().clone(),
        )]),
    });
    let prepared = prepared::PreparedPanelUi::prepare(bundle).unwrap();
    ([content, other], prepared)
}

/// Prepared routes keep the schema used to validate their actions, even with identical artwork.
#[test]
fn prepared_routes_match_their_own_control_schema() {
    let ([initial, _], _) = prepared_panels("Route schemas");
    let mut options = initial.clone();
    options.controls = vec![Control::Button {
        id: "back".into(),
        label: "Back".into(),
    }];
    let bundle = Arc::new(ui::mod_panel::PanelBundle {
        panel: initial.clone(),
        surfaces: std::collections::BTreeMap::from([(
            "options".into(),
            options.surface.clone().unwrap(),
        )]),
        control_schemas: std::collections::BTreeMap::from([(
            "options".into(),
            options.controls.clone(),
        )]),
    });
    let prepared = prepared::PreparedPanelUi::prepare(bundle).unwrap();
    assert!(prepared.route(&initial).is_some());
    assert!(prepared.route(&options).is_some());
    options.controls[0] = Control::Button {
        id: "unregistered".into(),
        label: "Back".into(),
    };
    assert!(prepared.route(&options).is_none());
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
        ui.set_prepared_panel_ui(PanelUiOwner::Registration(1), Some(prepared_registration));
        if let Some(previous) = previous {
            assert!(previous.upgrade().is_none(), "superseded bundle is retired");
        }
        previous = Some(current);
        ui.set_mod_panel(PanelUiOwner::Registration(1), Some(&registration))
            .unwrap();
        assert!(ui.mod_panel.as_ref().unwrap().prepared.is_some());
    }

    ui.set_mod_panel(PanelUiOwner::Environment(0), Some(&environment))
        .unwrap();
    assert!(Arc::ptr_eq(
        ui.mod_panel.as_ref().unwrap().prepared.as_ref().unwrap(),
        &environment_route,
    ));
    ui.set_prepared_panel_ui(PanelUiOwner::Registration(1), None);
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
        ui.set_prepared_panel_ui(PanelUiOwner::Registration(1), Some(prepared_registration));
        assert!(retired.iter().all(|catalog| catalog.upgrade().is_none()));
        for panel in &registration {
            render_prepared_route(&mut ui, PanelUiOwner::Registration(1), panel);
        }
        let registered = &ui.prepared_panel_ui[&PanelUiOwner::Registration(1)];
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
        render_prepared_route(&mut ui, PanelUiOwner::Registration(1), &registration[0]);
        retired.extend(catalogs);
    }

    ui.set_prepared_panel_ui(PanelUiOwner::Registration(1), None);
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
    ui.set_prepared_panel_ui(PanelUiOwner::Registration(1), Some(prepared_registration));
    ui.set_mod_panel(PanelUiOwner::Registration(1), Some(&registration))
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
        ui.set_prepared_panel_ui(PanelUiOwner::Registration(1), replacement);
        ui.set_mod_panel(PanelUiOwner::Registration(1), None)
            .unwrap();
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
                    .advance(&current.panel, &mut current.view, Some(0), 1.0, true);
                current.motion.scroll("list", 40.0, 30.0, 100.0);
                current
                    .motion
                    .advance(&current.panel, &mut current.view, Some(0), 1.03, true);
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
                .advance(&current.panel, &mut current.view, None, 1.0, true);
            current.motion.scroll("list", 0.0, 30.0, 100.0);
            current
                .motion
                .advance(&current.panel, &mut current.view, None, 1.03, true);
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
            .advance(&current.panel, &mut current.view, Some(0), 1.0, true);
        current
            .motion
            .advance(&current.panel, &mut current.view, Some(0), 1.2, true);
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
                    .advance(&current.panel, &mut current.view, Some(0), 1.0, true);
                current
                    .motion
                    .advance(&current.panel, &mut current.view, Some(0), 1.2, true);
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
                    .advance(&current.panel, &mut current.view, Some(0), 1.2, true);
                current
                    .motion
                    .advance(&current.panel, &mut current.view, Some(0), 1.24, true);
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

/// A panel whose surface `#fade` binding eases from 1 to `exit` when the panel closes.
fn exiting_panel(exit: Option<f64>) -> Panel {
    use ui::mod_panel::{Surface, SurfaceTransition, SurfaceValue};

    let mut content = panel();
    content.surface = Some(Surface {
        collections: Default::default(),
        previews: Default::default(),
        screen: "extension.screen".into(),
        document: serde_json::json!({
            "namespace": "extension",
            "screen": {"type": "screen", "size": ["100%", "100%"], "controls": []}
        })
        .to_string(),
        bindings: std::collections::BTreeMap::from([("#fade".into(), SurfaceValue::Number(1.0))]),
        transitions: std::collections::BTreeMap::from([(
            "#fade".into(),
            SurfaceTransition {
                duration: 0.2,
                easing: Default::default(),
                from: None,
                exit: exit.map(SurfaceValue::Number),
            },
        )]),
    });
    content
}

/// Opens `content` with captured pointer input and samples its transitions as a painted
/// frame would.
fn open_painted(ui: &mut ModUi, content: &Panel) {
    ui.set_mod_panel(PanelUiOwner::Environment(0), Some(content))
        .unwrap();
    ui.set_mod_panel_open(true);
    let current = ui.mod_panel.as_mut().unwrap();
    let surface = current.panel.surface.as_ref().unwrap();
    current.motion.transitions.advance(surface, 1.0, true);
    current.view.scroll.insert("list".into(), 40.0);
    current.pointer = Some([10.0, 10.0]);
    current.held = true;
    current.drag = Some(1);
}

/// Closing a painted surface with exit values releases input at once but keeps it drawing,
/// repeated closes continue the exit, and reopening resumes from the displayed values.
#[test]
fn closing_with_exit_values_releases_input_and_keeps_the_surface_drawing() {
    let mut ui = ModUi::default();
    open_painted(&mut ui, &exiting_panel(Some(0.0)));
    ui.set_mod_panel_open(false);
    for _ in 0..3 {
        assert!(!ui.mod_panel_open());
        assert!(ui.mod_panel_painting());
        assert!(ui.mod_panel_motion_active());
        let exiting = ui.mod_panel.as_ref().unwrap();
        assert_eq!(
            (exiting.drag, exiting.pointer, exiting.held),
            (None, None, false)
        );
        assert_eq!(exiting.view.scroll_offset("list"), 40.0);
        ui.set_mod_panel_open(false);
    }
    assert!(ui.mod_panel_events([10.0, 10.0], true, true).is_empty());
    assert!(!ui.mod_panel_open());

    ui.set_mod_panel_open(true);
    assert!(ui.mod_panel_open());
    let reopened = ui.mod_panel.as_ref().unwrap();
    assert!(!reopened.exiting());
    assert_eq!(reopened.view.scroll_offset("list"), 40.0);

    ui.set_mod_panel_open(false);
    assert!(ui.mod_panel_painting());
    ui.configure_motion(false);
    ui.set_mod_panel_open(false);
    assert!(!ui.mod_panel_painting());
    assert!(!ui.mod_panel_motion_active());
    assert_eq!(
        ui.mod_panel.as_ref().unwrap().view.scroll_offset("list"),
        0.0
    );
}

/// Without exit values, before the first paint, with animations off or while the HUD editor
/// owns the surface, closing removes the panel at once.
#[test]
fn panels_without_an_exit_to_play_close_at_once() {
    let closed_at_once = |ui: &mut ModUi| {
        ui.set_mod_panel_open(false);
        let closed = ui.mod_panel.as_ref().unwrap();
        !ui.mod_panel_open()
            && !ui.mod_panel_painting()
            && !ui.mod_panel_motion_active()
            && closed.frame.is_none()
            && closed.view.scroll_offset("list") == 0.0
    };
    let mut ui = ModUi::default();
    open_painted(&mut ui, &exiting_panel(None));
    assert!(closed_at_once(&mut ui));

    let mut ui = ModUi::default();
    open_painted(&mut ui, &exiting_panel(Some(1.0)));
    assert!(closed_at_once(&mut ui));

    let mut ui = ModUi::default();
    ui.set_mod_panel(
        PanelUiOwner::Environment(0),
        Some(&exiting_panel(Some(0.0))),
    )
    .unwrap();
    ui.set_mod_panel_open(true);
    assert!(closed_at_once(&mut ui));

    let mut ui = ModUi::default();
    ui.configure_motion(false);
    open_painted(&mut ui, &exiting_panel(Some(0.0)));
    assert!(closed_at_once(&mut ui));

    let mut ui = ModUi::default();
    open_painted(&mut ui, &exiting_panel(Some(0.0)));
    ui.open_mod_hud_editor(&ui::mod_hud::Hud::default())
        .unwrap();
    assert!(closed_at_once(&mut ui));
    assert!(!ui.mod_hud_editor_open());
}

/// A binding update keeps an exit going; a route change ends it, so a closed panel never
/// shows a route it did not display.
#[test]
fn route_changes_end_an_exit_and_binding_updates_continue_it() {
    use ui::mod_panel::SurfaceValue;

    let mut ui = ModUi::default();
    let content = exiting_panel(Some(0.0));
    open_painted(&mut ui, &content);
    ui.set_mod_panel_open(false);
    let mut updated = content.clone();
    updated
        .surface
        .as_mut()
        .unwrap()
        .bindings
        .insert("#fade".into(), SurfaceValue::Number(0.5));
    ui.set_mod_panel(PanelUiOwner::Environment(0), Some(&updated))
        .unwrap();
    assert!(ui.mod_panel_painting());

    let mut rerouted = updated.clone();
    rerouted.surface.as_mut().unwrap().screen = "extension.other".into();
    rerouted.surface.as_mut().unwrap().document = serde_json::json!({
        "namespace": "extension",
        "other": {"type": "screen", "size": [80, 60], "controls": []}
    })
    .to_string();
    ui.set_mod_panel(PanelUiOwner::Environment(0), Some(&rerouted))
        .unwrap();
    assert!(!ui.mod_panel_painting());
    assert!(!ui.mod_panel_motion_active());
}

/// Packet publication checks the selected route's schema and keeps its current visibility.
#[test]
fn packet_prepared_updates_accept_route_schemas_and_reject_changed_previews() {
    let ([initial, _], _) = prepared_panels("Packet routes");
    let mut options = initial.clone();
    options.controls = vec![Control::Button {
        id: "back".into(),
        label: "Back".into(),
    }];
    options.surface.as_mut().unwrap().previews.insert(
        "sample".into(),
        ui::mod_panel::preview::ControlPreview {
            reference: "fixture.first".into(),
            ..Default::default()
        },
    );
    let bundle = Arc::new(ui::mod_panel::PanelBundle {
        panel: initial,
        surfaces: std::collections::BTreeMap::from([(
            "options".into(),
            options.surface.clone().unwrap(),
        )]),
        control_schemas: std::collections::BTreeMap::from([(
            "options".into(),
            options.controls.clone(),
        )]),
    });
    let prepared = prepared::PreparedPanelUi::prepare(bundle).unwrap();
    let owner = prepared::PanelUiOwner::PacketSession(1);
    let mut ui = ModUi::default();
    ui.set_prepared_mod_panel(owner, Arc::clone(&prepared), &options)
        .unwrap();
    ui.set_mod_panel_open(true);
    let mut changed = options.clone();
    changed
        .surface
        .as_mut()
        .unwrap()
        .bindings
        .insert("#value".into(), ui::mod_panel::SurfaceValue::Number(2.));
    ui.set_prepared_mod_panel(owner, Arc::clone(&prepared), &changed)
        .unwrap();
    assert!(ui.mod_panel_open());
    changed
        .surface
        .as_mut()
        .unwrap()
        .previews
        .get_mut("sample")
        .unwrap()
        .reference = "fixture.second".into();
    assert!(
        ui.set_prepared_mod_panel(owner, prepared, &changed)
            .is_err()
    );
    assert!(ui.mod_panel_open());
    assert_eq!(
        ui.mod_panel.as_ref().unwrap().panel.surface,
        options.surface.map(|mut surface| {
            surface
                .bindings
                .insert("#value".into(), ui::mod_panel::SurfaceValue::Number(2.));
            surface
        })
    );
}

/// Host-authored fallback controls retain only their admitted schema between packet updates.
#[test]
fn packet_prepared_fallback_accepts_values_and_rejects_new_controls() {
    let initial = panel();
    let prepared = prepared::PreparedPanelUi::prepare(Arc::new(ui::mod_panel::PanelBundle {
        panel: initial.clone(),
        surfaces: Default::default(),
        control_schemas: Default::default(),
    }))
    .unwrap();
    let owner = prepared::PanelUiOwner::PacketSession(1);
    let mut ui = ModUi::default();
    ui.set_prepared_mod_panel(owner, Arc::clone(&prepared), &initial)
        .unwrap();
    let mut changed = initial.clone();
    if let Control::Toggle { value, .. } = &mut changed.controls[0] {
        *value = !*value;
    }
    ui.set_prepared_mod_panel(owner, Arc::clone(&prepared), &changed)
        .unwrap();
    changed.controls.push(Control::Button {
        id: "new".into(),
        label: "New".into(),
    });
    assert!(
        ui.set_prepared_mod_panel(owner, prepared, &changed)
            .is_err()
    );
    assert_eq!(
        ui.mod_panel.as_ref().unwrap().panel.controls.len(),
        initial.controls.len()
    );
}

/// Visibility is live panel metadata; changing it must preserve a captured gesture.
#[test]
fn gameplay_hud_policy_updates_without_replacing_panel_or_capture() {
    let mut ui = crate::ModUi::default();
    let mut settings = panel();
    let owner = PanelUiOwner::Environment(0);
    ui.set_mod_panel(owner, Some(&settings)).unwrap();
    ui.set_mod_panel_open(true);
    ui.mod_panel.as_mut().unwrap().drag = Some(1);
    for visible in [true, false, true] {
        settings.show_gameplay_hud = visible;
        ui.set_mod_panel(owner, Some(&settings)).unwrap();
        assert_eq!(ui.mod_panel_shows_gameplay_hud(), visible);
        assert!(ui.mod_panel_open());
        assert_eq!(ui.mod_panel.as_ref().unwrap().drag, Some(1));
    }
    ui.set_mod_panel_open(false);
    assert!(!ui.mod_panel_shows_gameplay_hud());
    settings.show_gameplay_hud = false;
    ui.set_mod_panel(PanelUiOwner::Environment(1), Some(&settings))
        .unwrap();
    ui.set_mod_panel_open(true);
    assert!(
        !ui.mod_panel_shows_gameplay_hud(),
        "new owner does not inherit policy"
    );
}

/// Packet publication rejects unadmitted documents and schemas.
#[test]
fn packet_prepared_updates_accept_route_schemas_and_reject_changed_documents() {
    let ([initial, _], _) = prepared_panels("Packet routes");
    let mut options = initial.clone();
    options.controls = vec![Control::Button {
        id: "back".into(),
        label: "Back".into(),
    }];
    let bundle = Arc::new(ui::mod_panel::PanelBundle {
        panel: initial,
        surfaces: std::collections::BTreeMap::from([(
            "options".into(),
            options.surface.clone().unwrap(),
        )]),
        control_schemas: std::collections::BTreeMap::from([(
            "options".into(),
            options.controls.clone(),
        )]),
    });
    let prepared = prepared::PreparedPanelUi::prepare(bundle).unwrap();
    let owner = prepared::PanelUiOwner::PacketSession(1);
    let mut ui = ModUi::default();
    ui.set_prepared_mod_panel(owner, Arc::clone(&prepared), &options)
        .unwrap();
    ui.set_mod_panel_open(true);
    let mut changed = options.clone();
    changed
        .surface
        .as_mut()
        .unwrap()
        .bindings
        .insert("#value".into(), ui::mod_panel::SurfaceValue::Number(2.));
    ui.set_prepared_mod_panel(owner, Arc::clone(&prepared), &changed)
        .unwrap();
    assert!(ui.mod_panel_open());
    let accepted = ui.mod_panel.as_ref().unwrap().panel.clone();
    let mut document = changed.clone();
    document.surface.as_mut().unwrap().document = serde_json::json!({
        "namespace":"extension", "screen":{"type":"screen","size":[80,60],"controls":[]}
    })
    .to_string();
    assert!(
        ui.set_prepared_mod_panel(owner, Arc::clone(&prepared), &document)
            .is_err()
    );
    let mut schema = changed;
    schema.controls[0] = Control::Button {
        id: "unregistered".into(),
        label: "Back".into(),
    };
    assert!(ui.set_prepared_mod_panel(owner, prepared, &schema).is_err());
    assert!(ui.mod_panel_open());
    assert_eq!(ui.mod_panel.as_ref().unwrap().panel, accepted);
}

/// Samples the displayed surface values through the same transition state used by painting.
fn transition_values(ui: &mut ModUi, now: f64) -> json_ui::DataSource {
    let current = ui.mod_panel.as_mut().unwrap();
    current
        .motion
        .transitions
        .advance(current.panel.surface.as_ref().unwrap(), now, true);
    let mut values = json_ui::DataSource::new();
    current.motion.transitions.write(&mut values);
    values
}

/// Navigation preserves shared opening progress, including the backdrop, while owner changes
/// start their own entrance and a genuine close still plays the exit.
#[test]
fn open_route_changes_preserve_surface_transition_progress() {
    use ui::mod_panel::SurfaceValue;

    let mut content = exiting_panel(Some(0.0));
    let surface = content.surface.as_mut().unwrap();
    let fade = surface.transitions.get_mut("#fade").unwrap();
    fade.from = Some(SurfaceValue::Number(0.0));
    fade.duration = 1.0;
    fade.easing = Default::default();
    let mut options = content.clone();
    options.surface.as_mut().unwrap().screen = "extension.options".into();
    options.surface.as_mut().unwrap().document = serde_json::json!({
        "namespace": "extension",
        "options": {"type": "screen", "size": [80, 60], "controls": []}
    })
    .to_string();
    let mut ui = ModUi::default();
    ui.set_mod_panel(PanelUiOwner::Environment(0), Some(&content))
        .unwrap();
    ui.set_mod_panel_open(true);
    let values = |fade| {
        let mut data = json_ui::DataSource::new();
        data.set_global("#fade", json_ui::Scalar::Num(fade));
        data
    };
    assert_eq!(transition_values(&mut ui, 1.0), values(0.0));
    assert_eq!(transition_values(&mut ui, 1.5), values(0.5));
    ui.set_mod_panel(PanelUiOwner::Environment(0), Some(&options))
        .unwrap();
    assert!(ui.mod_panel_open());
    assert_eq!(transition_values(&mut ui, 1.5), values(0.5));
    assert_eq!(transition_values(&mut ui, 2.0), values(1.0));
    ui.set_mod_panel(PanelUiOwner::Environment(0), Some(&content))
        .unwrap();
    assert_eq!(transition_values(&mut ui, 2.0), values(1.0));
    ui.set_mod_panel_open(false);
    transition_values(&mut ui, 2.0);
    assert_eq!(transition_values(&mut ui, 2.5), values(0.5));
    ui.set_mod_panel_open(true);
    assert_eq!(transition_values(&mut ui, 2.5), values(0.5));
    assert_eq!(transition_values(&mut ui, 3.5), values(1.0));
    ui.set_mod_panel(PanelUiOwner::Environment(1), Some(&options))
        .unwrap();
    assert_eq!(transition_values(&mut ui, 3.5), values(0.0));
    let mut builtin = content.clone();
    builtin.surface = None;
    ui.set_mod_panel(PanelUiOwner::Environment(1), Some(&builtin))
        .unwrap();
    assert!(ui.mod_panel_open());
    assert!(!ui.mod_panel_motion_active());
}
