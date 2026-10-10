use image::{ImageBuffer, Rgba, RgbaImage};

use super::{PBR_REF_HEIGHT, PBR_REF_NORMAL, PbrSurface, decode};

/// Validates a square frame or strip and returns its number of frames.
fn frame_count(image: &RgbaImage) -> Result<u32, String> {
    let width = image.width();
    let height = image.height();
    let side = width.min(height);
    if side == 0 || !width.max(height).is_multiple_of(side) {
        return Err(format!(
            "material image {width}x{height} is not a square or frame strip"
        ));
    }
    Ok(width.max(height) / side)
}

fn image_frame(
    image: &RgbaImage,
    index: usize,
    count: usize,
    timeline: &[u32],
    color_frames: u32,
) -> Result<RgbaImage, String> {
    let frames = frame_count(image)?;
    let width = image.width();
    let height = image.height();
    let side = width.min(height);
    let frame = if frames == 1 {
        0
    } else if !timeline.is_empty() && frames == color_frames {
        let timeline_index = index.saturating_mul(timeline.len()) / count.max(1);
        *timeline.get(timeline_index).unwrap_or(&timeline[0])
    } else {
        (index.saturating_mul(frames as usize) / count.max(1)) as u32
    };
    if frame >= frames {
        return Err(format!(
            "material animation frame {frame} exceeds {frames} source frames"
        ));
    }
    let x = if width > height { frame * side } else { 0 };
    let y = if height > width { frame * side } else { 0 };
    Ok(image::imageops::crop_imm(image, x, y, side, side).to_image())
}

pub(super) fn extract(
    source: &PbrSurface,
    height_source: Option<&RgbaImage>,
    timeline: &[u32],
    index: usize,
    count: usize,
) -> Result<PbrSurface, String> {
    let color_frames = frame_count(&source.color)?;
    let color = image_frame(&source.color, index, count, timeline, color_frames)?;
    let mut normal = image_frame(&source.normal, index, count, timeline, color_frames)?;
    let material = image_frame(&source.material, index, count, timeline, color_frames)?;
    let mut flags = source.flags;
    if let Some(height) = height_source {
        let height = image_frame(height, index, count, timeline, color_frames)?;
        let side = height.width();
        let has_normal = flags & PBR_REF_NORMAL != 0;
        if !has_normal {
            normal = ImageBuffer::from_pixel(side, side, Rgba([128, 128, 255, 128]));
        }
        let aligned_height = image::imageops::resize(
            &height,
            normal.width(),
            normal.height(),
            image::imageops::FilterType::Triangle,
        );
        for (x, y, pixel) in normal.enumerate_pixels_mut() {
            pixel[3] = aligned_height.get_pixel(x, y)[0];
        }
        if !has_normal {
            derive_height_normals(&mut normal);
        }
    }
    let first_height = normal.get_pixel(0, 0)[3];
    if normal.pixels().any(|pixel| pixel[3] != first_height) {
        flags |= PBR_REF_HEIGHT | PBR_REF_NORMAL;
    } else {
        flags &= !PBR_REF_HEIGHT;
    }
    Ok(PbrSurface {
        color,
        normal,
        material,
        flags,
    })
}

fn derive_height_normals(normal: &mut RgbaImage) {
    let side = normal.width();
    let heights = normal.pixels().map(|pixel| pixel[3]).collect::<Vec<_>>();
    let height = |x: u32, y: u32| heights[(y * side + x) as usize] as f32 / 255.0;
    for (x, y, pixel) in normal.enumerate_pixels_mut() {
        let left = height((x + side - 1) % side, y);
        let right = height((x + 1) % side, y);
        let above = height(x, (y + side - 1) % side);
        let below = height(x, (y + 1) % side);
        let strength = assets::PBR_HEIGHT_SCALE * side as f32 * 0.5;
        let value = decode::unit([(left - right) * strength, (above - below) * strength, 1.0]);
        pixel[0] = decode::byte(value[0] * 0.5 + 0.5);
        pixel[1] = decode::byte(value[1] * 0.5 + 0.5);
    }
}
