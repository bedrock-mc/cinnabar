use assets::{
    FontTexturePage, GlyphMetrics, HudTexture, HudTextureRole, RuntimeFontCatalog,
    RuntimeHudCatalog, encode_font_catalog, encode_hud_catalog,
};
use protocol::{
    BossAction as ProtocolBossAction, BossColor as ProtocolBossColor, BossEvent,
    BossOverlay as ProtocolBossOverlay, BossStyle as ProtocolBossStyle, HudEvent, ObjectiveEvent,
    ScoreAction as ProtocolScoreAction, ScoreEntry as ProtocolScoreEntry, ScoreEvent,
    ScoreIdentity as ProtocolScoreIdentity, TextCategory, TextEvent, TextKind, UiEvent,
};
use render::{UiRenderScene, UiRenderStats};
use sha2::{Digest, Sha256};
use ui::BoundedStat;

use super::*;
use crate::ui_runtime::SequencedUiEvent;

mod bed_screen_tests;
mod chat_screen_tests;
mod container_screen_tests;
mod debug_overlay_tests;
pub(crate) mod engine_hud_tests;
mod forms_tests;
mod gui_scale_settings_tests;
mod hud_matrix_tests;
mod hud_server_pack_tests;
mod inventory_count_tests;
mod item_pipeline_tests;
mod loading_screen_tests;
mod menu_caret_tests;
mod menu_status_tests;
mod paper_doll_tests;
mod publication_split_tests;
mod retained_hud_tests;
mod safe_area_tests;
mod sign_screen_tests;
mod texture_pages;
mod toast_tests;

#[test]
fn missing_local_hud_carrier_never_falls_back_to_numeric_corner_text() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

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

#[test]
fn maximum_page_font_is_rejected_before_appending_the_solid_layer() {
    let font = fixture_font_with_page_count(render::MAX_UI_TEXTURE_LAYERS as usize);
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

/// Models the shipped atlas: ink 16 texels tall sitting 14 above the baseline
/// with a 12-texel advance, so layout geometry in these tests matches what the
/// client actually lays out. A fixture whose glyphs hang below the baseline
/// instead makes every row report far more height than it occupies.
pub(crate) fn fixture_font() -> Arc<RuntimeFontCatalog> {
    let pixels = vec![255; 16 * 16 * 4].into_boxed_slice();
    let page = FontTexturePage {
        source_path: "font/page.png".into(),
        source_bytes: pixels.len() as u32,
        source_sha256: [1; 32],
        pixels_sha256: Sha256::digest(&pixels).into(),
        width: 16,
        height: 16,
        rgba8: pixels,
    };
    let glyphs = ['/', '0', '2', '\u{fffd}'].map(|codepoint| GlyphMetrics {
        codepoint,
        page: 0,
        uv: [0, 0, 12, 16],
        bearing: [0, -14],
        advance_64: 12 * 64,
    });
    let manifest = [7; 32];
    let bytes = encode_font_catalog(manifest, &glyphs, &[page]).unwrap();
    Arc::new(RuntimeFontCatalog::decode(&bytes, manifest).unwrap())
}

pub(crate) fn fixture_hud() -> Arc<RuntimeHudCatalog> {
    let textures = HudTextureRole::ALL
        .into_iter()
        .map(|role| {
            let [width, height] = role.expected_size();
            let rgba8 = [role as u8, 2, 3, 255]
                .repeat(width as usize * height as usize)
                .into_boxed_slice();
            HudTexture {
                role,
                source_bytes: rgba8.len() as u32,
                source_sha256: Sha256::digest(&rgba8).into(),
                pixels_sha256: Sha256::digest(&rgba8).into(),
                width,
                height,
                rgba8,
            }
        })
        .collect::<Vec<_>>();
    let manifest = assets::HUD_SOURCE_MANIFEST_SHA256;
    let bytes = encode_hud_catalog(manifest, &textures).unwrap();
    Arc::new(RuntimeHudCatalog::decode(&bytes).unwrap())
}
