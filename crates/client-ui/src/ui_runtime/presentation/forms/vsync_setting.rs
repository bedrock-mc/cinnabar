//! Video-section VSync and Allow Tearing toggles and the frame-rate limit caption; retail
//! vanilla keeps VSync out of its menus and has no Automatic limit.

use json_ui::{Catalog, DataSource, Scalar};
use render_api::FrameRateLimit;

use crate::menu::{
    MenuView,
    settings_options::{ALLOW_TEARING_OPTION, frame_rate_limit},
};

/// Caption for the Automatic stop, which vanilla's lang has no key for.
const AUTOMATIC_LABEL: &str = "Automatic";

/// The Max Framerate caption: Automatic, a cap, or vanilla's Unlimited.
pub(super) fn frame_rate_label(value: i32, translate: &dyn Fn(&str) -> String) -> String {
    match frame_rate_limit(value) {
        FrameRateLimit::Automatic => AUTOMATIC_LABEL.to_owned(),
        FrameRateLimit::Unlimited => translate("options.framerateLimit.max"),
        FrameRateLimit::Fixed(fps) => fps.to_string(),
    }
}

/// Allow Tearing's shown state and whether it is editable: only with VSync off, and a launch
/// override pins it along with VSync.
pub(super) fn tearing_toggle(view: &MenuView) -> (bool, bool) {
    match view.vsync_override {
        Some(vsync) => (!vsync, false),
        None => (
            view.settings_options.value(ALLOW_TEARING_OPTION.name) != 0,
            view.settings_options.value("vsync") == 0,
        ),
    }
}

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
    let (tearing, enabled) = tearing_toggle(view);
    data.set_global("#allow_tearing", Scalar::Bool(tearing));
    data.set_global("#allow_tearing_enabled", Scalar::Bool(enabled));
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
      },
      {
        "allow_tearing@settings_common.option_toggle": {
          "$option_label": "options.allowTearing",
          "$option_binding_name": "#allow_tearing",
          "$option_enabled_binding_name": "#allow_tearing_enabled",
          "$toggle_name": "allow_tearing"
        }
      }]
    }]
  }
}"##;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::settings_options::{
        FRAME_RATE_AUTOMATIC, FRAME_RATE_UNLIMITED, SETTINGS_OPTIONS,
    };
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
        locked.set_global("#allow_tearing", Scalar::Bool(true));
        locked.set_global("#allow_tearing_enabled", Scalar::Bool(false));
        assert_eq!(data, locked);
    }

    /// Allow Tearing only applies with VSync off, so it is editable only then.
    #[test]
    fn allow_tearing_is_editable_only_with_vsync_off() {
        let mut view = MenuView::new(true, "Steve".into());
        assert_eq!(tearing_toggle(&view), (false, false));
        let vsync = SETTINGS_OPTIONS
            .iter()
            .position(|option| option.name == "vsync")
            .unwrap();
        let tearing = SETTINGS_OPTIONS
            .iter()
            .position(|option| option.name == ALLOW_TEARING_OPTION.name)
            .unwrap();
        std::sync::Arc::make_mut(&mut view.settings_options).set(vsync, 0);
        assert_eq!(tearing_toggle(&view), (false, true));
        std::sync::Arc::make_mut(&mut view.settings_options).set(tearing, 1);
        assert_eq!(tearing_toggle(&view), (true, true));
        view.vsync_override = Some(true);
        assert_eq!(tearing_toggle(&view), (false, false));
    }

    #[test]
    fn frame_rate_caption_names_automatic_caps_and_vanilla_unlimited() {
        let translate = |key: &str| format!("<{key}>");
        assert_eq!(
            frame_rate_label(FRAME_RATE_AUTOMATIC, &translate),
            "Automatic"
        );
        assert_eq!(frame_rate_label(144, &translate), "144");
        assert_eq!(
            frame_rate_label(FRAME_RATE_UNLIMITED, &translate),
            "<options.framerateLimit.max>"
        );
    }
}
