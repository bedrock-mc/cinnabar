//! Original message-content fixture using the JSON-UI string operators and hover renderer.

use serde_json::json;

pub const CAPTION: &str = "§dHover for details";
pub const TOOLTIP: &str = "§d§lMESSAGE DETAILS\n§r\nExtra information\nOnly shown on hover";

/// Encodes a caption byte length followed by the caption and tooltip text.
pub fn message() -> String {
    message_with_caption(CAPTION)
}

/// Encodes caller-selected caption formatting with the fixture's tooltip.
pub fn message_with_caption(caption: &str) -> String {
    format!("%.{:05}s{caption}{TOOLTIP}", caption.len())
}

/// A server pack whose message rows parse their caption and retain a zero-sized tooltip.
pub fn files() -> Vec<(String, Vec<u8>)> {
    [
        ("ui/_ui_defs.json", json!({"ui_defs":["ui/hover_chat.json","ui/chat_screen.json","ui/hud_screen.json"]})),
        ("ui/_global_variables.json", json!({"$hover_caption_width":160})),
        ("ui/hud_screen.json", json!({"namespace":"hud","chat_grid_item":{
            "controls":[{"message@hover.row":{}}]}})),
        ("ui/chat_screen.json", json!({"namespace":"chat","messages_text":{
            "bindings":[{"binding_name":"#chat_visible","binding_name_override":"#visible"}],
            "controls":[{"message@hover.row":{}}]}})),
        ("ui/hover_chat.json", json!({"namespace":"hover","row":{
            "type":"panel","size":["100%",12],"anchor_from":"top_left","anchor_to":"top_left",
            "controls":[
                {"data":{"type":"panel","size":[0,0],"bindings":[
                    {"binding_type":"collection","binding_collection_name":"chat_text_grid",
                        "binding_name":"#chat_text","binding_name_override":"#text","binding_condition":"once"},
                    {"binding_type":"view","source_property_name":"#text","target_property_name":"#raw"},
                    {"binding_type":"view","source_property_name":"('%.8s' * #raw)","target_property_name":"#format"},
                    {"binding_type":"view","source_property_name":"(('\u{3}' + #raw) - ('\u{3}' + #format))","target_property_name":"#body"},
                    {"binding_type":"view","source_property_name":"(#format * #body)","target_property_name":"#caption"},
                    {"binding_type":"view","source_property_name":"(('\u{3}' + #body) - ('\u{3}' + #caption))","target_property_name":"#detail"}
                ]}},
                {"hover_button":{"type":"button","size":["$hover_caption_width",12],
                    "anchor_from":"top_left","anchor_to":"top_left","focus_enabled":false,
                    "bindings":[
                        {"binding_type":"view","source_control_name":"data","resolve_sibling_scope":true,
                            "source_property_name":"#caption","target_property_name":"#caption"},
                        {"binding_type":"view","source_control_name":"data","resolve_sibling_scope":true,
                            "source_property_name":"#detail","target_property_name":"#detail"}
                    ],
                    "default_control":"idle","hover_control":"tooltip","controls":[
                        {"idle":{"type":"panel","size":[0,0]}},
                        {"caption":{"type":"label","text":"#text","localize":false,
                            "anchor_from":"top_left","anchor_to":"top_left","size":["default",12],
                            "bindings":[{"binding_type":"view","source_control_name":"hover_button",
                                "resolve_sibling_scope":true,"source_property_name":"#caption","target_property_name":"#text"}]}},
                        {"tooltip":{"type":"custom","renderer":"hover_text_renderer","size":[0,0],
                            "layer":100,"allow_clipping":false,"hover_text_max_width":260,
                            "bindings":[{"binding_type":"view","source_control_name":"hover_button",
                                "resolve_sibling_scope":true,"source_property_name":"#detail","target_property_name":"#hover_text"}]}}
                    ]}},
                {"pass_through":{"type":"input_panel","size":["$hover_caption_width",12],
                    "anchor_from":"top_left","anchor_to":"top_left","layer":5,
                    "consume_hover_events":false}}
            ]}})),
    ].into_iter().map(|(path, value)| (path.to_owned(), value.to_string().into_bytes())).collect()
}
