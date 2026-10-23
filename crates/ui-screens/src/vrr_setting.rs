//! Variable-refresh selector in the JSON-UI Video settings.

use json_ui::Catalog;
use launcher::menu::settings_options::VRR_OPTION;

/// Adds the persisted variable-refresh choice to the advanced Video controls.
pub fn install(catalog: &mut Catalog) {
    super::settings_controls::install_video_dropdown(catalog, VRR_OPTION.id, "vsync");
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        launcher::menu::settings_options::{VRR_CHOICES, VRR_OPTION},
    };

    /// Finds a caption anywhere in a resolved control.
    fn has_text(control: &json_ui::ResolvedControl, text: &str) -> bool {
        control
            .properties
            .get("text")
            .and_then(|value| value.as_str())
            == Some(text)
            || control.children.iter().any(|child| has_text(child, text))
    }

    #[test]
    fn video_options_offer_vrr_and_all_presets() {
        let Some(carrier) = ui_engine::test_support::carrier() else {
            eprintln!(
                "skipping video_options_offer_vrr_and_all_presets: missing UI carrier; make assets"
            );
            return;
        };
        let files = carrier.ui_files();
        let mut catalog =
            Catalog::from_files(files.iter().map(|file| (&*file.path, &*file.bytes))).unwrap();
        super::super::vsync_setting::install(&mut catalog);
        install(&mut catalog);
        let context = json_ui::Context::retail(false);
        let section = json_ui::resolve(
            &catalog,
            "general_section.advanced_graphics_options_section",
            &context,
        )
        .control
        .expect("advanced video options");
        assert!(has_text(&section, VRR_OPTION.label));
        let choices = json_ui::resolve(
            &catalog,
            &format!("general_section.{}_dropdown_content", VRR_OPTION.name),
            &context,
        )
        .control
        .expect("variable-refresh choices");
        for choice in VRR_CHOICES {
            assert!(has_text(&choices, choice.label));
        }
    }
}
