use assets::{
    FontPixels, FontTexturePage, GlyphMetrics, RuntimeFontCatalog, RuntimeHudCatalog,
    encode_font_catalog,
};
use protocol::{
    BossAction as ProtocolBossAction, BossColor as ProtocolBossColor, BossEvent,
    BossOverlay as ProtocolBossOverlay, BossStyle as ProtocolBossStyle, HudEvent, ObjectiveEvent,
    ScoreAction as ProtocolScoreAction, ScoreEntry as ProtocolScoreEntry, ScoreEvent,
    ScoreIdentity as ProtocolScoreIdentity, TextCategory, TextEvent, TextKind, UiEvent,
};
use render_model::{UiRenderScene, UiRenderStats};
use sha2::{Digest, Sha256};
use ui::BoundedStat;

use super::*;
use crate::ui_runtime::SequencedUiEvent;

mod bed_screen_tests;
mod chat_screen_tests;
mod container_screen_tests;
mod credits_screen_tests;
mod debug_overlay_tests;
pub mod engine_hud_tests;
mod forms_tests;
mod hud_matrix_tests;
mod hud_server_pack_tests;
mod inventory_count_tests;
mod item_name_tests;
mod loading_screen_tests;
mod menu_status_tests;
mod paper_doll_tests;
mod publication_split_tests;
mod retained_hud_tests;
mod retained_menu_tests;
mod safe_area_tests;
mod server_menu_tests;
mod sign_screen_tests;
mod texture_pages;
mod toast_tests;

#[test]
fn missing_local_hud_carrier_never_falls_back_to_numeric_corner_text() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut runtime = UiRuntime::new(1);
    runtime
        .apply(
            &mut player_runtime,
            SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 1,
                local_millis: 0,
                server_tick: None,
                event: UiEvent::Hud(HudEvent::Health { health: 20 }),
            },
        )
        .unwrap();

    let input = presentation
        .build(
            &player_runtime,
            &runtime,
            0,
            [800, 600],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    assert!(input.vertices.is_empty());
    assert!(input.indices.is_empty());
    assert!(input.batches.is_empty());
}

/// An unchanged frame reuses the last draw list; a changed screen or viewport rebuilds it.
#[test]
fn unchanged_frames_skip_tree_layout_and_draw_list() {
    let mut player_runtime = player_state::PlayerState::new(1);
    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    let mut runtime = UiRuntime::new(1);
    runtime.publish_inventory_authority(&mut player_runtime, protocol::InventoryAuthority::Server);
    runtime
        .publish_local_runtime_id(&mut player_runtime, 1, 42)
        .unwrap();
    runtime.toggle_inventory(&mut player_runtime);
    let dpi = DpiScale::new(1.0).unwrap();
    let first = presentation
        .build(&player_runtime, &runtime, 100, [1280, 720], dpi)
        .unwrap();
    assert!(!first.vertices.is_empty(), "the inventory must draw");
    let builds = presentation.tree_builds;
    for now in 101..120 {
        let again = presentation
            .build(&player_runtime, &runtime, now, [1280, 720], dpi)
            .unwrap();
        assert_eq!(again, first);
    }
    assert_eq!(presentation.tree_builds, builds);
    runtime.toggle_inventory(&mut player_runtime);
    let closed = presentation
        .build(&player_runtime, &runtime, 120, [1280, 720], dpi)
        .unwrap();
    assert_ne!(closed.vertices, first.vertices);
    assert_eq!(presentation.tree_builds, builds + 1);
    runtime.toggle_inventory(&mut player_runtime);
    let resized = presentation
        .build(&player_runtime, &runtime, 121, [1920, 1080], dpi)
        .unwrap();
    assert_eq!(resized.viewport_size, [1920, 1080]);
    assert_eq!(presentation.tree_builds, builds + 2);
}

#[test]
fn maximum_page_font_is_rejected_before_appending_the_solid_layer() {
    let font = fixture_font_with_page_count(render_model::MAX_UI_TEXTURE_LAYERS as usize);
    assert!(matches!(
        UiPresentationRuntime::new(font),
        Err(UiPresentationError::InvalidFontTexture)
    ));
}

fn boss_event(
    action: ProtocolBossAction,
    target_entity_id: i64,
    title: &str,
    progress: f32,
    color: ProtocolBossColor,
    overlay: ProtocolBossOverlay,
) -> UiEvent {
    UiEvent::Boss(BossEvent {
        target_entity_id,
        action,
        title: Arc::from(title),
        filtered_title: Arc::from(""),
        progress,
        style: ProtocolBossStyle {
            color,
            overlay,
            darken_sky: None,
            create_world_fog: None,
        },
    })
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
                pixels: FontPixels::Rgba8(pixels),
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

pub use crate::test_support::{fixture_font, fixture_hud};
