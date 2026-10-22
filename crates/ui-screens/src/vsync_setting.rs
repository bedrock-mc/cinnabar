//! Video-section VSync choices.

use json_ui::{Catalog, DataSource, Scalar};

use launcher::menu::MenuView;

/// Places the registry's VSync choices beside the frame-rate slider.
pub fn install(catalog: &mut Catalog) {
    super::settings_controls::install_video_dropdown(
        catalog,
        launcher::menu::settings_options::SettingId::Vsync,
        "max_framerate_slider",
    );
}

/// Shows a launch-flag override as the effective, locked dropdown state.
pub(super) fn bind(view: &MenuView, data: &mut DataSource) {
    if let Some(vsync) = view.vsync_override {
        let selected = render_api::VsyncMode::from(vsync);
        data.set_global("#vsync_dropdown_enabled", Scalar::Bool(false));
        data.set_global("#vsync_dropdown", Scalar::Bool(false));
        data.set_global(
            "#vsync_dropdown_toggle_label",
            Scalar::Text(selected.label().into()),
        );
        for choice in render_api::VsyncMode::ALL {
            data.set_global(
                format!("#{}", choice.binding()),
                Scalar::Bool(choice == selected),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use {super::*, launcher::menu::MenuView};

    fn has_text(control: &json_ui::ResolvedControl, text: &str) -> bool {
        control
            .properties
            .get("text")
            .and_then(|value| value.as_str())
            == Some(text)
            || control.children.iter().any(|child| has_text(child, text))
    }

    /// The selector follows the frame-rate limit and exposes every registered choice.
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
        assert!(has_text(&section.children[vsync], "options.vsync"));
        let content = json_ui::resolve(
            &catalog,
            "general_section.vsync_dropdown_content",
            &json_ui::Context::retail(false),
        )
        .control
        .expect("VSync choices");
        for choice in launcher::menu::settings_options::VSYNC_CHOICES {
            assert!(has_text(&content, choice.label));
        }
    }

    #[test]
    fn launch_override_shows_the_effective_state_locked() {
        let mut view = MenuView::new(true, "Steve".into());
        view.vsync_override = Some(false);
        let mut data = DataSource::default();
        bind(&view, &mut data);
        let mut locked = DataSource::default();
        locked.set_global("#vsync_dropdown_enabled", Scalar::Bool(false));
        locked.set_global("#vsync_dropdown", Scalar::Bool(false));
        locked.set_global(
            "#vsync_dropdown_toggle_label",
            Scalar::Text(render_api::VsyncMode::Off.label().into()),
        );
        for mode in render_api::VsyncMode::ALL {
            locked.set_global(
                format!("#{}", mode.binding()),
                Scalar::Bool(mode == render_api::VsyncMode::Off),
            );
        }
        assert_eq!(data, locked);
    }
}
