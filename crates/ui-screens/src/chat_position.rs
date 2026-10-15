//! The Video-section chat placement preference uses the existing dropdown templates.

use json_ui::{Catalog, Context};

use launcher::menu::settings_options::{CHAT_POSITION_OPTION, SettingKind};

/// Adds a client preference to the vanilla Video section, using one shared option definition.
pub fn install(catalog: &mut Catalog) {
    let option = CHAT_POSITION_OPTION;
    let SettingKind::Dropdown(choices) = option.kind else {
        unreachable!()
    };
    let name = option.name;
    let content = format!("{name}_content");
    let rows: Vec<_> = choices
        .iter()
        .map(|choice| {
            serde_json::json!({
                "@settings_common.radio_with_label": {
                    "$toggle_state_binding_name": format!("#{}", choice.name),
                    "$radio_label_text": choice.label
                }
            })
        })
        .collect();
    let overlay = serde_json::json!({
        "namespace": "general_section",
        "video_section": {
            "modifications": [{
                "array_name": "controls", "operation": "insert_front",
                "value": [{
                    format!("{name}@settings_common.option_dropdown"): {
                        "$option_label": option.label,
                        "$dropdown_content": format!("general_section.{content}"),
                        "$dropdown_area": "content_area",
                        "$dropdown_name": format!("{name}_dropdown"),
                        "$option_enabled_binding_name": format!("#{name}_dropdown_enabled"),
                        "$options_dropdown_toggle_label_binding": format!("#{name}_dropdown_toggle_label"),
                        "$dropdown_scroll_content_size": ["100%", "145%"]
                    }
                }]
            }]
        },
        format!("{content}@settings_common.option_radio_dropdown_group"): { "$radio_buttons": rows }
    });
    catalog.overlay_text("ui/cinnabar_chat_position.json", &overlay.to_string());
}

/// `context` with the chat at the top or bottom of the screen.
pub fn placed(context: Context, top: bool) -> Context {
    context.with_flag("cinnabar_chat_top", top)
}

/// A placed context kept until its base context or placement changes, so the retained screen
/// reading it compares it by pointer rather than variable by variable.
#[derive(Default)]
pub struct PlacedContext {
    built: Option<(Context, bool, Context)>,
}

impl PlacedContext {
    /// `place(base, top)`, reused while `base` and `top` match the last call's. Each holder
    /// passes the same `place` on every call.
    pub fn get(
        &mut self,
        base: &Context,
        top: bool,
        place: impl FnOnce(Context, bool) -> Context,
    ) -> Context {
        if let Some((built_base, built_top, context)) = &self.built
            && *built_top == top
            && built_base == base
        {
            return context.clone();
        }
        let context = place(base.clone(), top);
        self.built = Some((base.clone(), top, context.clone()));
        context
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use json_ui::Context;

    use super::{PlacedContext, placed};

    #[test]
    fn placed_context_is_rebuilt_only_for_a_new_base_or_placement() {
        let builds = Cell::new(0);
        let place = |base: Context, top: bool| {
            builds.set(builds.get() + 1);
            placed(base, top)
        };
        let mut retained = PlacedContext::default();
        let base = Context::desktop();
        for top in [false, false, true, true, false] {
            assert_eq!(retained.get(&base, top, place), placed(base.clone(), top));
        }
        assert_eq!(builds.get(), 3, "each placement change builds once");
        let changed = base.clone().with_flag("touch", true);
        assert_eq!(
            retained.get(&changed, false, place),
            placed(changed.clone(), false)
        );
        assert_eq!(builds.get(), 4, "a changed base builds again");
        assert_eq!(
            retained.get(&Context::desktop().with_flag("touch", true), false, place),
            placed(changed, false)
        );
        assert_eq!(builds.get(), 4, "an equal base keeps the built context");
    }
}
