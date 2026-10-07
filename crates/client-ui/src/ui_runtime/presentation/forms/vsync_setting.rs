//! Video-section VSync toggle; retail vanilla keeps VSync out of its menus.

use json_ui::{Catalog, DataSource, Scalar};

use crate::menu::MenuView;

/// Places the toggle beside the frame-rate limit in the advanced video options.
pub(super) fn install(catalog: &mut Catalog) {
    catalog.overlay_text("ui/cinnabar_vsync.json", OVERLAY);
}

/// Shows a launch-flag override as the effective, locked state.
pub(super) fn bind(view: &MenuView, data: &mut DataSource) {
    if let Some(vsync) = view.vsync_override {
        data.set_global("#vsync", Scalar::Bool(vsync));
        data.set_global("#vsync_enabled", Scalar::Bool(false));
    }
}

const OVERLAY: &str = r##"{
  "namespace": "general_section",
  "advanced_graphics_options_section": {
    "modifications": [{
      "array_name": "controls",
      "operation": "insert_after",
      "control_name": "max_framerate_slider",
      "value": [{
        "vsync@settings_common.option_toggle": {
          "$option_label": "options.vsync",
          "$option_binding_name": "#vsync",
          "$option_enabled_binding_name": "#vsync_enabled",
          "$toggle_name": "vsync"
        }
      }]
    }]
  }
}"##;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_runtime::presentation::forms::pack_harness;

    fn has_text(control: &json_ui::ResolvedControl, text: &str) -> bool {
        control
            .properties
            .get("text")
            .and_then(|value| value.as_str())
            == Some(text)
            || control.children.iter().any(|child| has_text(child, text))
    }

    /// The toggle follows the frame-rate limit and is captioned by the vanilla lang key.
    #[test]
    fn video_options_show_vsync_with_the_vanilla_label_key() {
        let Some(carrier) = pack_harness::carrier() else {
            eprintln!(
                "skipping video_options_show_vsync_with_the_vanilla_label_key: fixture unavailable; requires installed UI carrier (make assets)"
            );
            return;
        };
        let files = carrier.ui_files();
        let mut catalog =
            Catalog::from_files(files.iter().map(|file| (&*file.path, &*file.bytes))).unwrap();
        install(&mut catalog);
        let section = json_ui::resolve(
            &catalog,
            "general_section.advanced_graphics_options_section",
            &json_ui::Context::retail(false),
        )
        .control
        .expect("advanced video options");
        let position = |name: &str| section.children.iter().position(|child| child.name == name);
        let vsync = position("vsync").expect("vsync toggle");
        assert_eq!(
            Some(vsync),
            position("max_framerate_slider").map(|at| at + 1)
        );
        assert!(has_text(&section.children[vsync], "options.vsync"));
    }

    #[test]
    fn launch_override_shows_the_effective_state_locked() {
        let mut view = MenuView::new(true, "Steve".into());
        view.vsync_override = Some(false);
        let mut data = DataSource::default();
        data.set_global("#vsync", Scalar::Bool(true));
        bind(&view, &mut data);
        let mut locked = DataSource::default();
        locked.set_global("#vsync", Scalar::Bool(false));
        locked.set_global("#vsync_enabled", Scalar::Bool(false));
        assert_eq!(data, locked);
    }
}
