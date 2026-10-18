use image::{ImageBuffer, Pixel, imageops::FilterType};

use super::{FloatImage, LABPBR_METAL_START, ScalarImage, categorical};

/// Resizes floating-point channels to the requested square layer size.
pub(super) fn resize<P: Pixel<Subpixel = f32> + 'static>(
    image: ImageBuffer<P, Vec<f32>>,
    side: u32,
) -> ImageBuffer<P, Vec<f32>> {
    if image.width() == side && image.height() == side {
        image
    } else {
        image::imageops::resize(&image, side, side, FilterType::Triangle)
    }
}

/// Filters channels with color opacity aligned to their native resolution, then removes the weight.
pub(super) fn resize_weighted<P: Pixel<Subpixel = f32> + 'static>(
    mut image: ImageBuffer<P, Vec<f32>>,
    opacity: &ScalarImage,
    side: u32,
) -> ImageBuffer<P, Vec<f32>> {
    if image.width() == side {
        return image;
    }
    let weights = resize(opacity.clone(), image.width());
    for (pixel, weight) in image.pixels_mut().zip(weights.pixels()) {
        for channel in pixel.channels_mut() {
            *channel *= weight[0];
        }
    }
    let mut image = resize(image, side);
    let weights = resize(weights, side);
    for (pixel, weight) in image.pixels_mut().zip(weights.pixels()) {
        for channel in pixel.channels_mut() {
            *channel = if weight[0] > 0.000001 {
                *channel / weight[0]
            } else {
                0.0
            };
        }
    }
    image
}

/// Visits the source texels and linear-filter weights contributing to one output coordinate.
fn triangle_samples(
    target: u32,
    source_side: u32,
    target_side: u32,
) -> impl Iterator<Item = (u32, f32)> + Clone {
    let ratio = source_side as f32 / target_side as f32;
    let support = ratio.max(1.0);
    let center = (target as f32 + 0.5) * ratio;
    let start = ((center - support).floor() as i64).clamp(0, source_side as i64 - 1) as u32;
    let end = ((center + support).ceil() as i64).clamp(start as i64 + 1, source_side as i64) as u32;
    (start..end).map(move |index| {
        (
            index,
            (1.0 - ((index as f32 + 0.5 - center) / support).abs()).max(0.0),
        )
    })
}

/// Retains LabPBR categories and exact metal IDs using the same visible filter footprint.
pub(super) fn resize_categories(
    source: &FloatImage,
    opacity: &ScalarImage,
    target: &mut FloatImage,
) {
    if source.dimensions() == target.dimensions() {
        return;
    }
    let opacity = resize(opacity.clone(), source.width());
    let side = target.width();
    for (x, y, pixel) in target.enumerate_pixels_mut() {
        let samples = triangle_samples(y, source.height(), side).flat_map(|(y, wy)| {
            let opacity = &opacity;
            triangle_samples(x, source.width(), side)
                .map(move |(x, wx)| (source.get_pixel(x, y), wx * wy * opacity.get_pixel(x, y)[0]))
        });
        pixel[0] = categorical(
            samples.clone().map(|(pixel, weight)| (pixel[0], weight)),
            |value| value >= LABPBR_METAL_START,
            true,
        );
        pixel[3] = categorical(
            samples.map(|(pixel, weight)| (pixel[3], weight)),
            |value| value > 64.0 / 255.0,
            false,
        );
    }
}
