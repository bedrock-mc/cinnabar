//! Optional authored PBR texture loading for Enhanced terrain.
//!
//! The compiled Bedrock carrier remains the source of truth for block
//! identity. This module only maps its texture references to an external
//! Java-style color/normal/specular pack when the developer opts in through
//! `CINNABAR_ENHANCED_PBR_DIR`.

use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use assets::{MaterialKeys, NO_ANIMATION, RuntimeAssets, TextureArray, TextureMip, TextureRef};
use image::{DynamicImage, ImageBuffer, Rgba, RgbaImage, imageops::FilterType};

const TARGET_SIDE: u32 = 512;
const REF_FALLBACK: u32 = u32::MAX;

pub(crate) fn load_optional_enhanced_textures(
    runtime: &RuntimeAssets,
    keys: &MaterialKeys,
) -> Option<Arc<render::EnhancedTextureAssets>> {
    let roots = env::var_os(crate::asset_startup::ENHANCED_PBR_DIR_ENVIRONMENT)?
        .to_string_lossy()
        .split(';')
        .filter(|root| !root.trim().is_empty())
        .map(PathBuf::from)
        .filter(|root| root.is_dir())
        .collect::<Vec<_>>();
    if roots.is_empty() {
        eprintln!(
            "{} was set, but no listed directory exists; Enhanced PBR textures are disabled",
            crate::asset_startup::ENHANCED_PBR_DIR_ENVIRONMENT
        );
        return None;
    }

    let mut source_refs = BTreeMap::<u32, String>::new();
    for (key, alias) in keys.aliases() {
        for &material_id in keys.materials(key) {
            let Some(material) = runtime.materials().get(material_id as usize) else {
                continue;
            };
            source_refs
                .entry(material.texture.raw())
                .or_insert_with(|| alias.to_owned());
            if material.animation != NO_ANIMATION
                && let Some(animation) = runtime.animations().get(material.animation as usize)
            {
                let start = animation.frame_start as usize;
                let end = start.saturating_add(animation.frame_count as usize);
                for frame in runtime
                    .animation_frames()
                    .get(start..end)
                    .into_iter()
                    .flatten()
                {
                    source_refs
                        .entry(frame.raw())
                        .or_insert_with(|| alias.to_owned());
                }
            }
        }
    }

    let mut colors = Vec::new();
    let mut normals = Vec::new();
    let mut mers = Vec::new();
    let mut refs = vec![REF_FALLBACK; assets::MAX_TEXTURE_PAGES * assets::MAX_TEXTURE_LAYERS];
    for (texture_ref, alias) in source_refs {
        let Some(color_path) = find_image(&roots, &alias, "") else {
            continue;
        };
        let Some(color) = decode_rgba(&color_path) else {
            continue;
        };
        let normal = find_image(&roots, &alias, "_normal")
            .and_then(|path| decode_rgba(&path))
            .or_else(|| find_image(&roots, &alias, "_n").and_then(|path| decode_rgba(&path)))
            .unwrap_or_else(|| flat_normal(color.width(), color.height()));
        let mer = find_image(&roots, &alias, "_mer")
            .and_then(|path| decode_rgba(&path))
            .or_else(|| {
                find_image(&roots, &alias, "_s")
                    .and_then(|path| decode_rgba(&path))
                    .map(convert_old_pbr_specular)
            })
            .unwrap_or_else(|| default_mer(color.width(), color.height()));
        let layer = u32::try_from(colors.len()).ok()?;
        if layer >= assets::MAX_TEXTURE_LAYERS as u32 {
            break;
        }
        colors.push(to_target_size(color, FilterType::Lanczos3));
        normals.push(to_target_size(normal, FilterType::Lanczos3));
        mers.push(to_target_size(mer, FilterType::Nearest));
        let page = (texture_ref >> 31) as usize;
        let source_layer = (texture_ref & 0x7ff) as usize;
        if page < assets::MAX_TEXTURE_PAGES {
            refs[page * assets::MAX_TEXTURE_LAYERS + source_layer] =
                TextureRef::new(0, layer).ok()?.raw();
        }
    }
    if colors.is_empty() {
        eprintln!("no authored block textures matched the compiled terrain catalog");
        return None;
    }

    let color_page = texture_array(colors)?;
    let normal_page = texture_array(normals)?;
    let mer_page = texture_array(mers)?;
    let diagnostic_color = solid_page([128, 128, 128, 255]);
    let diagnostic_normal = solid_page([128, 128, 255, 255]);
    let diagnostic_mer = solid_page([0, 0, 255, 255]);
    let enhanced = render::EnhancedTextureAssets::new(
        [color_page, diagnostic_color.clone()],
        [normal_page, diagnostic_normal.clone()],
        [mer_page, diagnostic_mer.clone()],
        refs.into_boxed_slice(),
    )?;
    eprintln!(
        "matched {} authored Enhanced terrain textures (normalized to {}x{})",
        enhanced_layer_count(&enhanced),
        TARGET_SIDE,
        TARGET_SIDE
    );
    Some(Arc::new(enhanced))
}

fn enhanced_layer_count(enhanced: &render::EnhancedTextureAssets) -> usize {
    enhanced.authored_layer_count()
}

fn texture_array(layers: Vec<RgbaImage>) -> Option<TextureArray> {
    let mut current = layers;
    let mut size = TARGET_SIDE;
    let mut all_mips = Vec::new();
    loop {
        let mut rgba8 = Vec::new();
        for layer in &current {
            rgba8.extend_from_slice(layer.as_raw());
        }
        all_mips.push(TextureMip {
            size,
            rgba8: rgba8.into_boxed_slice(),
        });
        if size == 1 {
            break;
        }
        let next_size = size / 2;
        current = current
            .iter()
            .map(|layer| image::imageops::resize(layer, next_size, next_size, FilterType::Triangle))
            .collect();
        size = next_size;
    }
    Some(TextureArray {
        layers: u32::try_from(current.len()).ok()?,
        mips: all_mips.into_boxed_slice(),
    })
}

fn solid_page(pixel: [u8; 4]) -> TextureArray {
    texture_array(vec![ImageBuffer::from_pixel(
        TARGET_SIDE,
        TARGET_SIDE,
        Rgba(pixel),
    )])
    .expect("diagnostic texture is valid")
}

fn decode_rgba(path: &Path) -> Option<RgbaImage> {
    let bytes = fs::read(path).ok()?;
    image::load_from_memory(&bytes)
        .ok()
        .map(DynamicImage::into_rgba8)
}

fn to_target_size(image: RgbaImage, filter: FilterType) -> RgbaImage {
    if image.width() == TARGET_SIDE && image.height() == TARGET_SIDE {
        return image;
    }
    image::imageops::resize(&image, TARGET_SIDE, TARGET_SIDE, filter)
}

fn flat_normal(width: u32, height: u32) -> RgbaImage {
    ImageBuffer::from_pixel(width.max(1), height.max(1), Rgba([128, 128, 255, 255]))
}

fn default_mer(width: u32, height: u32) -> RgbaImage {
    ImageBuffer::from_pixel(width.max(1), height.max(1), Rgba([0, 0, 255, 255]))
}

fn convert_old_pbr_specular(mut image: RgbaImage) -> RgbaImage {
    for pixel in image.pixels_mut() {
        let [smoothness, metalness, emissive, _] = pixel.0;
        *pixel = Rgba([metalness, emissive, 255_u8.saturating_sub(smoothness), 255]);
    }
    image
}

fn find_image(roots: &[PathBuf], alias: &str, suffix: &str) -> Option<PathBuf> {
    let mut relative = alias.replace('\\', "/");
    if relative.ends_with(".png") {
        relative.truncate(relative.len().saturating_sub(4));
    }
    relative.push_str(suffix);
    relative.push_str(".png");
    let variants = [
        relative.clone(),
        relative.replace("textures/blocks/", "textures/block/"),
        relative.replace("textures/block/", "textures/blocks/"),
    ];
    for root in roots {
        for variant in &variants {
            let candidates = [
                root.join("assets/minecraft").join(variant),
                root.join(variant),
            ];
            if let Some(path) = candidates.into_iter().find(|path| path.is_file()) {
                return Some(path);
            }
        }
    }
    None
}
