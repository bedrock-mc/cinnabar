//! Gallery art renders the same authored mesh and texture as the equipped attachment.

use cosmetics::Item;

/// Decodes either original texels or a fitted model thumbnail on the artwork worker.
pub(in crate::ui_runtime::presentation) fn decode(
    hat: Item,
    thumbnail: bool,
) -> Option<(Vec<u8>, u32, u32)> {
    let model = render_model::hats::rig(hat);
    if !thumbnail {
        return Some((
            model.texture.rgba8.to_vec(),
            model.texture.width,
            model.texture.height,
        ));
    }
    let [width, height] = render_model::hats::thumbnail::SIZE;
    let image = render_model::hats::thumbnail::render(hat, width, height)?;
    Some((image.rgba8.to_vec(), image.width, image.height))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosmetic_gallery_renders_the_original_black_and_pink_geometry() {
        let (pixels, width, height) = decode(render_model::hats::fixture::bunny(), true).unwrap();
        assert_eq!(pixels.len(), (width * height * 4) as usize);
        assert!(pixels.as_chunks::<4>().0.iter().any(|pixel| pixel[3] == 0));
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] != 0 && u16::from(pixel[0]) > u16::from(pixel[1]) * 3 / 2)
        );
        assert_eq!(
            pixels,
            decode(render_model::hats::fixture::bunny(), true)
                .unwrap()
                .0
        );
    }
}
