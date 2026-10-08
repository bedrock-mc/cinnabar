//! The optional Video-section extension, separate from vanilla settings wiring.

use json_ui::{Catalog, DataSource, HitKind, HitRegion, Scalar};

use crate::menu::{MenuAction, MenuScreen, MenuView};

/// Add Enhanced controls using the existing JSON-UI settings templates.
pub(super) fn install(catalog: &mut Catalog) {
    if !render_model::ENHANCED_RENDERING_ENABLED {
        return;
    }
    catalog.overlay_text("ui/cinnabar_enhanced.json", OVERLAY);
}

/// Publish the extension toggle and the current quality choice.
pub(super) fn bind(view: &MenuView, data: &mut DataSource) {
    let enhanced =
        render_model::ENHANCED_RENDERING_ENABLED && view.render_mode == ui::RenderMode::Enhanced;
    data.set_global("#cinnabar_enhanced", Scalar::Bool(enhanced));
    data.set_global(
        "#cinnabar_enhanced_enabled",
        Scalar::Bool(render_model::ENHANCED_RENDERING_ENABLED),
    );
    data.set_global("#cinnabar_enhanced_quality_enabled", Scalar::Bool(enhanced));
    data.set_global(
        "#cinnabar_enhanced_quality_label",
        Scalar::Text(format!(
            "Enhanced quality: {}",
            view.enhanced_quality.label()
        )),
    );
}

/// Route the extension controls to their retained setting requests.
pub(super) fn action(view: &MenuView, region: &HitRegion) -> Option<MenuAction> {
    if !render_model::ENHANCED_RENDERING_ENABLED
        || view.screen != MenuScreen::Settings
        || !region.enabled
    {
        return None;
    }
    if region.kind == HitKind::Toggle && region.control_name.as_deref() == Some("cinnabar_enhanced")
    {
        return Some(MenuAction::ToggleRenderMode);
    }
    (view.render_mode == ui::RenderMode::Enhanced
        && region.pressed.as_deref() == Some("button.cinnabar_enhanced_quality"))
    .then_some(MenuAction::CycleEnhancedQuality)
}

const OVERLAY: &str = r##"{
  "namespace": "general_section",
  "video_section": {
    "modifications": [{
      "array_name": "controls",
      "operation": "insert_front",
      "value": [
        {
          "cinnabar_enhanced@settings_common.option_toggle": {
            "$option_label": "Enhanced rendering (Cinnabar extension)",
            "$option_binding_name": "#cinnabar_enhanced",
            "$option_enabled_binding_name": "#cinnabar_enhanced_enabled",
            "$toggle_name": "cinnabar_enhanced"
          }
        },
        {
          "cinnabar_enhanced_quality@settings_common.action_button": {
            "$button_text": "#cinnabar_enhanced_quality_label",
            "$button_text_binding_type": "global",
            "$pressed_button_name": "button.cinnabar_enhanced_quality",
            "bindings": [{
              "binding_name": "#cinnabar_enhanced_quality_enabled",
              "binding_name_override": "#enabled"
            }]
          }
        }
      ]
    }]
  }
}"##;

#[cfg(test)]
mod tests {
    use super::*;

    /// The video toggle appears only in builds with the Enhanced feature.
    #[test]
    fn enhanced_toggle_visibility_matches_build_feature() {
        let mut catalog = Catalog::default();
        catalog.overlay_text(
            "ui/general_section.json",
            r#"{"namespace":"general_section","video_section":{"type":"stack_panel","controls":[]}}"#,
        );
        catalog.overlay_text(
            "ui/settings_common.json",
            r#"{"namespace":"settings_common","option_toggle":{"type":"toggle"},"action_button":{"type":"button"}}"#,
        );
        install(&mut catalog);
        assert!(catalog.diagnostics().is_empty());
        let resolution = json_ui::resolve(
            &catalog,
            "general_section.video_section",
            &json_ui::Context::default(),
        );
        assert!(resolution.diagnostics.is_empty());
        let resolved = resolution.control.expect("video section");
        assert_eq!(
            resolved.children.is_empty(),
            !render_model::ENHANCED_RENDERING_ENABLED
        );
    }
}
