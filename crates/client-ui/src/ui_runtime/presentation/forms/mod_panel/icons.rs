use serde_json::{Value, json};
use ui::mod_panel::Icon;

fn vector(icon: &str, size: [f64; 2], offset: [f64; 2], color: [f64; 4]) -> Value {
    json!({"type":"custom", "renderer":"cinnabar_vector_icon", "icon":icon,
        "anchor_from":"top_left", "anchor_to":"top_left",
        "size":size, "offset":offset, "color":color})
}

pub(super) fn mark(kind: Icon, size: f64, offset: [f64; 2], color: [f64; 4]) -> Value {
    let icon = match kind {
        Icon::Pointer => "pointer",
        Icon::Crosshair => "crosshair",
        Icon::Ruler => "ruler",
        Icon::Settings => "settings",
        Icon::None => "none",
    };
    vector(icon, [size; 2], offset, color)
}

pub(super) fn brand(size: f64, offset: [f64; 2], color: [f64; 4]) -> Value {
    vector("brand", [size; 2], offset, color)
}
pub(super) fn sword(size: f64, offset: [f64; 2], color: [f64; 4]) -> Value {
    vector("sword", [size; 2], offset, color)
}
pub(super) fn close(size: f64, offset: [f64; 2], color: [f64; 4]) -> Value {
    vector("close", [size; 2], offset, color)
}
pub(super) fn chevron(offset: [f64; 2], color: [f64; 4]) -> Value {
    vector("chevron", [5., 3.], offset, color)
}
