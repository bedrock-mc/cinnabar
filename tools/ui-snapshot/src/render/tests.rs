use super::REGISTRY;
use json_ui::{
    Catalog, DataSource, Draw, EmptyLibrary, HitKind, LayoutEnv, NineSlice, ResolvedControl,
    Scalar, TextMeasure, TextureMeta, TextureSource,
};
use serde_json::json;

struct Resources;

impl TextMeasure for Resources {
    /// Keeps this catalog-layering test independent of local font carriers.
    fn extent(&self, _: &str) -> [f64; 2] {
        [0.0; 2]
    }
}

impl TextureSource for Resources {
    /// Stretches like the retail backdrop: a 2x2 image with one-pixel nine-slice borders.
    fn texture(&self, _: &str) -> Option<TextureMeta> {
        Some(TextureMeta {
            base_size: [2.0; 2],
            nineslice: Some(NineSlice {
                left: 1.0,
                top: 1.0,
                right: 1.0,
                bottom: 1.0,
            }),
            pixels: [2.0; 2],
        })
    }
}

/// Binds the chat opacity the shipped backdrops read, at a translucent value.
fn with_chat_opacity(control: &ResolvedControl) -> ResolvedControl {
    let mut data = DataSource::new();
    data.set_global("#chat_background_opacity", Scalar::Num(0.5));
    json_ui::bind(control, &data, &EmptyLibrary)
}

#[test]
fn snapshot_chat_uses_the_shipped_input_and_autocomplete_backdrops() {
    let carrier = Catalog::from_files([
        ("ui/_global_variables.json", b"{}".as_slice()),
        (
            "ui/_ui_defs.json",
            br#"{"ui_defs":["ui/common.json","ui/chat_screen.json"]}"#.as_slice(),
        ),
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
        let screen = with_chat_opacity(&screen);
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
        assert_eq!(input.rect.x, 4.0, "input text keeps its left inset");
        assert_eq!(input.rect.w, viewport[0] - 8.0);
        let draws = json_ui::emit(&laid, &env);
        // The backdrop spans the viewport with a two-pixel inset on each side.
        let (left, right) = draws
            .iter()
            .filter(|node| {
                matches!(&node.draw, Draw::Sprite { texture, .. } if texture == "textures/ui/Black")
                    && node.alpha > 0.0
                    && node.alpha < 1.0
            })
            .fold((f64::MAX, f64::MIN), |(left, right), node| {
                let (x, w) = (node.dest.x, node.dest.w);
                (left.min(x), right.max(x + w))
            });
        assert_eq!(left, 2.0);
        assert_eq!(right, viewport[0] - 2.0);
    }
    let suggestion = json_ui::resolve(
        &catalog,
        "chat.auto_complete_panel_contents_with_item",
        REGISTRY.context(),
    )
    .control
    .unwrap();
    let suggestion = with_chat_opacity(&suggestion);
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

#[test]
fn custom_panel_texture_collection_resolves_static_template_parameters() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("textures")).unwrap();
    std::fs::write(
        directory.path().join("textures/icon.png"),
        b"original-artwork",
    )
    .unwrap();
    let panel: ui::mod_panel::Panel = serde_json::from_value(json!({
        "title":"Fixture", "toggle_key":"F8", "dark":true, "controls":[],
        "surface":{
            "screen":"fixture.root",
            "document":json!({
                "namespace":"fixture",
                "image":{"type":"image", "texture":"$asset"},
                "root":{"type":"panel", "controls":[
                    {"icon@fixture.image":{"$asset":"textures/icon"}}
                ]}
            }).to_string()
        }
    }))
    .unwrap();
    let files = super::panel_textures(&panel, directory.path()).unwrap();
    assert_eq!(
        files,
        vec![("textures/icon.png".into(), b"original-artwork".to_vec())]
    );
}
