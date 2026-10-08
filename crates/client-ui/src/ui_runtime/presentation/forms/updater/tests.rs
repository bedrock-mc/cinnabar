use super::*;
use json_ui::{Catalog, LayoutEnv, TextMeasure, TextureMeta, TextureSource, ViewState};

struct Measures;
impl TextMeasure for Measures {
    /// Uses predictable fixture metrics without a font or installed assets.
    fn extent(&self, text: &str) -> [f64; 2] {
        [text.len() as f64 * 5.0, 9.0]
    }
}
impl TextureSource for Measures {
    /// Fixture controls need no texture payloads.
    fn texture(&self, _: &str) -> Option<TextureMeta> {
        None
    }
}

/// Supplies small original controls to exercise the real extension and binding engine.
fn catalog() -> Catalog {
    let mut catalog = Catalog::default();
    catalog.overlay_text("ui/start_fixture.json", r#"{
      "namespace":"start",
      "start_screen":{"type":"panel","controls":[{"content@$screen_content":{}}]},
      "start_screen_content":{"type":"panel","controls":[{"original@common_buttons.light_text_button":{
        "size":[50,20],"anchor_from":"top_left","anchor_to":"top_left","$pressed_button_name":"button.original"
      }}]}
    }"#);
    catalog.overlay_text("ui/buttons_fixture.json", r#"{
      "namespace":"common_buttons","light_text_button":{"type":"button",
        "button_mappings":[{"from_button_id":"button.menu_select","to_button_id":"$pressed_button_name","mapping_type":"pressed"}]
      }
    }"#);
    catalog.overlay_text(
        "ui/settings_fixture.json",
        r#"{
      "namespace":"general_section","general_tab_section":{"type":"stack_panel","controls":[
        {"auto_update_mode_dropdown":{"type":"panel"}},
        {"auto_update_enabled_toggle":{"type":"panel","ignored":true}}
      ]}
    }"#,
    );
    catalog.overlay_text("ui/settings_common_fixture.json", r##"{
      "namespace":"settings_common","option_toggle":{"type":"toggle","size":[150,20],
        "toggle_name":"$toggle_name","bindings":[{"binding_name":"$option_binding_name","binding_name_override":"#toggle_state"}]
      }
    }"##);
    extend_catalog(&mut catalog);
    assert!(
        catalog.diagnostics().is_empty(),
        "{:?}",
        catalog.diagnostics()
    );
    catalog
}

/// Runs the home extension through resolution, binding, layout and hit collection.
fn render(view: &MenuView) -> json_ui::FormRender {
    let catalog = catalog();
    let mut data = DataSource::new();
    data.set_strict(true);
    let context = bind_home(view, &mut data, Context::default());
    json_ui::render_screen(
        "start.start_screen",
        &catalog,
        &context,
        &data,
        [360.0, 240.0],
        &LayoutEnv {
            text: &Measures,
            textures: &Measures,
        },
        &ViewState::default(),
    )
    .unwrap()
}

#[test]
fn ready_notice_reserves_space_and_routes_restart_and_notes() {
    let mut view = MenuView::new(true, "Fixture".into());
    view.update.message = "An update is ready".into();
    view.update.ready = true;
    view.update.notes = true;
    let rendered = render(&view);
    let actions: Vec<_> = rendered
        .hits
        .iter()
        .filter_map(|hit| action(&view, hit))
        .collect();
    assert_eq!(
        actions,
        [MenuAction::UpdateRestart, MenuAction::UpdateNotes]
    );
    let original = rendered
        .hits
        .iter()
        .find(|hit| hit.pressed.as_deref() == Some("button.original"))
        .unwrap();
    for hit in rendered
        .hits
        .iter()
        .filter(|hit| action(&view, hit).is_some())
    {
        assert!(
            hit.rect.y + hit.rect.h <= original.rect.y,
            "notice overlaps original menu"
        );
    }
}

#[test]
fn progress_has_no_install_action_and_errors_offer_retry() {
    let mut view = MenuView::new(true, "Fixture".into());
    view.update.message = "Downloading update: 25%".into();
    assert!(
        render(&view)
            .hits
            .iter()
            .all(|hit| action(&view, hit).is_none())
    );
    view.update.message = "Download failed".into();
    view.update.retry = true;
    let rendered = render(&view);
    let actions: Vec<_> = rendered
        .hits
        .iter()
        .filter_map(|hit| action(&view, hit))
        .collect();
    assert_eq!(actions, [MenuAction::UpdateRetry]);
}

#[test]
fn quiet_home_has_no_notice_and_world_sessions_cannot_restart() {
    let mut view = MenuView::new(true, "Fixture".into());
    let quiet = render(&view);
    assert_eq!(quiet.hits.len(), 1);
    assert_eq!(quiet.hits[0].rect.y, 0.0);
    view.update.message = "Ready".into();
    view.update.ready = true;
    let ready = render(&view);
    view.over_world = true;
    assert!(ready.hits.iter().all(|hit| action(&view, hit).is_none()));
    assert_eq!(render(&view).hits.len(), 1);
    view.screen = MenuScreen::Pause;
    assert!(ready.hits.iter().all(|hit| action(&view, hit).is_none()));
}

#[test]
fn settings_toggle_exposes_effective_preference_and_uses_update_action() {
    let catalog = catalog();
    let context = Context::default();
    let mut view = MenuView::new(true, "Fixture".into());
    view.screen = MenuScreen::Settings;
    for enabled in [true, false] {
        view.update.enabled = enabled;
        let mut data = DataSource::new();
        bind_settings(&view, &mut data);
        let resolved = json_ui::resolve(&catalog, "general_section.general_tab_section", &context)
            .control
            .unwrap();
        let bound = json_ui::bind(
            &resolved,
            &data,
            &json_ui::CatalogLibrary {
                catalog: &catalog,
                context: &context,
            },
        );
        let rendered = json_ui::render_bound(
            bound,
            [360.0, 240.0],
            &LayoutEnv {
                text: &Measures,
                textures: &Measures,
            },
            &ViewState::default(),
        );
        assert_eq!(rendered.hits.len(), 1);
        assert_eq!(rendered.hits[0].checked, Some(enabled));
        assert_eq!(
            action(&view, &rendered.hits[0]),
            Some(MenuAction::UpdateToggle)
        );
    }
}
