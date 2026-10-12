//! Prepares pack masks and End sky pixels before the accepted reload reaches the scene.

use assets::weather_textures::{END_SKY_TEXTURE_PATH, WeatherImage};
use pack_runtime::pack_textures::{DecodedTexture, decode_pack_texture};
use render_scene::{ScreenOverlayTextures, WeatherTextureAssets};
use resource_pack::LayeredPackView;
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// Resolves the active End sky at its source dimensions, falling back to startup assets.
pub(super) fn prepare_weather(
    view: &LayeredPackView,
    base: &WeatherTextureAssets,
) -> WeatherTextureAssets {
    let Some(image) = decode_pack_texture(view, END_SKY_TEXTURE_PATH) else {
        return base.clone();
    };
    let mut digest = Sha256::new();
    digest.update(base.identity());
    digest.update(END_SKY_TEXTURE_PATH);
    digest.update(image.width.to_le_bytes());
    digest.update(image.height.to_le_bytes());
    digest.update(&image.rgba8);
    base.clone().with_end_sky(
        WeatherImage {
            width: image.width,
            height: image.height,
            rgba8: image.rgba8,
        },
        digest.finalize().into(),
    )
}

/// Resolves each mask independently; removal always reads the immutable startup fallback.
pub(super) fn prepare_screen_overlays(
    view: &LayeredPackView,
    base: Option<&Arc<ScreenOverlayTextures>>,
) -> Option<Arc<ScreenOverlayTextures>> {
    use render_scene::screen_overlay::{PUMPKIN_MASK_PATH, SPYGLASS_MASK_PATH};
    let pumpkin =
        decode_pack_texture(view, PUMPKIN_MASK_PATH.trim_end_matches(".png")).and_then(mask_pixels);
    let spyglass = decode_pack_texture(view, SPYGLASS_MASK_PATH.trim_end_matches(".png"))
        .and_then(mask_pixels);
    if pumpkin.is_none() && spyglass.is_none() {
        return base.cloned();
    }
    let pumpkin = pumpkin.or_else(|| base.and_then(|base| base.pumpkin()).map(<[u8]>::to_vec));
    let spyglass = spyglass.or_else(|| base.and_then(|base| base.spyglass()).map(<[u8]>::to_vec));
    ScreenOverlayTextures::from_layers(pumpkin, spyglass).map(Arc::new)
}

/// Normalizes an admitted mask to the existing GPU layer size without changing its alpha.
fn mask_pixels(image: DecodedTexture) -> Option<Vec<u8>> {
    let side = render_scene::SCREEN_OVERLAY_TEXTURE_SIDE;
    let image = image::RgbaImage::from_raw(image.width, image.height, image.rgba8.into_vec())?;
    Some(if image.dimensions() == (side, side) {
        image.into_raw()
    } else {
        image::imageops::resize(&image, side, side, image::imageops::FilterType::Nearest).into_raw()
    })
}

#[cfg(test)]
mod tests;
