//! Video selector between Java 1.7 and vanilla Bedrock player animations.

use json_ui::Catalog;
use serde_json::json;

use launcher::menu::settings_options::{ANIMATION_CHOICES, ANIMATIONS_OPTION};

/// Uses the Graphics mode dropdown and radio templates after View Bobbing.
pub(super) fn install(catalog: &mut Catalog) {
    let name = ANIMATIONS_OPTION.name;
    let choices = ANIMATION_CHOICES
        .iter()
        .map(|choice| {
            json!({
                "@settings_common.radio_with_label": {
                    "$toggle_state_binding_name": format!("#{}", choice.name),
                    "$radio_label_text": choice.label
                }
            })
        })
        .collect::<Vec<_>>();
    let overlay = json!({
        "namespace": "general_section",
        format!("{name}_dropdown_content@settings_common.option_radio_dropdown_group"): {
            "$radio_buttons": choices
        },
        "video_section": {
            "modifications": [{
                "array_name": "controls",
                "operation": "insert_after",
                "control_name": "view_bobbing_toggle",
                "value": [{
                    format!("{name}@settings_common.option_dropdown"): {
                        "$option_label": ANIMATIONS_OPTION.label,
                        "$dropdown_content": format!("general_section.{name}_dropdown_content"),
                        "$dropdown_area": "content_area",
                        "$dropdown_name": format!("{name}_dropdown"),
                        "$option_enabled_binding_name": format!("#{name}_dropdown_enabled"),
                        "$options_dropdown_toggle_label_binding": format!("#{name}_dropdown_toggle_label")
                    }
                }]
            }]
        }
    });
    catalog.overlay_text("ui/cinnabar_java_animations.json", &overlay.to_string());
}

#[cfg(test)]
mod tests {
    use crate::ui_runtime::presentation::forms::pack_harness;
    use {
        super::*,
        launcher::menu::settings_options::{ANIMATION_CHOICES, ANIMATIONS_OPTION},
    };

    /// Searches labels throughout an inherited control.
    fn has_text(control: &json_ui::ResolvedControl, text: &str) -> bool {
        control
            .properties
            .get("text")
            .and_then(|value| value.as_str())
            == Some(text)
            || control.children.iter().any(|child| has_text(child, text))
    }

    /// The selector follows View Bobbing and carries its registry caption.
    #[test]
    fn video_options_show_animations_after_view_bobbing() {
        let Some(carrier) = pack_harness::carrier() else {
            eprintln!(
                "skipping video_options_show_animations_after_view_bobbing: fixture unavailable; requires installed UI carrier (make assets)"
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
        .expect("video options");
        let position = |name: &str| section.children.iter().position(|child| child.name == name);
        let selector = position(ANIMATIONS_OPTION.name).expect("animations selector");
        assert_eq!(
            Some(selector),
            position("view_bobbing_toggle").map(|at| at + 1)
        );
        assert!(has_text(
            &section.children[selector],
            ANIMATIONS_OPTION.label
        ));
        let choices = json_ui::resolve(
            &catalog,
            &format!(
                "general_section.{}_dropdown_content",
                ANIMATIONS_OPTION.name
            ),
            &json_ui::Context::retail(false),
        )
        .control
        .expect("animation choices");
        for choice in ANIMATION_CHOICES {
            assert!(has_text(&choices, choice.label));
        }
    }
}
