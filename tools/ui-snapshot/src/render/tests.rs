use super::REGISTRY;
use json_ui::{Catalog, Draw, HitKind, LayoutEnv, TextMeasure, TextureMeta, TextureSource};
use serde_json::json;

struct Resources;

impl TextMeasure for Resources {
    /// Keeps this catalog-layering test independent of local font carriers.
    fn extent(&self, _: &str) -> [f64; 2] {
        [0.0; 2]
    }
}

impl TextureSource for Resources {
    /// Supplies a plain texture so layout can emit the shipped backdrops.
    fn texture(&self, _: &str) -> Option<TextureMeta> {
        Some(TextureMeta::plain([1.0; 2]))
    }
}

#[test]
fn snapshot_chat_uses_the_shipped_input_and_autocomplete_backdrops() {
    let carrier = Catalog::from_files([
        (
            "ui/common.json",
            br#"{
                "namespace": "common",
                "base_screen": {
                    "type": "panel",
                    "size": ["100%", "100%"],
                    "controls": [{"content@$screen_content": {}}]
                }
            }"#
            .as_slice(),
        ),
        (
            "ui/chat_screen.json",
            br#"{
                "namespace": "chat",
                "chat_screen@common.base_screen": {
                    "$screen_content": "chat.carrier_content"
                },
                "carrier_content": {
                    "type": "edit_box", "size": ["100%", "100%"]
                },
                "auto_complete_panel_contents_with_item": {
                    "type": "panel", "size": ["100%", "100%"]
                }
            }"#
            .as_slice(),
        ),
    ])
    .unwrap();
    let catalog = REGISTRY.catalog().builtin(&carrier);
    let env = LayoutEnv {
        text: &Resources,
        textures: &Resources,
    };
    let viewport = [640.0, 360.0];
    for width in [180.0, 240.0] {
        let context = REGISTRY
            .context()
            .clone()
            .with_var("cinnabar_chat_width", json!(width));
        let screen = json_ui::resolve(&catalog, ui_screens::chat_screen::CHAT_SCREEN, &context)
            .control
            .unwrap();
        let laid = json_ui::layout(&screen, viewport, &env);
        let hits = json_ui::hit_regions(&laid);
        let input = hits
            .iter()
            .find(|hit| hit.kind == HitKind::EditBox)
            .unwrap();
        assert!(
            input.rect.y > viewport[1] / 2.0,
            "input must stay near the bottom"
        );
        assert!(
            input.rect.w <= width,
            "input must follow the configured chat width"
        );
        let draws = json_ui::emit(&laid, &env);
        assert!(
            draws.iter().any(|node| {
                matches!(&node.draw, Draw::Sprite { texture, .. } if texture == "textures/ui/Black")
                    && node.dest.w == width
                    && node.alpha > 0.0
                    && node.alpha < 1.0
            }),
            "the input must paint its shipped translucent backdrop"
        );
    }
    let suggestion = json_ui::resolve(
        &catalog,
        "chat.auto_complete_panel_contents_with_item",
        REGISTRY.context(),
    )
    .control
    .unwrap();
    let draws = json_ui::emit(&json_ui::layout(&suggestion, [120.0, 20.0], &env), &env);
    assert!(
        draws.iter().any(|node| {
            matches!(&node.draw, Draw::Sprite { texture, .. } if texture == "textures/ui/Black")
                && node.alpha > 0.0
                && node.alpha < 1.0
        }),
        "autocomplete must paint its shipped translucent backdrop"
    );
}
