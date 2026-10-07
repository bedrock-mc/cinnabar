use assets::TextureMip;
use image::{ImageBuffer, Luma, Rgba, RgbaImage, imageops::FilterType};

use super::{PBR_REF_LABPBR, PbrSurface, decode};

type FloatImage = ImageBuffer<Rgba<f32>, Vec<f32>>;
type ScalarImage = ImageBuffer<Luma<f32>, Vec<f32>>;

#[derive(Clone, Debug)]
pub struct PbrMipLayer {
    pub color: Box<[TextureMip]>,
    pub normal: Box<[TextureMip]>,
    pub material: Box<[TextureMip]>,
    pub flags: u32,
}

struct Working {
    color: FloatImage,
    normal: FloatImage,
    ao: ScalarImage,
    material: FloatImage,
}

fn linear(value: u8) -> f32 {
    let value = value as f32 / 255.0;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn srgb(value: f32) -> u8 {
    let value = value.clamp(0.0, 1.0);
    decode::byte(if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    })
}

fn coverage(image: &RgbaImage) -> f32 {
    image.pixels().filter(|pixel| pixel[3] >= 128).count() as f32
        / (image.width() * image.height()) as f32
}

fn resize(image: FloatImage, side: u32) -> FloatImage {
    if image.width() == side && image.height() == side {
        image
    } else {
        image::imageops::resize(&image, side, side, FilterType::Triangle)
    }
}

fn prepare(surface: &PbrSurface, side: u32) -> Working {
    let color = FloatImage::from_fn(surface.color.width(), surface.color.height(), |x, y| {
        let pixel = surface.color.get_pixel(x, y);
        let alpha = pixel[3] as f32 / 255.0;
        Rgba([
            linear(pixel[0]) * alpha,
            linear(pixel[1]) * alpha,
            linear(pixel[2]) * alpha,
            alpha,
        ])
    });
    let normal = FloatImage::from_fn(surface.normal.width(), surface.normal.height(), |x, y| {
        let pixel = surface.normal.get_pixel(x, y);
        let nx = pixel[0] as f32 / 127.5 - 1.0;
        let ny = pixel[1] as f32 / 127.5 - 1.0;
        let value = decode::unit([nx, ny, (1.0 - nx * nx - ny * ny).max(0.0).sqrt()]);
        Rgba([value[0], value[1], value[2], pixel[3] as f32 / 255.0])
    });
    let ao = ScalarImage::from_fn(surface.normal.width(), surface.normal.height(), |x, y| {
        Luma([surface.normal.get_pixel(x, y)[2] as f32 / 255.0])
    });
    let material = FloatImage::from_fn(
        surface.material.width(),
        surface.material.height(),
        |x, y| {
            let pixel = surface.material.get_pixel(x, y);
            let roughness = pixel[2] as f32 / 255.0;
            Rgba([
                pixel[0] as f32 / 255.0,
                pixel[1] as f32 / 255.0,
                roughness.powi(4),
                pixel[3] as f32 / 255.0,
            ])
        },
    );
    let mut material = resize(material, side);
    if surface.flags & PBR_REF_LABPBR != 0 {
        let nearest = image::imageops::resize(&surface.material, side, side, FilterType::Nearest);
        for (pixel, exact) in material.pixels_mut().zip(nearest.pixels()) {
            pixel[0] = exact[0] as f32 / 255.0;
            pixel[3] = exact[3] as f32 / 255.0;
        }
    }
    Working {
        color: resize(color, side),
        normal: resize(normal, side),
        ao: image::imageops::resize(&ao, side, side, FilterType::Triangle),
        material,
    }
}

fn alpha_scale(image: &FloatImage, wanted: f32) -> f32 {
    if wanted <= 0.0 || wanted >= 1.0 {
        return 1.0;
    }
    let covered = |scale: f32| {
        image
            .pixels()
            .filter(|pixel| pixel[3] * scale >= 0.5)
            .count() as f32
            / (image.width() * image.height()) as f32
    };
    if (covered(1.0) - wanted).abs() <= 0.5 / (image.width() * image.height()) as f32 {
        return 1.0;
    }
    let mut low = 0.0;
    let mut high = 16.0;
    for _ in 0..24 {
        let middle = (low + high) * 0.5;
        if covered(middle) < wanted {
            low = middle;
        } else {
            high = middle;
        }
    }
    if (covered(low) - wanted).abs() <= (covered(high) - wanted).abs() {
        low
    } else {
        high
    }
}

fn encode(
    working: &Working,
    side: u32,
    cutout: bool,
    wanted: f32,
) -> (TextureMip, TextureMip, TextureMip) {
    let scale = if cutout {
        alpha_scale(&working.color, wanted)
    } else {
        1.0
    };
    let mut color = Vec::with_capacity((side * side * 4) as usize);
    let mut normal = Vec::with_capacity(color.capacity());
    let mut material = Vec::with_capacity(color.capacity());
    for (((c, n), ao), mer) in working
        .color
        .pixels()
        .zip(working.normal.pixels())
        .zip(working.ao.pixels())
        .zip(working.material.pixels())
    {
        let opacity = c[3];
        let inverse = if opacity > 0.000001 {
            1.0 / opacity
        } else {
            0.0
        };
        color.extend_from_slice(&[
            srgb(c[0] * inverse),
            srgb(c[1] * inverse),
            srgb(c[2] * inverse),
            decode::byte(opacity * scale),
        ]);
        let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2])
            .sqrt()
            .clamp(0.0001, 1.0);
        let vector = decode::unit([n[0], n[1], n[2]]);
        normal.extend_from_slice(&[
            decode::byte(vector[0] * 0.5 + 0.5),
            decode::byte(vector[1] * 0.5 + 0.5),
            decode::byte(ao[0]),
            decode::byte(n[3]),
        ]);
        let variance = (0.25 * (1.0 - length) / length).min(1.0);
        let roughness = (mer[2] + variance).clamp(0.0, 1.0).powf(0.25);
        material.extend_from_slice(&[
            decode::byte(mer[0]),
            decode::byte(mer[1]),
            decode::byte(roughness),
            decode::byte(mer[3]),
        ]);
    }
    let mip = |rgba8: Vec<u8>| TextureMip {
        size: side,
        rgba8: rgba8.into_boxed_slice(),
    };
    (mip(color), mip(normal), mip(material))
}

fn categorical(values: [f32; 4], threshold: f32) -> f32 {
    let above = values.iter().filter(|value| **value >= threshold).count();
    let take_above = above >= 2;
    let selected = || {
        values
            .iter()
            .copied()
            .filter(|value| (*value >= threshold) == take_above)
    };
    if take_above {
        selected()
            .max_by_key(|candidate| {
                selected()
                    .filter(|value| (*value - *candidate).abs() < 0.001)
                    .count()
            })
            .unwrap_or(values[0])
    } else {
        selected().sum::<f32>() / selected().count().max(1) as f32
    }
}

fn downsample(working: &Working, side: u32, lab: bool) -> Working {
    let coordinates = |x: u32, y: u32| {
        [
            (x * 2, y * 2),
            (x * 2 + 1, y * 2),
            (x * 2, y * 2 + 1),
            (x * 2 + 1, y * 2 + 1),
        ]
    };
    let color = FloatImage::from_fn(side, side, |x, y| {
        let points = coordinates(x, y);
        Rgba(std::array::from_fn(|channel| {
            points
                .iter()
                .map(|&(x, y)| working.color.get_pixel(x, y)[channel] * 0.25)
                .sum()
        }))
    });
    let weighted = |x: u32, y: u32, channel: usize, image: &FloatImage| {
        let points = coordinates(x, y);
        let total = points
            .iter()
            .map(|&(x, y)| working.color.get_pixel(x, y)[3])
            .sum::<f32>();
        if total <= 0.000001 {
            image.get_pixel(x * 2, y * 2)[channel]
        } else {
            points
                .iter()
                .map(|&(x, y)| image.get_pixel(x, y)[channel] * working.color.get_pixel(x, y)[3])
                .sum::<f32>()
                / total
        }
    };
    let normal = FloatImage::from_fn(side, side, |x, y| {
        Rgba(std::array::from_fn(|channel| {
            weighted(x, y, channel, &working.normal)
        }))
    });
    let ao = ScalarImage::from_fn(side, side, |x, y| {
        let points = coordinates(x, y);
        let total = points
            .iter()
            .map(|&(x, y)| working.color.get_pixel(x, y)[3])
            .sum::<f32>();
        Luma([if total <= 0.000001 {
            1.0
        } else {
            points
                .iter()
                .map(|&(x, y)| working.ao.get_pixel(x, y)[0] * working.color.get_pixel(x, y)[3])
                .sum::<f32>()
                / total
        }])
    });
    let material = FloatImage::from_fn(side, side, |x, y| {
        let mut result = std::array::from_fn(|channel| weighted(x, y, channel, &working.material));
        if lab {
            let points = coordinates(x, y);
            result[0] = categorical(
                points.map(|(x, y)| working.material.get_pixel(x, y)[0]),
                230.0 / 255.0,
            );
            let blue = points.map(|(x, y)| working.material.get_pixel(x, y)[3]);
            let sss = blue.iter().filter(|value| **value > 64.0 / 255.0).count() >= 2;
            let selected = || {
                blue.iter()
                    .filter(|value| (**value > 64.0 / 255.0) == sss)
                    .copied()
            };
            result[3] = selected().sum::<f32>() / selected().count().max(1) as f32;
        }
        Rgba(result)
    });
    Working {
        color,
        normal,
        ao,
        material,
    }
}

/// Filters radiance in linear premultiplied space and raises roughness for unresolved normal detail.
pub fn build_pbr_mips(
    surface: &PbrSurface,
    side: u32,
    cutout: bool,
) -> Result<PbrMipLayer, String> {
    if !side.is_power_of_two() || side > assets::PBR_TILE_SIZE {
        return Err("unsupported authored material layer size".to_owned());
    }
    if [
        surface.color.dimensions(),
        surface.normal.dimensions(),
        surface.material.dimensions(),
    ]
    .iter()
    .any(|&(width, height)| width == 0 || width != height)
    {
        return Err("authored mip inputs must be square extracted frames".to_owned());
    }
    let wanted = coverage(&surface.color);
    let mut working = prepare(surface, side);
    let mut size = side;
    let (mut colors, mut normals, mut materials) = (Vec::new(), Vec::new(), Vec::new());
    loop {
        let (color, normal, material) = encode(&working, size, cutout, wanted);
        colors.push(color);
        normals.push(normal);
        materials.push(material);
        if size == 1 {
            break;
        }
        size /= 2;
        working = downsample(&working, size, surface.flags & PBR_REF_LABPBR != 0);
    }
    Ok(PbrMipLayer {
        color: colors.into_boxed_slice(),
        normal: normals.into_boxed_slice(),
        material: materials.into_boxed_slice(),
        flags: surface.flags,
    })
}
