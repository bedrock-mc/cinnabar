//! Server-owned message buttons expose hover feedback through the chat adapter.

use super::*;
use assets::{RuntimeUiAssets, UiAtlasPage, encode_ui_catalog};
use json_ui::{Catalog, Context, Draw};
use ui::{DpiScale, SafeArea, TextLayoutCache};
use ui_engine::{
    sources::{ImageWork, RemoteImage, RemoteSource},
    test_support::fixture_font,
    text_metrics::TextMetrics,
};

struct Offline;
impl RemoteSource for Offline {
    /// This fixture has no network dependencies.
    fn state(&self, _: &str) -> RemoteImage {
        RemoteImage::Failed
    }
}

/// Creates anonymous chat history over an empty artwork carrier.
fn engine() -> FormEngine {
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
    let mut catalog = Catalog::default();
    catalog.overlay_text("ui/chat_screen.json", &serde_json::json!({"namespace":"chat",
        "chat_screen":{"type":"screen","size":["100%","100%"],"controls":[{
            "messages_panel":{"type":"stack_panel","size":["100%","100%c"],
                "anchor_from":"top_left","anchor_to":"top_left","offset":[20,20],
                "factory":{"name":"messages_factory","control_ids":{"chat_screen_messages":"chat.messages_text"}}}
        }]},
        "messages_text":{"type":"panel","size":["100%","100%c"],
            "controls":[{"text":{"type":"label","text":"#text"}}]}}).to_string());
    catalog.overlay_text(
        "ui/hud_screen.json",
        r#"{"namespace":"hud","hud_screen":{"type":"screen"}}"#,
    );
    let mut registry = ScreenRegistry::new(Context::desktop());
    registry.screen(CHAT_SCREEN);
    let mut engine = FormEngine::new(
        Arc::new(RuntimeUiAssets::decode(&bytes).unwrap()),
        catalog,
        1,
        Box::leak(Box::new(registry)),
        Arc::new(Offline),
        ImageWork::inline(),
    );
    engine.set_server_pack(&[crate::test_support::hover_chat::files()]);
    engine
}

#[test]
fn message_tooltip_tracks_pointer_without_a_native_chat_action() {
    let engine = engine();
    let font = fixture_font();
    let messages = [ui::ChatMessage {
        fifo_sequence: 1,
        received_millis: 0,
        kind: ui::ChatMessageKind::System,
        source: None,
        message: crate::test_support::hover_chat::message().into(),
        parameters: Arc::from([]),
    }]
    .into();
    let editor = ui::ChatEditor::new(ui::MAX_CHAT_INPUT_BYTES).unwrap();
    for dpi in [1.0, 1.5, 2.0] {
        let mut screen = ChatScreen::default();
        let metrics = TextMetrics::for_viewport([1280, 720], DpiScale::new(dpi).unwrap(), Some(2));
        let mut layouts = TextLayoutCache::new(32, 1024 * 1024);
        let mut nodes = Vec::new();
        let mut next = 1;
        let mut point = None;
        for hovered in [false, true, true, false] {
            screen
                .set_pointer(hovered.then(|| point.expect("the first frame measured the caption")));
            let mut frame = PaintFrame {
                layouts: &mut layouts,
                font: &font,
                metrics,
                solid_page: 0,
                safe_area: SafeArea::ZERO,
                content: [1280.0 / dpi, 720.0 / dpi],
                translate: &|_| None,
                language: [0; 3],
                now: 0.0,
                nodes: &mut nodes,
                next: &mut next,
                renderers: &mut [],
            };
            screen
                .paint(
                    &engine,
                    ChatInput {
                        editor: &editor,
                        messages: &messages,
                        suggestions: &[],
                        usage: None,
                        selected_suggestion: None,
                        annotations: &[],
                        clocks: &BTreeMap::new(),
                        now_millis: 0,
                    },
                    &mut frame,
                )
                .unwrap();
            let caption = screen
                .nodes()
                .iter()
                .find(|node| node.name == "caption")
                .unwrap();
            assert!(matches!(&caption.draw, Draw::Text { text, .. }
                if text == crate::test_support::hover_chat::CAPTION));
            let tooltip = screen.nodes().iter().find_map(|node| match &node.draw {
                Draw::Custom { renderer, data } if renderer == "hover_text_renderer" => Some(data),
                _ => None,
            });
            assert_eq!(tooltip.is_some(), hovered);
            if let Some(data) = tooltip {
                assert_eq!(
                    data["#hover_text"],
                    crate::test_support::hover_chat::TOOLTIP
                );
            }
            let caption_point = UiPoint::new(
                (caption.dest.x as f32 + 4.0) * screen.scale,
                (caption.dest.y as f32 + 4.0) * screen.scale,
            )
            .unwrap();
            assert_eq!(
                screen.hit_test(caption_point),
                None,
                "hover content needs no native action"
            );
            point = Some(caption_point);
        }
        screen.close();
        assert!(screen.hover_hits.is_empty());
    }
}
