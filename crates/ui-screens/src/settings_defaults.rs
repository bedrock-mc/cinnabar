//! Disabled placeholders not covered by the persisted option registry.

use json_ui::{DataSource, Scalar};

/// Dropdowns: the name its bindings share, the default radio's binding and
/// label key.
const DROPDOWNS: &[(&str, &str, &str)] = &[
    (
        "split_screen",
        "#split_screen_radio_horizontal",
        "options.splitscreen.horizontal",
    ),
    (
        "ui_profile",
        "#ui_profile_radio_classic",
        "options.uiprofile.classic",
    ),
];

/// Sliders: the name its bindings share, its label key, the default position
/// (0..=1) and how the value reads.
const SLIDERS: &[(&str, &str, f64, &str)] = &[(
    "splitscreen_interface_opacity",
    "options.splitscreenInterfaceOpacity",
    1.0,
    "100%",
)];

/// Toggles on by default; every other toggle reads off.
const TOGGLES_ON: &[&str] = &["#splitscreen_ingame_player_names"];

/// Bind the vanilla defaults of the options the client does not back.
pub(super) fn bind(data: &mut DataSource, translate: &dyn Fn(&str) -> String) {
    for (name, radio, label) in DROPDOWNS {
        data.set_global(
            format!("#{name}_dropdown_toggle_label"),
            Scalar::Text(translate(label)),
        );
        data.set_global(*radio, Scalar::Bool(true));
    }
    for (name, label, position, value) in SLIDERS {
        data.set_global(format!("#{name}"), Scalar::Num(*position));
        data.set_global(
            format!("#{name}_slider_label"),
            // The label localizes again, where `%%` keeps one `%`.
            Scalar::Text(format!(
                "{}: {}",
                translate(label),
                value.replace('%', "%%")
            )),
        );
    }
    for toggle in TOGGLES_ON {
        data.set_global(*toggle, Scalar::Bool(true));
    }
}
