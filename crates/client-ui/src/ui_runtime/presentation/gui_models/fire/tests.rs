use super::*;
use std::sync::Arc;

fn texture() -> ParticleTexture {
    ParticleTexture {
        path: assets::ACTOR_FLAME_TEXTURE.into(),
        width: 16,
        height: 32,
        rgba8: (0..16 * 32)
            .flat_map(|pixel| [pixel as u8, (pixel / 16) as u8, 0, 255])
            .collect::<Vec<_>>()
            .into(),
    }
}

#[test]
fn flame_frames_retain_original_texels_and_animate_without_texture_rebuilds() {
    let mut presentation =
        UiPresentationRuntime::new(crate::ui_runtime::presentation::tests::fixture_font()).unwrap();
    presentation.gui_models.enabled = true;
    let source = texture();
    presentation.set_gui_fire_texture(Some(&source)).unwrap();
    let textures = Arc::clone(&presentation.textures);
    let frames = presentation.gui_models.fire.frames.clone();
    assert_eq!(frames.len(), 2);
    for (frame, icon) in frames.iter().enumerate() {
        let page = &textures.pages()[usize::from(icon.page)];
        let stride = page.dimensions()[0] as usize;
        for row in 0..16 {
            let start = ((usize::from(icon.uv[1]) + row) * stride + usize::from(icon.uv[0])) * 4;
            let original = (frame * 16 * 16 + row * 16) * 4;
            assert_eq!(
                &page.pixels()[start..start + 16 * 4],
                &source.rgba8[original..original + 16 * 4]
            );
        }
    }
    presentation.player_preview_view = player_preview::PreviewView::Hud;
    presentation.gui_models.live_player.fire_size = Some([0.6, 1.8]);
    presentation.gui_models.live_player.outer_y = player_preview::HUD_SWIM_OFFSET;
    for frame in [1, 1, 0, 0, 1] {
        presentation
            .gui_models
            .live_player
            .fire
            .observe((1, 1, 1), true, frames.len());
        assert_eq!(
            presentation.gui_fire_frame().unwrap().texture,
            frames[frame]
        );
        assert_eq!(
            presentation.gui_fire_frame().unwrap().outer_y,
            player_preview::HUD_SWIM_OFFSET
        );
    }
    assert!(Arc::ptr_eq(&textures, &presentation.textures));
    presentation.gui_models.live_player.fire_size = None;
    assert!(presentation.gui_fire_frame().is_none());
    presentation.gui_models.live_player.fire_size = Some([0.6, 1.8]);
    presentation.player_preview_view = player_preview::PreviewView::default();
    assert!(presentation.gui_fire_frame().is_none());
}

#[test]
fn optional_flame_texture_can_be_replaced_removed_or_rejected_without_accumulating_pages() {
    let mut presentation =
        UiPresentationRuntime::new(crate::ui_runtime::presentation::tests::fixture_font()).unwrap();
    let mut texture = texture();
    presentation.set_gui_fire_texture(Some(&texture)).unwrap();
    let pages = presentation.gui_models.pages.len();
    texture.rgba8 = vec![255; texture.rgba8.len()].into();
    presentation.set_gui_fire_texture(Some(&texture)).unwrap();
    assert_eq!(presentation.gui_models.pages.len(), pages);
    assert_eq!(presentation.gui_models.fire.frames.len(), 2);
    texture.height = 17;
    presentation.set_gui_fire_texture(Some(&texture)).unwrap();
    assert!(presentation.gui_models.pages.is_empty());
    assert!(presentation.gui_models.fire.frames.is_empty());
    presentation.set_gui_fire_texture(None).unwrap();
    assert!(presentation.gui_models.pages.is_empty());
}

#[test]
fn fire_playback_holds_when_extinguished_and_resets_when_actor_is_replaced() {
    let mut playback = FirePlayback::default();
    playback.observe((1, 1, 1), true, 32);
    assert_eq!(playback.frame, 1);
    playback.observe((1, 1, 1), false, 32);
    playback.observe((1, 1, 1), false, 32);
    assert_eq!(playback.frame, 1);
    playback.observe((1, 1, 1), true, 32);
    assert_eq!(playback.frame, 1);
    playback.observe((1, 1, 1), true, 32);
    assert_eq!(playback.frame, 2);
    playback.observe((1, 1, 2), true, 32);
    assert_eq!(playback.frame, 1);
}
