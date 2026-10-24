//! Player placement preferences the built-in HUD pack reads as context flags: the chat's edge
//! and whether the bottom stack sits at vanilla Bedrock's hotbar height.

use json_ui::{Catalog, Context};
use serde_json::json;

use launcher::menu::settings_options::{BEDROCK_HOTBAR_OPTION, SettingsOptions};

/// Where the built-in HUD and chat screen place their chat and hotbar.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HudPlacement {
    /// The chat at the top of the screen rather than above the hotbar.
    pub chat_top: bool,
    /// The hotbar, everything stacked on it, and the chat raised to vanilla Bedrock's hotbar
    /// height rather than flush with the bottom edge as in Java.
    pub bedrock_hotbar: bool,
}

impl HudPlacement {
    /// Every placement, for resolving the HUD before its first paint.
    pub const ALL: [Self; 4] = [
        Self::new(false, false),
        Self::new(true, false),
        Self::new(false, true),
        Self::new(true, true),
    ];

    const fn new(chat_top: bool, bedrock_hotbar: bool) -> Self {
        Self {
            chat_top,
            bedrock_hotbar,
        }
    }

    /// The placement `options` selects.
    pub fn from_settings(options: &SettingsOptions) -> Self {
        Self {
            chat_top: options.chat_at_top(),
            bedrock_hotbar: options.value(BEDROCK_HOTBAR_OPTION.id) != 0,
        }
    }

    /// `context` with this placement's pack flags.
    pub fn placed(self, context: Context) -> Context {
        crate::chat_position::placed(context, self.chat_top)
            .with_flag(BEDROCK_HOTBAR_OPTION.name, self.bedrock_hotbar)
    }
}

/// Adds the Bedrock hotbar toggle after the vanilla Video section's interface opacity slider.
pub fn install(catalog: &mut Catalog) {
    let name = BEDROCK_HOTBAR_OPTION.name;
    let overlay = json!({
        "namespace": "general_section",
        "video_section": {
            "modifications": [{
                "array_name": "controls",
                "operation": "insert_after",
                "control_name": "hud_opacity_slider",
                "value": [{
                    format!("{name}@settings_common.option_toggle"): {
                        "$option_label": BEDROCK_HOTBAR_OPTION.label,
                        "$option_binding_name": format!("#{name}"),
                        "$option_enabled_binding_name": format!("#{name}_enabled"),
                        "$toggle_name": name
                    }
                }]
            }]
        }
    });
    catalog.overlay_text("ui/cinnabar_bedrock_hotbar.json", &overlay.to_string());
}

/// A placed context kept until its base context or placement changes, so the retained screen
/// reading it compares it by pointer rather than variable by variable.
#[derive(Default)]
pub struct PlacedContext {
    built: Option<(Context, HudPlacement, Context)>,
}

impl PlacedContext {
    /// `place(base, placement)`, reused while `base` and `placement` match the last call's. Each
    /// holder passes the same `place` on every call.
    pub fn get(
        &mut self,
        base: &Context,
        placement: HudPlacement,
        place: impl FnOnce(Context, HudPlacement) -> Context,
    ) -> Context {
        if let Some((built_base, built_placement, context)) = &self.built
            && *built_placement == placement
            && built_base == base
        {
            return context.clone();
        }
        let context = place(base.clone(), placement);
        self.built = Some((base.clone(), placement, context.clone()));
        context
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use json_ui::Context;
    use launcher::menu::settings_options::{
        BEDROCK_HOTBAR_OPTION, CHAT_POSITION_OPTION, SETTINGS_OPTIONS, SettingsOptions,
    };

    use super::{HudPlacement, PlacedContext, install};

    #[test]
    fn placed_context_is_rebuilt_only_for_a_new_base_or_placement() {
        let builds = Cell::new(0);
        let place = |base: Context, placement: HudPlacement| {
            builds.set(builds.get() + 1);
            placement.placed(base)
        };
        let mut retained = PlacedContext::default();
        let base = Context::desktop();
        let [bottom, top, raised, ..] = HudPlacement::ALL;
        for placement in [bottom, bottom, top, top, raised, raised, bottom] {
            assert_eq!(
                retained.get(&base, placement, place),
                placement.placed(base.clone())
            );
        }
        assert_eq!(builds.get(), 4, "each placement change builds once");
        let changed = base.clone().with_flag("touch", true);
        assert_eq!(
            retained.get(&changed, bottom, place),
            bottom.placed(changed.clone())
        );
        assert_eq!(builds.get(), 5, "a changed base builds again");
        assert_eq!(
            retained.get(&Context::desktop().with_flag("touch", true), bottom, place),
            bottom.placed(changed)
        );
        assert_eq!(builds.get(), 5, "an equal base keeps the built context");
    }

    #[test]
    fn settings_select_each_placement_flag_independently() {
        let index = |name: &str| {
            SETTINGS_OPTIONS
                .iter()
                .position(|option| option.name == name)
                .unwrap()
        };
        for expected in HudPlacement::ALL {
            let mut options = SettingsOptions::default();
            options.set(
                index(CHAT_POSITION_OPTION.name),
                i32::from(expected.chat_top),
            );
            options.set(
                index(BEDROCK_HOTBAR_OPTION.name),
                i32::from(expected.bedrock_hotbar),
            );
            let placement = HudPlacement::from_settings(&options);
            assert_eq!(placement, expected);
            let vars = placement.placed(Context::desktop()).vars().clone();
            assert_eq!(
                vars.get(BEDROCK_HOTBAR_OPTION.name),
                Some(&serde_json::Value::Bool(expected.bedrock_hotbar))
            );
        }
        assert_eq!(
            HudPlacement::from_settings(&SettingsOptions::default()),
            HudPlacement::default(),
            "Java's flush hotbar and bottom chat stay the defaults"
        );
    }

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
    fn video_section_shows_the_bedrock_hotbar_toggle_after_interface_opacity() {
        let Some(carrier) = ui_engine::test_support::carrier() else {
            eprintln!(
                "skipping video_section_shows_the_bedrock_hotbar_toggle_after_interface_opacity: missing UI carrier (make assets)"
            );
            return;
        };
        let files = carrier.ui_files();
        let mut catalog =
            json_ui::Catalog::from_files(files.iter().map(|file| (&*file.path, &*file.bytes)))
                .unwrap();
        install(&mut catalog);
        let section = json_ui::resolve(
            &catalog,
            "general_section.video_section",
            &json_ui::Context::retail(false),
        )
        .control
        .expect("video settings");
        let position = |name: &str| section.children.iter().position(|child| child.name == name);
        let toggle = position(BEDROCK_HOTBAR_OPTION.name).expect("bedrock hotbar toggle");
        assert_eq!(
            Some(toggle),
            position("hud_opacity_slider").map(|at| at + 1)
        );
        assert!(has_text(
            &section.children[toggle],
            BEDROCK_HOTBAR_OPTION.label
        ));
    }
}
