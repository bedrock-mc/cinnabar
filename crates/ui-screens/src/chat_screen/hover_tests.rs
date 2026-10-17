//! Server-owned message buttons expose hover feedback through the chat adapter.

use super::*;
use assets::{RuntimeUiAssets, UiAtlasPage, encode_ui_catalog};
use json_ui::{Catalog, Context, Draw};
use ui::{DpiScale, SafeArea, TextLayoutCache, UiVisual};
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
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(16, 16, image::Rgba([20, 10, 30, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let background = ui_engine::engine::tooltip::BACKGROUND_TEXTURE;
    engine.set_server_atlas(
        ui_engine::atlas::ServerAtlas::new(
            &[
                (format!("{background}.png"), png),
                (
                    format!("{background}.json"),
                    br#"{"nineslice_size":4,"base_size":[16,16]}"#.to_vec(),
                ),
            ],
            None,
            1,
        )
        .with_image_work(ImageWork::inline()),
        1,
    );
    engine
}

#[test]
fn message_tooltip_tracks_pointer_without_a_native_chat_action() {
    for caption in [crate::test_support::hover_chat::CAPTION, "Plain caption"] {
        for color in [0, 1] {
            check_message_tooltip(caption, color);
        }
    }
}

/// Paints decoded text and hover feedback with settings, scaling and safe-area variations.
fn check_message_tooltip(expected_caption: &str, color: i32) {
    let engine = engine();
    let font = fixture_font();
    let messages = [ui::ChatMessage {
        fifo_sequence: 1,
        received_millis: 0,
        kind: ui::ChatMessageKind::System,
        source: None,
        message: crate::test_support::hover_chat::message_with_caption(expected_caption).into(),
        parameters: Arc::from([]),
    }]
    .into();
    let editor = ui::ChatEditor::new(ui::MAX_CHAT_INPUT_BYTES).unwrap();
    for (dpi, safe_area) in [1.0, 1.5, 2.0].into_iter().flat_map(|dpi| {
        [SafeArea::ZERO, SafeArea::new(24.0, 18.0, 0.0, 0.0).unwrap()].map(|area| (dpi, area))
    }) {
        let mut screen = ChatScreen::default();
        let mut options = launcher::menu::settings_options::SettingsOptions::default();
        options.set(SettingId::ChatColor.index(), color);
        screen.settings.set_snapshot((Arc::new(options), None));
        let metrics = TextMetrics::for_viewport([1280, 720], DpiScale::new(dpi).unwrap(), Some(2));
        let mut layouts = TextLayoutCache::new(32, 1024 * 1024);
        let mut nodes = Vec::new();
        let mut next = 1;
        let mut point = None;
        for hovered in [false, true, true, false] {
            nodes.clear();
            screen
                .set_pointer(hovered.then(|| point.expect("the first frame measured the caption")));
            let mut frame = PaintFrame {
                layouts: &mut layouts,
                font: &font,
                metrics,
                solid_page: 0,
                safe_area,
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
                if text == expected_caption));
            let painted = nodes
                .iter()
                .find_map(|node| match node.visual() {
                    UiVisual::Text { layout, .. } => layout.glyphs().first(),
                    _ => None,
                })
                .expect("caption paints text");
            let expected_color = if expected_caption.starts_with('§') {
                ui::BedrockColor::LightPurple
            } else if color == 0 {
                ui::BedrockColor::Base
            } else {
                ui::BedrockColor::Green
            };
            assert_eq!(painted.style.color, expected_color);
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
            if hovered {
                let pointer: UiPoint = point.unwrap();
                let min = nodes
                    .iter()
                    .filter(|node| matches!(node.visual(), UiVisual::Sprite { .. }))
                    .map(|node| node.bounds().min())
                    .fold([f32::INFINITY; 2], |min, point| {
                        [min[0].min(point.x()), min[1].min(point.y())]
                    });
                let offset = ui_engine::engine::tooltip::MOUSE_OFFSET;
                let expected = [
                    pointer.x() - safe_area.left() + offset[0] * screen.scale,
                    pointer.y() - safe_area.top() + offset[1] * screen.scale,
                ];
                for axis in 0..2 {
                    assert!(
                        (min[axis] - expected[axis]).abs() <= 1.0,
                        "tooltip position {min:?}, expected {expected:?}, dpi={dpi}, safe={safe_area:?}"
                    );
                }
            }
            let caption_point = UiPoint::new(
                safe_area.left() + (caption.dest.x as f32 + 4.0) * screen.scale,
                safe_area.top() + (caption.dest.y as f32 + 4.0) * screen.scale,
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

#[test]
fn keyboard_completion_feedback_survives_pointer_over_the_editor() {
    let bounds = UiRect::new(
        UiPoint::new(0.0, 0.0).unwrap(),
        UiPoint::new(100.0, 20.0).unwrap(),
    )
    .unwrap();
    let suggestion = UiRect::new(
        UiPoint::new(0.0, 30.0).unwrap(),
        UiPoint::new(100.0, 50.0).unwrap(),
    )
    .unwrap();
    let screen = ChatScreen {
        pointer: Some(UiPoint::new(10.0, 10.0).unwrap()),
        hover_hits: vec![(bounds, "chat/text_edit_box".into())],
        hits: vec![(
            ChatHit::Suggestion(1),
            suggestion,
            "chat/suggestion[1]".into(),
        )],
        edit_box: Some("chat/text_edit_box".into()),
        ..ChatScreen::default()
    };
    let view = screen.view_state(Some(1));
    assert!(view.is_hovered("chat/suggestion[1]"));
    assert!(view.is_focused("chat/text_edit_box"));
    assert!(view.is_hovered("chat/text_edit_box"));
    assert!(screen.view_state(None).is_hovered("chat/text_edit_box"));
}

#[test]
fn native_links_remain_clickable_inside_hover_message_buttons() {
    let bounds = UiRect::new(
        UiPoint::new(0.0, 0.0).unwrap(),
        UiPoint::new(100.0, 20.0).unwrap(),
    )
    .unwrap();
    let pointer = UiPoint::new(10.0, 10.0).unwrap();
    let screen = ChatScreen {
        pointer: Some(pointer),
        hover_hits: vec![(bounds, "messages/button".into())],
        hits: vec![(ChatHit::Link(0), bounds, "messages/button/caption".into())],
        ..ChatScreen::default()
    };
    assert!(screen.view_state(None).is_hovered("messages/button"));
    assert_eq!(screen.hit_test(pointer), Some(ChatHit::Link(0)));
}
