//! Video toggle for Discord Rich Presence, after the Animations selector.

use json_ui::Catalog;
use serde_json::json;

use crate::menu::settings_options::{ANIMATIONS_OPTION, DISCORD_PRESENCE_OPTION};

pub(super) fn install(catalog: &mut Catalog) {
    let name = DISCORD_PRESENCE_OPTION.name;
    let overlay = json!({
        "namespace": "general_section",
        "video_section": {
            "modifications": [{
                "array_name": "controls",
                "operation": "insert_after",
                "control_name": ANIMATIONS_OPTION.name,
                "value": [{
                    format!("{name}@settings_common.option_toggle"): {
                        "$option_label": DISCORD_PRESENCE_OPTION.label,
                        "$option_binding_name": format!("#{name}"),
                        "$option_enabled_binding_name": format!("#{name}_enabled"),
                        "$toggle_name": name,
                        "$focus_override_right": "FOCUS_OVERRIDE_STOP"
                    }
                }]
            }]
        }
    });
    catalog.overlay_text("ui/cinnabar_discord_presence.json", &overlay.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_runtime::presentation::forms::pack_harness;

    /// The toggle follows the Animations selector it is anchored to.
    #[test]
    fn video_options_show_discord_toggle_after_animations() {
        let Some(carrier) = pack_harness::carrier() else {
            eprintln!(
                "skipping video_options_show_discord_toggle_after_animations: fixture unavailable; requires installed UI carrier (make assets)"
            );
            return;
        };
        let files = carrier.ui_files();
        let mut catalog =
            Catalog::from_files(files.iter().map(|file| (&*file.path, &*file.bytes))).unwrap();
        super::super::java_animations_setting::install(&mut catalog);
        install(&mut catalog);
        let section = json_ui::resolve(
            &catalog,
            "general_section.video_section",
            &json_ui::Context::retail(false),
        )
        .control
        .expect("video options");
        let position = |name: &str| section.children.iter().position(|child| child.name == name);
        assert_eq!(
            position(DISCORD_PRESENCE_OPTION.name),
            position(ANIMATIONS_OPTION.name).map(|at| at + 1)
        );
    }
}
