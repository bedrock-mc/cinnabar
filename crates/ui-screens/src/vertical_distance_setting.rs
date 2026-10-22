//! Optional vertical terrain distance in the in-game Video screen.

use json_ui::Catalog;
use launcher::menu::settings_options::SettingId;
use serde_json::json;

/// Reuses the installed slider beside horizontal render distance and its shared bindings.
pub fn install(catalog: &mut Catalog) {
    let option = SettingId::VerticalRenderDistance.definition();
    let name = option.name;
    catalog.overlay_text(
        "ui/cinnabar_vertical_distance.json",
        &json!({
            "namespace": "general_section",
            "video_section": {"modifications": [{
                "array_name": "controls",
                "operation": "insert_after",
                "control_name": "render_distance_slider",
                "value": [{format!("{name}_slider@general_section.video_menu_customization_option_slider"): {
                    "$option_label": format!("#{name}_slider_label"),
                    "$slider_name": name,
                    "$slider_value_binding_name": format!("#{name}"),
                    "$slider_tts_text_value": format!("#{name}_text_value"),
                    "$option_enabled_binding_name": format!("#{name}_enabled"),
                    "$slider_steps_binding_name": format!("#{name}_steps"),
                    "$label_bindings": [{"binding_name": "$option_label"}]
                }}]
            }]}
        }).to_string(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use json_ui::{DataSource, ResolvedControl};
    use launcher::menu::settings_options::SettingsOptions;

    /// Searches visible text without depending on the installed slider's layout hierarchy.
    fn has_text(control: &ResolvedControl, text: &str) -> bool {
        control
            .properties
            .get("text")
            .and_then(|value| value.as_str())
            == Some(text)
            || control.children.iter().any(|child| has_text(child, text))
    }

    #[test]
    #[expect(clippy::print_stderr, reason = "names the missing optional UI fixture")]
    fn native_slider_displays_the_saved_radius_and_unlimited() {
        let Some(carrier) = ui_engine::test_support::carrier() else {
            eprintln!(
                "skipping native_slider_displays_the_saved_radius_and_unlimited: missing UI carrier; make assets"
            );
            return;
        };
        let files = carrier.ui_files();
        let mut catalog =
            Catalog::from_files(files.iter().map(|file| (&*file.path, &*file.bytes))).unwrap();
        install(&mut catalog);
        let root = json_ui::resolve(
            &catalog,
            "general_section.video_section",
            &json_ui::Context::retail(false),
        )
        .control
        .expect("installed Video controls");
        let mut settings = SettingsOptions::default();
        for (value, shown) in [
            (
                render_api::terrain_distance::VERTICAL_DISTANCE_UNLIMITED as i32,
                "Unlimited",
            ),
            (4, "4 chunks"),
        ] {
            settings.set(SettingId::VerticalRenderDistance.index(), value);
            let mut data = DataSource::default();
            super::super::settings_controls::bind_values(&settings, None, &mut data, &|key| {
                match key {
                    "options.framerateLimit.max" => "Unlimited",
                    "options.renderDistanceFormat" => "%s chunks",
                    _ => key,
                }
                .to_owned()
            });
            let bound = json_ui::bind(&root, &data, &json_ui::EmptyLibrary);
            assert!(has_text(
                &bound,
                &format!("Vertical Render Distance: {shown}")
            ));
        }
    }
}
