//! Optional crosshair presentation controls in Video settings.

use json_ui::Catalog;
use serde_json::json;

use crate::menu::settings_options::{INVERT_CROSSHAIR_OPTION, THIRD_PERSON_CROSSHAIR_OPTION};

/// Adds both crosshair toggles beside the HUD visibility control.
pub(super) fn install(catalog: &mut Catalog) {
    let controls = [THIRD_PERSON_CROSSHAIR_OPTION, INVERT_CROSSHAIR_OPTION].map(|option| {
        let name = option.name;
        json!({
            format!("{name}@settings_common.option_toggle"): {
                "$option_label": option.label,
                "$option_binding_name": format!("#{name}"),
                "$option_enabled_binding_name": format!("#{name}_enabled"),
                "$toggle_name": name
            }
        })
    });
    let overlay = json!({
        "namespace": "general_section",
        "video_section": {
            "modifications": [{
                "array_name": "controls",
                "operation": "insert_after",
                "control_name": "option_toggle_hidehud",
                "value": controls
            }]
        }
    });
    catalog.overlay_text("ui/cinnabar_crosshair.json", &overlay.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_runtime::presentation::forms::pack_harness;

    /// Searches inherited labels without depending on template nesting.
    fn has_text(control: &json_ui::ResolvedControl, text: &str) -> bool {
        control
            .properties
            .get("text")
            .and_then(|value| value.as_str())
            == Some(text)
            || control.children.iter().any(|child| has_text(child, text))
    }

    #[test]
    fn video_section_exposes_both_crosshair_preferences() {
        let Some(carrier) = pack_harness::carrier() else {
            eprintln!(
                "skipping video_section_exposes_both_crosshair_preferences: missing UI carrier (make assets)"
            );
            return;
        };
        let files = carrier.ui_files();
        let mut catalog =
            Catalog::from_files(files.iter().map(|file| (&*file.path, &*file.bytes))).unwrap();
        install(&mut catalog);
        let section = json_ui::resolve(
            &catalog,
            "general_section.video_section",
            &json_ui::Context::retail(false),
        )
        .control
        .expect("video settings");
        for option in [THIRD_PERSON_CROSSHAIR_OPTION, INVERT_CROSSHAIR_OPTION] {
            assert!(
                has_text(&section, option.label),
                "{} is visible",
                option.name
            );
        }
    }
}
