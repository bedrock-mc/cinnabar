//! The launcher's optional retry controls on a remote-session failure.

/// Installs the retry layout below server packs while retaining the disconnect screen.
pub fn install(catalog: &mut json_ui::Catalog) {
    catalog.overlay_text("ui/cinnabar_disconnect_retry.json", LAYOUT);
    let route = serde_json::json!({
        "namespace": "disconnect",
        "disconnect_screen": {
            "$cinnabar_reconnect|default": false,
            "variables": [{
                "requires": "$cinnabar_reconnect",
                "$button_layout": BUTTON_LAYOUT
            }]
        }
    });
    catalog.overlay_text("ui/cinnabar_disconnect_route.json", &route.to_string());
}

pub(super) const BUTTON_LAYOUT: &str = "@cinnabar_disconnect.retry_buttons";

const LAYOUT: &str = r##"{
  "namespace": "cinnabar_disconnect",
  "retry_buttons": {
    "type": "stack_panel",
    "orientation": "vertical",
    "size": [128, "100%c"],
    "anchor_from": "bottom_middle", "anchor_to": "bottom_middle",
    "offset": [0, "-20%"],
    "controls": [
      {"retry@disconnect.menu_button_template": {
        "$pressed_button_name": "button.cinnabar_reconnect",
        "$button_text": "$cinnabar_reconnect_text"
      }},
      {"gap": {"type": "panel", "size": ["100%", 4]}},
      {"dismiss@disconnect.ok_button": {}}
    ]
  }
}"##;
