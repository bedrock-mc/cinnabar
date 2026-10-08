//! A JSON-UI editor core over Cinnabar's own engine: layered packs, the
//! engine's resolve/bind/layout/emit pipeline, the client's text and draw-list
//! stack, a CPU rasterizer, provenance and diagnostics. The browser editor
//! (`wasm`) and the `jsonui-mcp` server are thin front ends over [`api`].

pub mod api;
pub mod diagnose;
pub mod export;
pub mod index;
pub mod mock;
pub mod outline;
pub mod paint;
pub mod provenance;
pub mod raster;
pub mod scene;
pub mod text;
pub mod textures;
pub mod workspace;

#[cfg(target_arch = "wasm32")]
mod wasm;

pub use scene::{Frame, LaidBox, Session, View};

/// Context presets the picker offers, as `$`-less variable maps.
pub fn context_presets() -> serde_json::Value {
    use json_ui::Context;
    use serde_json::json;
    // Vanilla's safe-zone size variable with a full safe zone: a zero
    // extent on the inset axis, `100%` on the other.
    let vars = |context: Context| {
        let context = context
            .with_var("top_vertical_safezone_size", json!(["100%", 0]))
            .with_var("bottom_vertical_safezone_size", json!(["100%", 0]))
            .with_var("left_horizontal_safezone_size", json!([0, "100%"]))
            .with_var("right_horizontal_safezone_size", json!([0, "100%"]));
        serde_json::to_value(context.vars()).unwrap_or_default()
    };
    let pocket = Context::retail(false)
        .with_flag("desktop_screen", false)
        .with_flag("pocket_screen", true)
        .with_flag("touch", true)
        .with_flag("win10_edition", false)
        .with_flag("pocket_edition", true);
    let console = Context::retail(false)
        .with_flag("win10_edition", false)
        .with_flag("console_edition", true);
    json!({
        "Desktop (Windows)": vars(Context::retail(false)),
        "Desktop (macOS)": vars(Context::retail(true)),
        "Pocket (touch)": vars(pocket),
        "Console": vars(console),
        "Empty": {},
    })
}
