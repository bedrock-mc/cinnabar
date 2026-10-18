use assets::texture::{PBR_REF_LABPBR, TextureMip};
use image::{ImageBuffer, Luma, Rgba, RgbaImage};

use super::{PbrSurface, decode};

mod normalization;
use normalization::{resize, resize_categories, resize_weighted};

type FloatImage = ImageBuffer<Rgba<f32>, Vec<f32>>;
type ScalarImage = ImageBuffer<Luma<f32>, Vec<f32>>;
const LABPBR_METAL_START: f32 = 230.0 / 255.0;

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

/// Converts an sRGB byte to linear intensity.
fn linear(value: u8) -> f32 {
    let value = value as f32 / 255.0;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

/// Encodes a bounded linear intensity as an sRGB byte.
fn srgb(value: f32) -> u8 {
    let value = value.clamp(0.0, 1.0);
    decode::byte(if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    })
}

/// Measures the fraction of texels passing the cutout alpha threshold.
fn coverage(image: &RgbaImage) -> f32 {
    image.pixels().filter(|pixel| pixel[3] >= 128).count() as f32
        / (image.width() * image.height()) as f32
}

/// Converts color, normals and material channels into their filtering spaces.
fn prepare(surface: &PbrSurface, side: u32, overlay_mask: bool) -> Working {
    let opacity = ScalarImage::from_fn(surface.color.width(), surface.color.height(), |x, y| {
        Luma([if overlay_mask {
            1.0
        } else {
            surface.color.get_pixel(x, y)[3] as f32 / 255.0
        }])
    });
    let color = FloatImage::from_fn(surface.color.width(), surface.color.height(), |x, y| {
        let pixel = surface.color.get_pixel(x, y);
        let alpha = pixel[3] as f32 / 255.0;
        let weight = if overlay_mask { 1.0 } else { alpha };
        Rgba([
            linear(pixel[0]) * weight,
            linear(pixel[1]) * weight,
            linear(pixel[2]) * weight,
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
    let mut resized_material = resize_weighted(material.clone(), &opacity, side);
    if surface.flags & PBR_REF_LABPBR != 0 {
        resize_categories(&material, &opacity, &mut resized_material);
    }
    Working {
        color: resize(color, side),
        normal: resize_weighted(normal, &opacity, side),
        ao: resize_weighted(ao, &opacity, side),
        material: resized_material,
    }
}

/// Chooses an alpha multiplier that best retains the requested cutout coverage.
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

/// Encodes filtered channels and raises roughness for unresolved normal detail.
fn encode(
    working: &Working,
    side: u32,
    cutout: bool,
    wanted: f32,
    overlay_mask: bool,
) -> (TextureMip, TextureMip, TextureMip) {
    let scale = if cutout && !overlay_mask {
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
        let inverse = if overlay_mask {
            1.0
        } else if opacity > 0.000001 {
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

/// Selects the category with most coverage, then filters only its visible values.
fn categorical(
    samples: impl Iterator<Item = (f32, f32)> + Clone,
    above: impl Fn(f32) -> bool,
    discrete: bool,
) -> f32 {
    let fallback = samples.clone().next().map_or(0.0, |(value, _)| value);
    let total = samples.clone().map(|(_, weight)| weight).sum::<f32>();
    if total <= 0.000001 {
        return fallback;
    }
    let above_weight = samples
        .clone()
        .filter(|(value, _)| above(*value))
        .map(|(_, weight)| weight)
        .sum::<f32>();
    let take_above = above_weight >= total * 0.5;
    let selected = || {
        samples
            .clone()
            .filter(|(value, weight)| *weight > 0.0 && above(*value) == take_above)
    };
    if take_above && discrete {
        let mut coverage = [0.0_f32; u8::MAX as usize + 1];
        for (value, weight) in selected() {
            coverage[decode::byte(value) as usize] += weight;
        }
        selected()
            .max_by(|(a, _), (b, _)| {
                coverage[decode::byte(*a) as usize].total_cmp(&coverage[decode::byte(*b) as usize])
            })
            .map(|(value, _)| value)
            .unwrap_or(fallback)
    } else {
        selected()
            .map(|(value, weight)| value * weight)
            .sum::<f32>()
            / selected().map(|(_, weight)| weight).sum::<f32>()
    }
}

/// Filters one mip while preserving opacity weights and material categories.
fn downsample(working: &Working, side: u32, lab: bool, overlay_mask: bool) -> Working {
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
    let weight = |x, y| {
        if overlay_mask {
            1.0
        } else {
            working.color.get_pixel(x, y)[3]
        }
    };
    let weighted = |x: u32, y: u32, channel: usize, image: &FloatImage| {
        let points = coordinates(x, y);
        let total = points.iter().map(|&(x, y)| weight(x, y)).sum::<f32>();
        if total <= 0.000001 {
            image.get_pixel(x * 2, y * 2)[channel]
        } else {
            points
                .iter()
                .map(|&(x, y)| image.get_pixel(x, y)[channel] * weight(x, y))
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
        let total = points.iter().map(|&(x, y)| weight(x, y)).sum::<f32>();
        Luma([if total <= 0.000001 {
            1.0
        } else {
            points
                .iter()
                .map(|&(x, y)| working.ao.get_pixel(x, y)[0] * weight(x, y))
                .sum::<f32>()
                / total
        }])
    });
    let material = FloatImage::from_fn(side, side, |x, y| {
        let mut result = std::array::from_fn(|channel| weighted(x, y, channel, &working.material));
        if lab {
            let points = coordinates(x, y);
            let weights = points.map(|(x, y)| weight(x, y));
            let metal = points.map(|(x, y)| working.material.get_pixel(x, y)[0]);
            result[0] = categorical(
                metal.into_iter().zip(weights),
                |value| value >= LABPBR_METAL_START,
                true,
            );
            let blue = points.map(|(x, y)| working.material.get_pixel(x, y)[3]);
            result[3] = categorical(
                blue.into_iter().zip(weights),
                |value| value > 64.0 / 255.0,
                false,
            );
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

/// Filters linear radiance, preserving RGB independently when alpha is an overlay tint mask.
/// Transparency remains premultiplied; unresolved normal detail raises roughness.
pub fn build_pbr_mips(
    surface: &PbrSurface,
    side: u32,
    cutout: bool,
    overlay_mask: bool,
) -> Result<PbrMipLayer, String> {
    if !side.is_power_of_two() || side > assets::texture::PBR_TILE_SIZE {
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
    let mut working = prepare(surface, side, overlay_mask);
    let mut size = side;
    let (mut colors, mut normals, mut materials) = (Vec::new(), Vec::new(), Vec::new());
    loop {
        let (color, normal, material) = encode(&working, size, cutout, wanted, overlay_mask);
        colors.push(color);
        normals.push(normal);
        materials.push(material);
        if size == 1 {
            break;
        }
        size /= 2;
        working = downsample(
            &working,
            size,
            surface.flags & PBR_REF_LABPBR != 0,
            overlay_mask,
        );
    }
    Ok(PbrMipLayer {
        color: colors.into_boxed_slice(),
        normal: normals.into_boxed_slice(),
        material: materials.into_boxed_slice(),
        flags: surface.flags,
    })
}
