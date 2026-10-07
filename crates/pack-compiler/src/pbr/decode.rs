use image::{Rgba, RgbaImage};

use super::{
    PBR_REF_LABPBR, PBR_REF_MATERIAL, PBR_REF_OCCLUSION, PBR_REF_SUBSURFACE, PbrFormat,
    PbrNormalFormat,
};

pub(super) fn byte(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

pub(super) fn unit(mut value: [f32; 3]) -> [f32; 3] {
    let length = value.iter().map(|v| v * v).sum::<f32>().sqrt();
    if length < 0.000001 {
        return [0.0, 0.0, 1.0];
    }
    for component in &mut value {
        *component /= length;
    }
    value
}

pub(super) fn normal(
    mut image: RgbaImage,
    format: PbrFormat,
    normal_format: PbrNormalFormat,
) -> (RgbaImage, u32) {
    let lab = format == PbrFormat::LabPbr13;
    let mut flags = 0;
    for pixel in image.pixels_mut() {
        let [r, g, b, a] = pixel.0;
        let x = r as f32 / 127.5 - 1.0;
        let y = (g as f32 / 127.5 - 1.0)
            * if normal_format == PbrNormalFormat::OpenGl {
                -1.0
            } else {
                1.0
            };
        let z = if lab {
            (1.0 - x * x - y * y).max(0.0).sqrt()
        } else {
            (b as f32 / 127.5 - 1.0).max(0.0)
        };
        let value = unit([x, y, z]);
        let ao = if lab { b } else { 255 };
        flags |= u32::from(ao < 255) * PBR_REF_OCCLUSION;
        *pixel = Rgba([
            byte(value[0] * 0.5 + 0.5),
            byte(value[1] * 0.5 + 0.5),
            ao,
            if lab { a } else { 128 },
        ]);
    }
    (image, flags)
}

pub(super) fn specular(mut image: RgbaImage, format: PbrFormat) -> (RgbaImage, u32) {
    let lab = format == PbrFormat::LabPbr13;
    let mut flags = PBR_REF_MATERIAL | u32::from(lab) * PBR_REF_LABPBR;
    for pixel in image.pixels_mut() {
        let [smoothness, green, blue, alpha] = pixel.0;
        let emission = if lab {
            if alpha == 255 {
                0
            } else {
                byte(alpha as f32 / 254.0)
            }
        } else {
            blue
        };
        flags |= u32::from(lab && blue > 64) * PBR_REF_SUBSURFACE;
        *pixel = Rgba([
            green,
            emission,
            255 - smoothness,
            if lab { blue } else { 0 },
        ]);
    }
    (image, flags)
}

pub(super) fn mer(mut image: RgbaImage, subsurface: bool) -> (RgbaImage, u32) {
    for pixel in image.pixels_mut() {
        if !subsurface {
            pixel[3] = 0;
        } else if pixel[3] >= pixel[0] {
            pixel[0] = 0;
        } else {
            pixel[3] = 0;
        }
    }
    (
        image,
        PBR_REF_MATERIAL | u32::from(subsurface) * PBR_REF_SUBSURFACE,
    )
}
