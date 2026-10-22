//! Saved chat colors apply after binding the built-in notification factory.

use super::*;
use assets::{RuntimeUiAssets, UiAtlasPage, encode_ui_catalog};
use json_ui::{HudChanges, Timed};
use launcher::menu::settings_options::{SettingId, SettingsOptions};
use ui::{BedrockColor, DpiScale, SafeArea, TextLayoutCache, UiVisual};
use ui_engine::{
    sources::{ImageWork, RemoteImage, RemoteSource},
    test_support::fixture_font,
    text_metrics::TextMetrics,
};

struct Offline;

impl RemoteSource for Offline {
    /// The fixture uses only local artwork.
    fn state(&self, _: &str) -> RemoteImage {
        RemoteImage::Failed
    }
}

/// Supplies the carrier labels while retaining the production chat panel and factory.
fn engine(server_content: bool) -> FormEngine {
    let bytes = encode_ui_catalog(
        [1; 32],
        &[UiAtlasPage {
            width: 1,
            height: 1,
            rgba8: vec![255; 4].into(),
        }],
        &[],
        &[],
        &[],
    )
    .unwrap();
    let catalog = Catalog::from_files([
        ("ui/_global_variables.json", b"{}".as_slice()),
        (
            "ui/_ui_defs.json",
            br#"{"ui_defs":["ui/hud_screen.json"]}"#.as_slice(),
        ),
        (
            "ui/hud_screen.json",
            br##"{"namespace":"hud",
        "hud_screen":{"type":"screen","size":["100%","100%"],"controls":[
            {"additional@$additional_screen_content":{}},
            {"unrelated":{"type":"label","text":"Other HUD text","size":["default",12]}}
        ]},
        "chat_label":{"type":"label","text":"#text","size":["default",12],"localize":false}
    }"##
            .as_slice(),
        ),
    ])
    .unwrap();
    let mut registry = ScreenRegistry::new(Context::desktop());
    register(&mut registry);
    registry.screen(Reference::HudHudScreen.as_str());
    let mut engine = FormEngine::new(
        Arc::new(RuntimeUiAssets::decode(&bytes).unwrap()),
        catalog,
        1,
        Box::leak(Box::new(registry)),
        Arc::new(Offline),
        ImageWork::inline(),
    );
    if server_content {
        engine.set_server_pack(&[crate::test_support::hover_chat::files()]);
    }
    engine
}

#[test]
fn factory_notifications_paint_selected_colors_after_message_parsing() {
    let font = fixture_font();
    for server_content in [false, true] {
        let engine = engine(server_content);
        for caption in ["Plain caption", "§dServer-colored caption"] {
            let mut screens = HudScreens::default();
            let context = screens.context(&Context::desktop(), false);
            let mut model = HudModel {
                chat_visible: true,
                chat: vec![Timed {
                    text: if server_content {
                        crate::test_support::hover_chat::message_with_caption(caption)
                    } else {
                        caption.to_owned()
                    },
                    born: 0.0,
                }],
                ..Default::default()
            };
            let mut layouts = TextLayoutCache::new(32, 1024 * 1024);
            for (index, color) in [0, 1, 0].into_iter().enumerate() {
                let mut options = SettingsOptions::default();
                options.set(SettingId::ChatColor.index(), color);
                crate::settings_chat::apply_hud(&options, &mut model);
                let data = screens.begin(
                    Some((
                        &model,
                        if index == 0 {
                            HudChanges::ALL
                        } else {
                            HudChanges::default()
                        },
                        100,
                    )),
                    &BTreeMap::new(),
                    1,
                    engine.catalog(),
                );
                let mut nodes = Vec::new();
                let mut next = 1;
                let mut frame = PaintFrame {
                    layouts: &mut layouts,
                    font: &font,
                    metrics: TextMetrics::for_viewport(
                        [1280, 720],
                        DpiScale::new(1.0).unwrap(),
                        Some(2),
                    ),
                    solid_page: 0,
                    safe_area: SafeArea::ZERO,
                    content: [1280.0, 720.0],
                    translate: &|_| None,
                    language: [0; 3],
                    now: 0.1,
                    nodes: &mut nodes,
                    next: &mut next,
                    renderers: &mut [],
                };
                screens
                    .paint(
                        &engine,
                        HudInput {
                            capture_keep_out: false,
                            renderers_unchanged: false,
                            crosshair: false,
                            catalog: engine.catalog(),
                            context: &context,
                            data,
                            paint: &HudPaint::default(),
                            icons: &[],
                        },
                        &mut frame,
                    )
                    .unwrap()
                    .unwrap();
                assert!(
                    screens.hud.nodes().iter().any(|node| matches!(
                        &node.draw, json_ui::Draw::Text { text, .. } if text == caption
                    )),
                    "caption stays decoded after binding: server={server_content}, color={color}, caption={caption}"
                );
                let colors = nodes
                    .iter()
                    .filter_map(|node| match node.visual() {
                        UiVisual::Text { layout, .. } => layout
                            .glyphs()
                            .first()
                            .map(|glyph| (glyph.codepoint, glyph.style.color)),
                        _ => None,
                    })
                    .collect::<BTreeMap<_, _>>();
                let selected = if caption.starts_with('§') {
                    BedrockColor::LightPurple
                } else if color == 0 {
                    BedrockColor::Base
                } else {
                    BedrockColor::Green
                };
                let first = caption
                    .strip_prefix("§d")
                    .unwrap_or(caption)
                    .chars()
                    .next()
                    .unwrap();
                assert_eq!(colors.len(), 2);
                assert_eq!(colors[&'O'], BedrockColor::Base);
                assert_eq!(
                    colors[&first], selected,
                    "server={server_content}, color={color}, caption={caption}"
                );
            }
        }
    }
}
