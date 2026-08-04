//! Composition of the bounded dynamic UI texture pages.

use std::sync::Arc;

use render::{MAX_UI_TEXTURE_BYTES, MAX_UI_TEXTURE_LAYERS, UiRenderTextureArray};
use sha2::{Digest, Sha256};

use super::{IconRef, UiPresentationRuntime, item_viewmodel, menu_artwork, player_preview};

/// Rebuilds dynamic pages from immutable base assets so refreshed launcher
/// artwork cannot accumulate stale layers or discard the HUD carriers.
pub(super) fn rebuild(runtime: &mut UiPresentationRuntime) {
    let width = runtime.base_textures.width;
    let height = runtime.base_textures.height;
    let Some(layer_bytes) = usize::try_from(width)
        .ok()
        .and_then(|width| width.checked_mul(height as usize))
        .and_then(|pixels| pixels.checked_mul(4))
    else {
        return;
    };

    let mut rgba8 = runtime.base_textures.rgba8.to_vec();
    let mut layers = runtime.base_textures.layers;
    let mut identity = Sha256::new();
    identity.update(runtime.base_texture_identity);
    identity.update(b"cinnabar-dynamic-hud-v4");

    runtime.player_preview_page = None;
    runtime.player_preview_icon = None;
    runtime.left_hand_icon = None;
    runtime.right_hand_icon = None;
    runtime.held_viewmodel_icon = None;
    runtime.offhand_viewmodel_icon = None;
    runtime.menu_artwork = menu_artwork::MenuArtworkAtlas::default();

    let preview_fits = runtime.player_preview_pixels.is_some()
        && width >= player_preview::PREVIEW_WIDTH
        && height >= player_preview::PREVIEW_HEIGHT
        && width >= player_preview::HAND_WIDTH.saturating_mul(2)
        && height >= player_preview::PREVIEW_HEIGHT.saturating_add(player_preview::HAND_HEIGHT)
        && layers < MAX_UI_TEXTURE_LAYERS
        && rgba8.len().saturating_add(layer_bytes) <= MAX_UI_TEXTURE_BYTES;
    let viewmodel_fits = width >= item_viewmodel::MAIN_ORIGIN[0] + item_viewmodel::SIDE
        && height >= item_viewmodel::OFFHAND_ORIGIN[1] + item_viewmodel::SIDE;
    if preview_fits {
        let page = layers as u16;
        let layer_start = rgba8.len();
        rgba8.extend(std::iter::repeat_n(0, layer_bytes));
        layers = layers.saturating_add(1);
        let texture_width = width as usize;
        let copy_raster = |target: &mut [u8],
                           raster: &[u8],
                           origin: [u32; 2],
                           raster_width: u32,
                           raster_height: u32| {
            let raster_width = raster_width as usize;
            let raster_height = raster_height as usize;
            for row in 0..raster_height {
                let source_start = row * raster_width * 4;
                let target_start = layer_start
                    + ((origin[1] as usize + row) * texture_width + origin[0] as usize) * 4;
                target[target_start..target_start + raster_width * 4]
                    .copy_from_slice(&raster[source_start..source_start + raster_width * 4]);
            }
        };
        if let Some(rasters) = runtime.player_preview_pixels.as_ref() {
            for row in 0..player_preview::PREVIEW_HEIGHT as usize {
                let source_start = row * player_preview::PREVIEW_WIDTH as usize * 4;
                let target_start = layer_start + row * texture_width * 4;
                let target_end = target_start + player_preview::PREVIEW_WIDTH as usize * 4;
                rgba8[target_start..target_end].copy_from_slice(
                    &rasters.preview
                        [source_start..source_start + player_preview::PREVIEW_WIDTH as usize * 4],
                );
            }
            copy_raster(
                &mut rgba8,
                &rasters.left_hand,
                [0, player_preview::PREVIEW_HEIGHT],
                player_preview::HAND_WIDTH,
                player_preview::HAND_HEIGHT,
            );
            copy_raster(
                &mut rgba8,
                &rasters.right_hand,
                [player_preview::HAND_WIDTH, player_preview::PREVIEW_HEIGHT],
                player_preview::HAND_WIDTH,
                player_preview::HAND_HEIGHT,
            );
        }
        if viewmodel_fits {
            if let Some(main) = runtime
                .held_viewmodel_source
                .and_then(|icon| item_viewmodel::render(&runtime.base_textures, icon, false))
            {
                copy_raster(
                    &mut rgba8,
                    &main,
                    item_viewmodel::MAIN_ORIGIN,
                    item_viewmodel::SIDE,
                    item_viewmodel::SIDE,
                );
                runtime.held_viewmodel_icon =
                    Some(item_viewmodel::icon_at(page, item_viewmodel::MAIN_ORIGIN));
            }
            if let Some(offhand) = runtime
                .offhand_viewmodel_source
                .and_then(|icon| item_viewmodel::render(&runtime.base_textures, icon, true))
            {
                copy_raster(
                    &mut rgba8,
                    &offhand,
                    item_viewmodel::OFFHAND_ORIGIN,
                    item_viewmodel::SIDE,
                    item_viewmodel::SIDE,
                );
                runtime.offhand_viewmodel_icon = Some(item_viewmodel::icon_at(
                    page,
                    item_viewmodel::OFFHAND_ORIGIN,
                ));
            }
        }
        runtime.player_preview_page = Some(page);
        runtime.player_preview_icon = Some(IconRef {
            page,
            uv: [
                0,
                0,
                player_preview::PREVIEW_WIDTH as u16,
                player_preview::PREVIEW_HEIGHT as u16,
            ],
        });
        runtime.left_hand_icon = Some(IconRef {
            page,
            uv: [
                0,
                player_preview::PREVIEW_HEIGHT as u16,
                player_preview::HAND_WIDTH as u16,
                player_preview::PREVIEW_HEIGHT as u16 + player_preview::HAND_HEIGHT as u16,
            ],
        });
        runtime.right_hand_icon = Some(IconRef {
            page,
            uv: [
                player_preview::HAND_WIDTH as u16,
                player_preview::PREVIEW_HEIGHT as u16,
                player_preview::HAND_WIDTH.saturating_mul(2) as u16,
                player_preview::PREVIEW_HEIGHT as u16 + player_preview::HAND_HEIGHT as u16,
            ],
        });
    }

    let remaining_layers = MAX_UI_TEXTURE_LAYERS.saturating_sub(layers);
    let remaining_bytes = MAX_UI_TEXTURE_BYTES.saturating_sub(rgba8.len());
    runtime.menu_artwork = menu_artwork::load(
        &runtime.menu_artwork_paths,
        width,
        height,
        u16::try_from(layers).unwrap_or(u16::MAX),
        remaining_layers,
        remaining_bytes,
    );
    if runtime.menu_artwork.layers > 0 {
        layers = layers.saturating_add(runtime.menu_artwork.layers);
        rgba8.extend_from_slice(&runtime.menu_artwork.rgba8);
        identity.update(runtime.menu_artwork.signature);
    }
    identity.update(&rgba8);
    runtime.textures = Arc::new(UiRenderTextureArray {
        identity: identity.finalize().into(),
        width,
        height,
        layers,
        rgba8: rgba8.into(),
    });
}
