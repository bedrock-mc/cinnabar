use assets::{FontTexturePage, GlyphMetrics, RuntimeFontCatalog, encode_font_catalog};
use protocol::{HudEvent, TextCategory, TextEvent, TextKind, UiEvent};
use sha2::{Digest, Sha256};

use super::*;
use crate::ui_runtime::SequencedUiEvent;

#[test]
fn retained_hud_publishes_through_tree_adapter_and_render_scene() {
    let font = fixture_font();
    let mut presentation = UiPresentationRuntime::new(font).unwrap();
    let mut runtime = UiRuntime::new(1);
    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 1,
            local_millis: 0,
            server_tick: None,
            event: UiEvent::Hud(HudEvent::Health { health: 20 }),
        })
        .unwrap();

    let input = presentation
        .build(&runtime, 0, [800, 600], DpiScale::new(1.0).unwrap())
        .unwrap();
    assert!(!input.vertices.is_empty());
    assert!(!input.indices.is_empty());
    assert!(!input.batches.is_empty());
    let mut scene = UiRenderScene::default();
    scene.publish(input, &UiRenderStats::default()).unwrap();
    assert!(scene.input.is_some());
}

#[test]
fn focused_chat_editor_history_and_suggestions_are_presented() {
    let font = fixture_font();
    let font_page_count = u32::try_from(font.pages().len()).unwrap();
    let mut presentation = UiPresentationRuntime::new(font).unwrap();
    let mut runtime = UiRuntime::new(1);
    let empty = presentation
        .build(&runtime, 0, [800, 600], DpiScale::new(1.0).unwrap())
        .unwrap();
    assert!(
        empty
            .batches
            .iter()
            .all(|batch| batch.texture_page != font_page_count)
    );
    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 1,
            local_millis: 0,
            server_tick: None,
            event: chat_event(&"history ".repeat(12)),
        })
        .unwrap();
    runtime.open_chat();
    runtime.insert_chat_text("/g").unwrap();
    let autocomplete_request = runtime.take_chat_autocomplete_request().unwrap();
    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 2,
            local_millis: 0,
            server_tick: None,
            event: UiEvent::ChatAutocomplete(protocol::ChatAutocompleteEvent {
                enum_name: Arc::from("commands"),
                action: protocol::ChatAutocompleteAction::Replace,
                suggestions: Arc::from(
                    (0..MAX_PRESENTED_CHAT_SUGGESTIONS)
                        .map(|index| Arc::from(format!("/give-{index}")))
                        .collect::<Vec<_>>(),
                ),
            }),
        })
        .unwrap();
    assert!(runtime.complete_chat_autocomplete(autocomplete_request));

    let active = presentation
        .build(&runtime, 0, [800, 600], DpiScale::new(1.0).unwrap())
        .unwrap();

    assert!(active.vertices.len() > empty.vertices.len());
    assert!(active.indices.len() > empty.indices.len());
    assert_eq!(
        active.batches.first().map(|batch| batch.texture_page),
        Some(font_page_count),
        "the translucent surface must draw behind history and suggestion text"
    );
}

#[test]
fn focused_chat_editor_uses_a_dedicated_solid_panel_layer() {
    let font = fixture_font();
    let font_page_count = u32::try_from(font.pages().len()).unwrap();
    let mut presentation = UiPresentationRuntime::new(font).unwrap();
    let mut runtime = UiRuntime::new(1);
    runtime.open_chat();
    runtime.insert_chat_text("hello").unwrap();

    let active = presentation
        .build(&runtime, 0, [800, 600], DpiScale::new(1.0).unwrap())
        .unwrap();

    assert_eq!(active.textures.layers, font_page_count + 1);
    let panel_batch = active
        .batches
        .iter()
        .find(|batch| batch.texture_page == font_page_count)
        .expect("focused chat must draw through the dedicated solid panel layer");
    let panel_indices = &active.indices[panel_batch.first_index as usize
        ..(panel_batch.first_index + panel_batch.index_count) as usize];
    let panel_vertices = panel_indices
        .iter()
        .map(|index| active.vertices[*index as usize])
        .collect::<Vec<_>>();
    let layer_bytes = active.textures.width as usize
        * active.textures.height as usize
        * std::mem::size_of::<[u8; 4]>();
    let solid_start = font_page_count as usize * layer_bytes;
    assert!(
        active.textures.rgba8[solid_start..solid_start + layer_bytes]
            .iter()
            .all(|byte| *byte == 255)
    );
    assert!(
        panel_vertices
            .iter()
            .all(|vertex| vertex.color == [0, 0, 0, 176])
    );
    assert!(panel_vertices.iter().any(|vertex| vertex.color[3] >= 128));
    assert!(
        panel_vertices
            .iter()
            .any(|vertex| vertex.position[0] <= 8.0)
    );
    assert!(
        panel_vertices
            .iter()
            .any(|vertex| vertex.position[0] >= 370.0)
    );
    assert!(
        panel_vertices
            .iter()
            .all(|vertex| vertex.position[1] <= 558.0)
    );
    assert!(
        panel_vertices
            .iter()
            .all(|vertex| vertex.position[1] >= 500.0)
    );
}

#[test]
fn focused_chat_uses_compact_java_style_text_and_does_not_dim_the_hud() {
    let font = fixture_font();
    let solid_page = u32::try_from(font.pages().len()).unwrap();
    let mut presentation = UiPresentationRuntime::new(font).unwrap();
    let mut runtime = UiRuntime::new(1);
    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 1,
            local_millis: 0,
            server_tick: None,
            event: chat_event("compact history"),
        })
        .unwrap();
    runtime.open_chat();

    let active = presentation
        .build(&runtime, 0, [800, 600], DpiScale::new(1.0).unwrap())
        .unwrap();
    let panel = active
        .batches
        .iter()
        .find(|batch| batch.texture_page == solid_page)
        .unwrap();
    let panel_vertices = active.indices
        [panel.first_index as usize..(panel.first_index + panel.index_count) as usize]
        .iter()
        .map(|index| active.vertices[*index as usize])
        .collect::<Vec<_>>();
    let (panel_top, panel_bottom) = vertical_bounds(&panel_vertices);
    assert!(panel_bottom <= 558.0, "panel dimmed the bottom HUD band");
    assert!(
        panel_bottom - panel_top <= 60.0,
        "single-row chat used an oversized {panel_top}..{panel_bottom} surface"
    );

    for glyph in active.vertices[4..].chunks_exact(4) {
        let (top, bottom) = vertical_bounds(glyph);
        assert!(
            bottom - top <= 12.0,
            "chat glyph exceeded the approved compact scale: {top}..{bottom}"
        );
    }
}

#[test]
fn wrapped_chat_messages_reserve_their_full_visual_height() {
    let font = fixture_font();
    let mut presentation = UiPresentationRuntime::new(font).unwrap();
    let mut runtime = UiRuntime::new(1);
    let first = "a".repeat(70);
    let second = "b".repeat(70);
    for (fifo_sequence, message) in [(1, first.as_str()), (2, second.as_str())] {
        runtime
            .apply(SequencedUiEvent {
                session_id: 1,
                fifo_sequence,
                local_millis: 0,
                server_tick: None,
                event: chat_event(message),
            })
            .unwrap();
    }

    let active = presentation
        .build(&runtime, 0, [800, 600], DpiScale::new(1.0).unwrap())
        .unwrap();
    let first_vertex_count = first.chars().count() * 4;
    let first_bottom = active.vertices[..first_vertex_count]
        .iter()
        .map(|vertex| vertex.position[1])
        .fold(f32::NEG_INFINITY, f32::max);
    let second_top = active.vertices[first_vertex_count..]
        .iter()
        .map(|vertex| vertex.position[1])
        .fold(f32::INFINITY, f32::min);

    assert!(
        first_bottom <= second_top,
        "wrapped chat rows overlap: first bottom {first_bottom}, second top {second_top}"
    );
}

#[test]
fn chat_surface_uses_bounded_width_across_resize_and_dpi() {
    let font = fixture_font();
    let solid_page = u32::try_from(font.pages().len()).unwrap();
    let mut presentation = UiPresentationRuntime::new(font).unwrap();
    let mut runtime = UiRuntime::new(1);
    runtime.open_chat();

    for (physical_size, dpi, maximum_panel_right) in [
        ([3_840, 1_080], 1.0, 656.0),
        ([1_600, 1_200], 2.0, 752.0),
        ([320, 200], 1.0, 320.0),
    ] {
        let active = presentation
            .build(&runtime, 0, physical_size, DpiScale::new(dpi).unwrap())
            .unwrap();
        let panel = active
            .batches
            .iter()
            .find(|batch| batch.texture_page == solid_page)
            .unwrap();
        let right = active.indices
            [panel.first_index as usize..(panel.first_index + panel.index_count) as usize]
            .iter()
            .map(|index| active.vertices[*index as usize].position[0])
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(right <= maximum_panel_right, "panel right edge was {right}");
    }
}

#[test]
fn focused_chat_editor_does_not_overlap_bottom_hud_text() {
    let font = fixture_font();
    let mut presentation = UiPresentationRuntime::new(font).unwrap();
    let mut runtime = UiRuntime::new(1);
    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 1,
            local_millis: 0,
            server_tick: None,
            event: UiEvent::Hud(HudEvent::Health { health: 20 }),
        })
        .unwrap();
    runtime.open_chat();

    let active = presentation
        .build(&runtime, 0, [800, 600], DpiScale::new(1.0).unwrap())
        .unwrap();
    let hud_vertices = &active.vertices[.."20/20".chars().count() * 4];
    let editor_vertex_count = "> |".chars().count() * 4;
    let editor_vertices = &active.vertices[active.vertices.len() - editor_vertex_count..];
    let hud_top = hud_vertices
        .iter()
        .map(|vertex| vertex.position[1])
        .fold(f32::INFINITY, f32::min);
    let editor_bottom = editor_vertices
        .iter()
        .map(|vertex| vertex.position[1])
        .fold(f32::NEG_INFINITY, f32::max);

    assert!(
        editor_bottom <= hud_top,
        "chat editor overlaps HUD: editor bottom {editor_bottom}, HUD top {hud_top}"
    );
}

#[test]
fn autocomplete_rows_reserve_actual_text_height_above_editor_and_history() {
    let font = fixture_font();
    let mut presentation = UiPresentationRuntime::new(font).unwrap();
    let mut runtime = UiRuntime::new(1);
    let history = "history ".repeat(12);
    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 1,
            local_millis: 0,
            server_tick: None,
            event: chat_event(&history),
        })
        .unwrap();
    runtime.open_chat();
    runtime.insert_chat_text("/g").unwrap();
    let autocomplete_request = runtime.take_chat_autocomplete_request().unwrap();
    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 2,
            local_millis: 0,
            server_tick: None,
            event: UiEvent::ChatAutocomplete(protocol::ChatAutocompleteEvent {
                enum_name: Arc::from("commands"),
                action: protocol::ChatAutocompleteAction::Replace,
                suggestions: Arc::from(
                    (0..MAX_PRESENTED_CHAT_SUGGESTIONS)
                        .map(|index| Arc::from(format!("/give-{index}")))
                        .collect::<Vec<_>>(),
                ),
            }),
        })
        .unwrap();
    assert!(runtime.complete_chat_autocomplete(autocomplete_request));

    let active = presentation
        .build(&runtime, 0, [800, 600], DpiScale::new(1.0).unwrap())
        .unwrap();
    let panel_vertices = 4;
    let history_vertices = history.chars().count() * 4;
    let editor_vertices = "> /g|".chars().count() * 4;
    let suggestion_vertices = "> /give-0".chars().count() * 4;
    let history_bounds =
        vertical_bounds(&active.vertices[panel_vertices..panel_vertices + history_vertices]);
    let editor_start = panel_vertices + history_vertices;
    let editor_bounds =
        vertical_bounds(&active.vertices[editor_start..editor_start + editor_vertices]);
    let first_suggestion_start = editor_start + editor_vertices;
    let first_suggestion_bounds = vertical_bounds(
        &active.vertices[first_suggestion_start..first_suggestion_start + suggestion_vertices],
    );
    let second_suggestion_bounds = vertical_bounds(
        &active.vertices[first_suggestion_start + suggestion_vertices
            ..first_suggestion_start + suggestion_vertices * 2],
    );

    assert!(first_suggestion_bounds.1 <= editor_bounds.0);
    assert!(second_suggestion_bounds.1 <= first_suggestion_bounds.0);
    let topmost_start =
        first_suggestion_start + suggestion_vertices * (MAX_PRESENTED_CHAT_SUGGESTIONS - 1);
    let topmost_bounds =
        vertical_bounds(&active.vertices[topmost_start..topmost_start + suggestion_vertices]);
    assert!(history_bounds.1 <= topmost_bounds.0);
}

#[test]
fn oversized_latest_chat_message_keeps_a_bounded_visible_portion() {
    let font = fixture_font();
    let mut presentation = UiPresentationRuntime::new(font).unwrap();
    let mut runtime = UiRuntime::new(1);
    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 1,
            local_millis: 0,
            server_tick: None,
            event: chat_event(&"latest ".repeat(60)),
        })
        .unwrap();

    let active = presentation
        .build(&runtime, 0, [800, 600], DpiScale::new(1.0).unwrap())
        .unwrap();
    assert!(!active.vertices.is_empty());
    assert!(
        active
            .vertices
            .iter()
            .all(|vertex| vertex.position[1] >= 380.0 && vertex.position[1] <= 528.0),
        "oversized message escaped bounded presentation region"
    );
}

#[test]
fn maximum_page_font_is_rejected_before_appending_the_solid_layer() {
    let font = fixture_font_with_page_count(MAX_UI_TEXTURE_LAYERS as usize);
    assert!(matches!(
        UiPresentationRuntime::new(font),
        Err(UiPresentationError::InvalidFontTexture)
    ));
}

#[test]
fn suggestion_window_keeps_the_selected_row_visible() {
    assert_eq!(visible_suggestion_range(20, Some(12)), 5..13);
    assert_eq!(visible_suggestion_range(20, Some(19)), 12..20);
    assert_eq!(visible_suggestion_range(3, Some(2)), 0..3);
}

#[test]
fn suggestion_hit_testing_uses_the_exact_rendered_rows_and_width_cap() {
    let font = fixture_font();
    let mut presentation = UiPresentationRuntime::new(font).unwrap();
    let mut runtime = UiRuntime::new(1);
    runtime.open_chat();
    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 1,
            local_millis: 0,
            server_tick: None,
            event: UiEvent::ChatAutocomplete(protocol::ChatAutocompleteEvent {
                enum_name: Arc::from("commands"),
                action: protocol::ChatAutocompleteAction::Replace,
                suggestions: Arc::from(
                    (0..12)
                        .map(|index| Arc::from(format!("/s{index}")))
                        .collect::<Vec<_>>(),
                ),
            }),
        })
        .unwrap();
    runtime.insert_chat_text("/").unwrap();
    assert!(runtime.service_pending_chat_autocomplete());
    for _ in 0..10 {
        runtime.handle_chat_ui_action(ui::UiAction::Navigate([0, 1]));
    }

    let active = presentation
        .build(&runtime, 0, [800, 600], DpiScale::new(1.0).unwrap())
        .unwrap();
    assert_eq!(
        presentation
            .chat_suggestion_hits
            .iter()
            .map(|(index, _)| *index)
            .collect::<Vec<_>>(),
        (3..11).collect::<Vec<_>>()
    );
    for (index, bounds) in &presentation.chat_suggestion_hits {
        let center = UiPoint::new(
            (bounds.min().x() + bounds.max().x()) * 0.5,
            (bounds.min().y() + bounds.max().y()) * 0.5,
        )
        .unwrap();
        assert_eq!(
            presentation.hit_test_chat_suggestion(center, [800.0, 600.0]),
            Some(*index)
        );
        assert!(active.vertices[4..].iter().any(|vertex| {
            bounds.contains(UiPoint::new(vertex.position[0], vertex.position[1]).unwrap())
        }));
    }
    assert_eq!(
        presentation.hit_test_chat_suggestion(UiPoint::new(20.0, 533.0).unwrap(), [800.0, 600.0],),
        Some(3),
        "the old synthetic hit test incorrectly selected row 4 here"
    );

    presentation
        .build(&runtime, 0, [3_840, 1_080], DpiScale::new(1.0).unwrap())
        .unwrap();
    let row_center_y = presentation.chat_suggestion_hits[0].1.min().y()
        + presentation.chat_suggestion_hits[0].1.height() * 0.5;
    assert_eq!(
        presentation.hit_test_chat_suggestion(
            UiPoint::new(1_000.0, row_center_y).unwrap(),
            [3_840.0, 1_080.0],
        ),
        None,
        "ultrawide hit testing escaped the rendered 640px chat cap"
    );
    assert_eq!(
        presentation
            .hit_test_chat_suggestion(UiPoint::new(20.0, row_center_y).unwrap(), [800.0, 600.0],),
        None,
        "stale viewport geometry must fail closed"
    );
}

fn chat_event(message: &str) -> UiEvent {
    UiEvent::Text(TextEvent {
        category: TextCategory::MessageOnly,
        kind: TextKind::Chat,
        needs_translation: false,
        source: None,
        message: Arc::from(message),
        parameters: Arc::from([]),
        xuid: Arc::from(""),
        platform_chat_id: Arc::from(""),
        filtered_message: None,
    })
}

fn vertical_bounds(vertices: &[render::UiRenderVertex]) -> (f32, f32) {
    vertices.iter().fold(
        (f32::INFINITY, f32::NEG_INFINITY),
        |(top, bottom), vertex| (top.min(vertex.position[1]), bottom.max(vertex.position[1])),
    )
}

fn fixture_font_with_page_count(page_count: usize) -> Arc<RuntimeFontCatalog> {
    let pages = (0..page_count)
        .map(|index| {
            let pixels = vec![index as u8, (index >> 8) as u8, 255, 255].into_boxed_slice();
            let mut source_sha256 = [1; 32];
            source_sha256[..8].copy_from_slice(&(index as u64).to_le_bytes());
            FontTexturePage {
                source_path: format!("font/page-{index:03}.png").into(),
                source_bytes: 4,
                source_sha256,
                pixels_sha256: Sha256::digest(&pixels).into(),
                width: 1,
                height: 1,
                rgba8: pixels,
            }
        })
        .collect::<Vec<_>>();
    let glyph = GlyphMetrics {
        codepoint: '\u{fffd}',
        page: 0,
        uv: [0, 0, 1, 1],
        bearing: [0, 0],
        advance_64: 64,
    };
    let manifest = [9; 32];
    let bytes = encode_font_catalog(manifest, &[glyph], &pages).unwrap();
    Arc::new(RuntimeFontCatalog::decode(&bytes, manifest).unwrap())
}

fn fixture_font() -> Arc<RuntimeFontCatalog> {
    let pixels = vec![255; 16 * 24 * 4].into_boxed_slice();
    let page = FontTexturePage {
        source_path: "font/page.png".into(),
        source_bytes: pixels.len() as u32,
        source_sha256: [1; 32],
        pixels_sha256: Sha256::digest(&pixels).into(),
        width: 16,
        height: 24,
        rgba8: pixels,
    };
    let glyphs = ['/', '0', '2', '\u{fffd}'].map(|codepoint| GlyphMetrics {
        codepoint,
        page: 0,
        uv: [0, 0, 16, 24],
        bearing: [0, 0],
        advance_64: 512,
    });
    let manifest = [7; 32];
    let bytes = encode_font_catalog(manifest, &glyphs, &[page]).unwrap();
    Arc::new(RuntimeFontCatalog::decode(&bytes, manifest).unwrap())
}
