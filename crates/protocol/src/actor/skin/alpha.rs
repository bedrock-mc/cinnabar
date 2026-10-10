//! Bedrock's non-persona skin alpha validation, before texture packing or resampling.
use valentine::bedrock::version::v1_26_51::SerializedSkinRef;

use render_api::{CLASSIC_SKIN_SIDE, MAX_CLASSIC_SKIN_SIDE};

/// Binarizes supported skin regions and protects classic body sides from excessive transparency.
pub(super) fn normalize(skin: &mut SerializedSkinRef) {
    let image = &mut skin.image_data;
    if skin.is_persona
        || image.width != image.height
        || !(image.width as usize == CLASSIC_SKIN_SIDE
            || image.width as usize == MAX_CLASSIC_SKIN_SIDE)
    {
        return;
    }
    let custom_geometry = serde_json::from_str::<serde_json::Value>(&skin.resource_patch)
        .ok()
        .and_then(|patch| {
            patch
                .get("geometry")?
                .get("default")?
                .as_str()
                .map(str::to_owned)
        })
        .is_some_and(|name| {
            !matches!(
                name.as_str(),
                "geometry.humanoid.custom" | "geometry.humanoid.customSlim"
            )
        });
    normalize_pixels(
        image.width,
        image.height,
        &mut image.image_bytes,
        !custom_geometry,
    );
}

/// Validates classic image size and applies native wide/slim alpha and body-coverage rules.
pub fn normalize_classic_skin_rgba8(width: u32, height: u32, pixels: &mut [u8]) -> bool {
    normalize_pixels(width, height, pixels, true)
}

/// Binarizes custom skin texels without forcing humanoid body coverage.
pub fn normalize_custom_skin_rgba8(width: u32, height: u32, pixels: &mut [u8]) -> bool {
    normalize_pixels(width, height, pixels, false)
}

fn normalize_pixels(width: u32, height: u32, pixels: &mut [u8], protect_body: bool) -> bool {
    if width != height
        || !(width as usize == CLASSIC_SKIN_SIDE || width as usize == MAX_CLASSIC_SKIN_SIDE)
        || pixels.len() != width as usize * height as usize * 4
    {
        return false;
    }
    let side = width as usize;
    let scale = side / CLASSIC_SKIN_SIDE;
    // Vanilla validates alpha and coverage across these skin regions.
    for (bounds, protect) in [
        ([0, 8, 32, 16], true),
        ([8, 0, 24, 8], false),
        ([0, 20, 56, 32], true),
        ([4, 16, 12, 20], false),
        ([20, 16, 36, 20], false),
        ([44, 16, 52, 20], false),
        ([16, 52, 48, 64], true),
        ([20, 48, 28, 64], false),
        ([36, 48, 44, 64], false),
        ([32, 0, 64, 32], false),
        ([0, 32, 16, 48], false),
        ([16, 32, 40, 48], false),
        ([40, 32, 56, 48], false),
        ([0, 48, 16, 64], false),
        ([48, 48, 64, 64], false),
    ] {
        region(
            pixels,
            side,
            bounds.map(|value| value * scale),
            protect && protect_body,
        );
    }
    true
}

/// Applies the 26/255 alpha cutoff and the strict 60% transparent coverage threshold.
fn region(pixels: &mut [u8], side: usize, [x0, y0, x1, y1]: [usize; 4], protect: bool) {
    let mut transparent = 0;
    for y in y0..y1 {
        for x in x0..x1 {
            let alpha = &mut pixels[(y * side + x) * 4 + 3];
            transparent += usize::from(*alpha < 26);
            *alpha = if *alpha < 26 { 0 } else { 255 };
        }
    }
    if protect && transparent as f32 / ((x1 - x0) * (y1 - y0)) as f32 > 0.6 {
        for y in y0..y1 {
            for x in x0..x1 {
                pixels[(y * side + x) * 4 + 3] = 255;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use valentine::bedrock::version::v1_26_51::SkinImage;

    /// Makes a classic raster with transparent body texels and a named resource-patch model.
    fn skin(name: &str, side: u32) -> SerializedSkinRef {
        SerializedSkinRef {
            resource_patch: format!(r#"{{"geometry":{{"default":"{name}"}}}}"#),
            image_data: SkinImage {
                width: side,
                height: side,
                image_bytes: vec![0; (side * side * 4) as usize],
            },
            ..Default::default()
        }
    }

    #[test]
    fn classic_transparent_body_sides_become_visible_without_filling_the_hat() {
        for side in [CLASSIC_SKIN_SIDE as u32, MAX_CLASSIC_SKIN_SIDE as u32] {
            let mut skin = skin("geometry.humanoid.customSlim", side);
            normalize(&mut skin);
            let scale = side as usize / CLASSIC_SKIN_SIDE;
            assert_eq!(
                skin.image_data.image_bytes[((8 * scale * side as usize) * 4) + 3],
                255
            );
            assert_eq!(skin.image_data.image_bytes[(32 * scale * 4) + 3], 0);
        }
    }

    #[test]
    fn custom_geometry_keeps_holes_and_capture_partial_alpha_becomes_opaque() {
        let mut skin = skin("geometry.fixture", MAX_CLASSIC_SKIN_SIDE as u32);
        let index = (64 * 4) + 3;
        skin.image_data.image_bytes[index] = 56;
        normalize(&mut skin);
        assert_eq!(skin.image_data.image_bytes[index], 255);
        assert_eq!(
            skin.image_data.image_bytes[16 * MAX_CLASSIC_SKIN_SIDE * 4 + 3],
            0
        );
    }

    #[test]
    fn persona_alpha_is_not_reinterpreted_as_classic() {
        let mut skin = skin("geometry.humanoid.custom", CLASSIC_SKIN_SIDE as u32);
        skin.is_persona = true;
        skin.image_data.image_bytes[3] = 56;
        let before = skin.clone();
        normalize(&mut skin);
        assert_eq!(skin, before);
    }
}
