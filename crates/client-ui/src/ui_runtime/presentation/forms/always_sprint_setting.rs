//! Keyboard/mouse extension using the installed option toggle template.

use json_ui::Catalog;

pub(super) fn install(catalog: &mut Catalog) {
    catalog.overlay_text("ui/cinnabar_always_sprint.json", OVERLAY);
}

const OVERLAY: &str = r##"{
  "namespace": "controls_section",
  "keyboard_and_mouse_section": {
    "modifications": [{
      "array_name": "controls",
      "operation": "insert_after",
      "control_name": "option_toggle_1",
      "value": [{
        "always_sprint@settings_common.option_toggle": {
          "$option_label": "Always Sprint",
          "$option_binding_name": "#always_sprint",
          "$option_enabled_binding_name": "#always_sprint_enabled",
          "$toggle_name": "always_sprint",
          "$focus_override_right": "FOCUS_OVERRIDE_STOP"
        }
      }]
    }]
  }
}"##;
