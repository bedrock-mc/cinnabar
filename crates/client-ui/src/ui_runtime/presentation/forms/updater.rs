//! Cinnabar update notice using the start screen's buttons and General settings toggle.

use crate::menu::{MenuAction, MenuScreen, MenuView};
use json_ui::{Context, DataSource, HitKind, HitRegion, Scalar};
use serde_json::json;

/// Adds an original notice while retaining the pack's start controls and settings layout.
pub(super) fn extend_catalog(catalog: &mut json_ui::Catalog) {
    catalog.overlay_text("ui/cinnabar_update.json", include_str!("updater.json"));
    catalog.overlay_text("ui/cinnabar_update_start.json", START);
    catalog.overlay_text("ui/cinnabar_update_settings.json", SETTINGS);
}

/// Reserves space above the home screen only while an update has something to report.
pub(super) fn bind_home(view: &MenuView, data: &mut DataSource, context: Context) -> Context {
    let visible = !view.over_world && !view.update.message.is_empty();
    data.set_global("#cinnabar_update_visible", Scalar::Bool(visible));
    data.set_global(
        "#cinnabar_update_message",
        Scalar::Text(view.update.message.clone()),
    );
    data.set_global(
        "#cinnabar_update_ready",
        Scalar::Bool(visible && view.update.ready),
    );
    data.set_global(
        "#cinnabar_update_retry",
        Scalar::Bool(visible && view.update.retry),
    );
    data.set_global(
        "#cinnabar_update_notes",
        Scalar::Bool(visible && view.update.notes),
    );
    context.with_var(
        "cinnabar_home_size",
        json!(["100%", if visible { "100% - 62px" } else { "100%" }]),
    )
}

/// Gives the existing General auto-update toggle the host's effective preference.
pub(super) fn bind_settings(view: &MenuView, data: &mut DataSource) {
    data.set_global("#auto_update_enabled", Scalar::Bool(view.update.enabled));
    data.set_global("#auto_update_enabled_enabled", Scalar::Bool(true));
}

/// Maps only enabled update controls, keeping restart actions outside world sessions.
pub(super) fn action(view: &MenuView, region: &HitRegion) -> Option<MenuAction> {
    if !region.enabled {
        return None;
    }
    if view.screen == MenuScreen::Settings
        && region.kind == HitKind::Toggle
        && region
            .control_name
            .as_deref()
            .map(|name| name.trim_start_matches('#'))
            == Some("auto_update_enabled")
    {
        return Some(MenuAction::UpdateToggle);
    }
    if view.screen != MenuScreen::Home || view.over_world || view.update.message.is_empty() {
        return None;
    }
    match region.pressed.as_deref()? {
        "button.cinnabar_update_restart" if view.update.ready => Some(MenuAction::UpdateRestart),
        "button.cinnabar_update_retry" if view.update.retry => Some(MenuAction::UpdateRetry),
        "button.cinnabar_update_notes" if view.update.notes => Some(MenuAction::UpdateNotes),
        _ => None,
    }
}

const START: &str = r#"{
  "namespace": "start",
  "start_screen": { "$screen_content": "cinnabar_update.home" }
}"#;

// General already supplies this setting on desktop; expose the same toggle on Linux as well.
const SETTINGS: &str = r##"{
  "namespace": "general_section",
  "general_tab_section": {
    "modifications": [
      { "array_name": "controls", "operation": "replace", "control_name": "auto_update_mode_dropdown",
        "value": { "auto_update_mode_dropdown": { "type": "panel", "ignored": true } } },
      { "array_name": "controls", "operation": "replace", "control_name": "auto_update_enabled_toggle",
        "value": { "auto_update_enabled_toggle@settings_common.option_toggle": {
          "$option_label": "options.autoUpdateEnabled",
          "$option_binding_name": "#auto_update_enabled",
          "$option_enabled_binding_name": "#auto_update_enabled_enabled",
          "$toggle_name": "auto_update_enabled"
        } } }
    ]
  }
}"##;

#[cfg(test)]
mod tests;
