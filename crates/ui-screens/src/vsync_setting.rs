//! Video-section VSync choices.

use json_ui::{Catalog, DataSource, Scalar};

use launcher::menu::MenuView;

/// Places the registry's VSync toggle beside the frame-rate slider.
pub fn install(catalog: &mut Catalog) {
    let option = launcher::menu::settings_options::VSYNC_OPTION;
    let name = option.name;
    catalog.overlay_text(
        "ui/cinnabar_vsync.json",
        &serde_json::json!({
            "namespace": "general_section",
            "advanced_graphics_options_section": {"modifications": [{
                "array_name": "controls", "operation": "insert_after",
                "control_name": "max_framerate_slider",
                "value": [{format!("{name}@settings_common.option_toggle"): {
                    "$option_label": option.label,
                    "$option_binding_name": format!("#{name}"),
                    "$option_enabled_binding_name": format!("#{name}_enabled"),
                    "$toggle_name": name,
                    "$focus_override_right": "FOCUS_OVERRIDE_STOP"
                }}]
            }]}
        })
        .to_string(),
    );
}

/// Shows a launch-flag override as the effective, locked toggle state.
pub(super) fn bind(view: &MenuView, data: &mut DataSource) {
    if let Some(vsync) = view.vsync_override {
        let name = launcher::menu::settings_options::VSYNC_OPTION.name;
        data.set_global(format!("#{name}_enabled"), Scalar::Bool(false));
        data.set_global(format!("#{name}"), Scalar::Bool(vsync));
    }
}

#[cfg(test)]
mod tests {
    use {super::*, launcher::menu::MenuView};

    /// Finds a caption anywhere in a resolved control.
    fn has_text(control: &json_ui::ResolvedControl, text: &str) -> bool {
        control
            .properties
            .get("text")
            .and_then(|value| value.as_str())
            == Some(text)
            || control.children.iter().any(|child| has_text(child, text))
    }

    /// The toggle follows the frame-rate limit and uses the shared caption.
    #[test]
    fn video_options_show_vsync_with_the_vanilla_label_key() {
        let Some(carrier) = ui_engine::test_support::carrier() else {
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
        assert!(has_text(
            &section.children[vsync],
            launcher::menu::settings_options::VSYNC_OPTION.label
        ));
    }

    #[test]
    fn launch_override_shows_the_effective_state_locked() {
        let name = launcher::menu::settings_options::VSYNC_OPTION.name;
        for on in [false, true] {
            let mut view = MenuView::new(true, "Steve".into());
            view.vsync_override = Some(on);
            let mut data = DataSource::default();
            bind(&view, &mut data);
            let mut locked = DataSource::default();
            locked.set_global(format!("#{name}_enabled"), Scalar::Bool(false));
            locked.set_global(format!("#{name}"), Scalar::Bool(on));
            assert_eq!(data, locked);
        }
    }
}
