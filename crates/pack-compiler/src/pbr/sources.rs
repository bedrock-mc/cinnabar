use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use image::{DynamicImage, ImageBuffer, Rgba, RgbaImage};
use serde_json::Value;

use super::{PBR_REF_COLOR, PBR_REF_NORMAL, PbrFormat, PbrPack, PbrSurface, PbrTexture, decode};

mod names;

const IMAGE_BYTES_LIMIT: u64 = 64 * 1024 * 1024;
const JSON_BYTES_LIMIT: u64 = 1024 * 1024;

pub(super) fn declared_format(root: &Path) -> PbrFormat {
    for relative in [
        "assets/minecraft/optifine/texture.properties",
        "optifine/texture.properties",
    ] {
        if let Ok(text) = fs::read_to_string(root.join(relative)) {
            for line in text.lines().map(str::trim) {
                if let Some(value) = line.strip_prefix("format=") {
                    return PbrFormat::parse(value).unwrap_or_default();
                }
            }
        }
    }
    PbrFormat::Unspecified
}

fn safe_relative(value: &str) -> Option<PathBuf> {
    let value = value.replace('\\', "/");
    let path = Path::new(&value);
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        return None;
    }
    Some(path.to_owned())
}

fn candidates(pack: &PbrPack, alias: &str, suffix: &str, extension: &str) -> Vec<PathBuf> {
    names::variants(alias)
        .into_iter()
        .filter_map(|name| {
            let relative = safe_relative(&format!("{name}{suffix}.{extension}"))?;
            Some([
                pack.root.join(&relative),
                pack.root.join("assets/minecraft").join(relative),
            ])
        })
        .flatten()
        .collect()
}

fn find(pack: &PbrPack, alias: &str, suffix: &str) -> Option<PathBuf> {
    ["tga", "png", "jpg", "jpeg"]
        .into_iter()
        .flat_map(|extension| candidates(pack, alias, suffix, extension))
        .find(|path| path.is_file())
}

fn json_path(pack: &PbrPack, alias: &str) -> Option<PathBuf> {
    candidates(pack, alias, ".texture_set", "json")
        .into_iter()
        .find(|path| path.is_file())
}

fn image(path: &Path, channels: &[u8]) -> Result<RgbaImage, String> {
    if fs::metadata(path)
        .map_err(|error| format!("{}: {error}", path.display()))?
        .len()
        > IMAGE_BYTES_LIMIT
    {
        return Err(format!(
            "{} exceeds authored texture byte limit",
            path.display()
        ));
    }
    let image = image::ImageReader::open(path)
        .map_err(|error| format!("{}: {error}", path.display()))?
        .with_guessed_format()
        .map_err(|error| error.to_string())?
        .decode()
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if !channels.contains(&image.color().channel_count()) {
        return Err(format!(
            "{} has unsupported {}-channel data",
            path.display(),
            image.color().channel_count()
        ));
    }
    Ok(DynamicImage::into_rgba8(image))
}

fn solid(pixel: [u8; 4]) -> RgbaImage {
    ImageBuffer::from_pixel(1, 1, Rgba(pixel))
}

fn byte(value: &Value) -> Result<u8, String> {
    value
        .as_f64()
        .filter(|value| {
            value.is_finite() && *value >= 0.0 && *value <= 255.0 && value.fract() == 0.0
        })
        .map(|value| value as u8)
        .ok_or_else(|| "texture set constants must be integers between 0 and 255".to_owned())
}

fn uniform(value: &Value, channels: usize) -> Result<Option<[u8; 4]>, String> {
    let mut result = [0, 0, 0, 255];
    if let Some(values) = value.as_array() {
        if values.len() != channels {
            return Err(format!("texture set constant needs {channels} channels"));
        }
        for (output, value) in result.iter_mut().zip(values) {
            *output = byte(value)?;
        }
        return Ok(Some(result));
    }
    if channels == 1 && value.is_number() {
        result[0] = byte(value)?;
        return Ok(Some(result));
    }
    if let Some(hex) = value.as_str().and_then(|value| value.strip_prefix('#')) {
        let encoded_channels = if channels == 4 && hex.len() == 6 {
            3
        } else {
            channels
        };
        if hex.len() != encoded_channels * 2 {
            return Err(format!(
                "texture set hex constant needs {} digits",
                channels * 2
            ));
        }
        let packed =
            u32::from_str_radix(hex, 16).map_err(|_| "invalid texture set hex constant")?;
        for (index, output) in result.iter_mut().take(encoded_channels).enumerate() {
            *output = ((packed >> (8 * (encoded_channels - index - 1))) & 255) as u8;
        }
        if encoded_channels == 4 {
            result.rotate_left(1);
        }
        return Ok(Some(result));
    }
    Ok(None)
}

fn layer(
    pack: &PbrPack,
    json: &Path,
    value: &Value,
    channels: usize,
    allowed: &[u8],
) -> Result<RgbaImage, String> {
    if let Some(pixel) = uniform(value, channels)? {
        return Ok(solid(pixel));
    }
    let reference = value
        .as_str()
        .ok_or_else(|| "texture set layer must be an image reference or constant".to_owned())?;
    let relative = safe_relative(reference)
        .ok_or_else(|| "texture set image reference escapes its pack".to_owned())?;
    let stem = relative.with_extension("");
    let parent = json.parent().unwrap_or(&pack.root);
    let roots = if reference.starts_with("textures/") {
        vec![pack.root.clone(), pack.root.join("assets/minecraft")]
    } else {
        vec![parent.to_owned()]
    };
    let path = ["tga", "png", "jpg", "jpeg"]
        .into_iter()
        .flat_map(|extension| {
            roots
                .iter()
                .map(|root| root.join(stem.with_extension(extension)))
                .collect::<Vec<_>>()
        })
        .find(|path| path.is_file())
        .ok_or_else(|| format!("texture set cannot find {reference} in its defining pack"))?;
    image(&path, allowed)
}

fn read_json(path: &Path) -> Result<Value, String> {
    if fs::metadata(path).map_err(|error| error.to_string())?.len() > JSON_BYTES_LIMIT {
        return Err("texture metadata exceeds byte limit".to_owned());
    }
    serde_json::from_slice(&fs::read(path).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())
}

fn texture_set(pack: &PbrPack, path: &Path) -> Result<PbrTexture, String> {
    let json = read_json(path)?;
    let set = json
        .get("minecraft:texture_set")
        .and_then(Value::as_object)
        .ok_or_else(|| "missing minecraft:texture_set object".to_owned())?;
    if set.contains_key("normal") && set.contains_key("heightmap") {
        return Err("texture set defines both normal and heightmap".to_owned());
    }
    if set.contains_key("metalness_emissive_roughness")
        && set.contains_key("metalness_emissive_roughness_subsurface")
    {
        return Err("texture set defines both MER and MERS".to_owned());
    }
    let color = layer(
        pack,
        path,
        set.get("color")
            .ok_or_else(|| "texture set has no color".to_owned())?,
        4,
        &[3, 4],
    )?;
    let mut flags = PBR_REF_COLOR;
    let normal = if let Some(value) = set.get("normal") {
        if value
            .as_str()
            .is_none_or(|reference| reference.starts_with('#'))
        {
            return Err("texture set normal must reference an RGB image".to_owned());
        }
        flags |= PBR_REF_NORMAL;
        let (normal, normal_flags) = decode::normal(
            layer(pack, path, value, 3, &[3, 4])?,
            PbrFormat::Legacy,
            pack.normal_format,
        );
        flags |= normal_flags;
        normal
    } else {
        solid([128, 128, 255, 128])
    };
    let material = if let Some(value) = set.get("metalness_emissive_roughness_subsurface") {
        let (material, material_flags) = decode::mer(layer(pack, path, value, 4, &[4])?, true);
        flags |= material_flags;
        material
    } else if let Some(value) = set.get("metalness_emissive_roughness") {
        let (material, material_flags) = decode::mer(layer(pack, path, value, 3, &[3, 4])?, false);
        flags |= material_flags;
        material
    } else {
        solid([0, 0, 255, 0])
    };
    let height = set
        .get("heightmap")
        .map(|value| layer(pack, path, value, 1, &[1]))
        .transpose()?;
    Ok(PbrTexture {
        surface: PbrSurface {
            color,
            normal,
            material,
            flags,
        },
        timeline: Vec::new(),
        height,
    })
}

fn animation(path: &Path) -> Result<Vec<u32>, String> {
    let meta = PathBuf::from(format!("{}.mcmeta", path.display()));
    if !meta.is_file() {
        return Ok(Vec::new());
    }
    let json = read_json(&meta)?;
    let Some(frames) = json
        .get("animation")
        .and_then(|value| value.get("frames"))
        .and_then(Value::as_array)
    else {
        return Ok(Vec::new());
    };
    let mut timeline = Vec::new();
    for frame in frames {
        let index = frame
            .as_u64()
            .or_else(|| frame.get("index").and_then(Value::as_u64))
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| "invalid authored animation frame index".to_owned())?;
        let repeat = frame
            .get("time")
            .and_then(Value::as_u64)
            .unwrap_or(1)
            .min(512) as usize;
        if repeat == 0 || timeline.len().saturating_add(repeat) > 4096 {
            return Err("authored animation timeline exceeds supported budget".to_owned());
        }
        timeline.extend(std::iter::repeat_n(index, repeat));
    }
    Ok(timeline)
}

pub(super) fn load(packs: &[PbrPack], alias: &str) -> Result<Option<PbrTexture>, String> {
    for owner in packs {
        if let Some(path) = json_path(owner, alias) {
            return texture_set(owner, &path).map(Some);
        }
        let Some(color_path) = find(owner, alias, "") else {
            continue;
        };
        let color = image(&color_path, &[3, 4])?;
        let companions = packs.iter().filter(|pack| {
            pack.root == owner.root
                || owner
                    .group
                    .as_ref()
                    .is_some_and(|group| pack.group.as_ref() == Some(group))
        });
        let companions = companions.collect::<Vec<_>>();
        let mut flags = PBR_REF_COLOR;
        let normal_source = companions.iter().find_map(|pack| {
            find(pack, alias, "_normal")
                .map(|path| (*pack, path, PbrFormat::Legacy))
                .or_else(|| {
                    effective_format(owner, pack)
                        .map(|format| find(pack, alias, "_n").map(|path| (*pack, path, format)))
                        .flatten()
                })
        });
        let normal = if let Some((pack, path, format)) = normal_source {
            let (normal, normal_flags) =
                decode::normal(image(&path, &[3, 4])?, format, pack.normal_format);
            flags |= PBR_REF_NORMAL | normal_flags;
            normal
        } else {
            solid([128, 128, 255, 128])
        };
        let material_source = companions.iter().find_map(|pack| {
            find(pack, alias, "_mers")
                .map(|path| (path, true, None))
                .or_else(|| find(pack, alias, "_mer").map(|path| (path, false, None)))
                .or_else(|| {
                    effective_format(owner, pack)
                        .map(|format| {
                            find(pack, alias, "_s").map(|path| (path, false, Some(format)))
                        })
                        .flatten()
                })
        });
        let material = if let Some((path, subsurface, format)) = material_source {
            let raw = image(&path, if subsurface { &[4] } else { &[3, 4] })?;
            let (material, material_flags) = match format {
                Some(format) => decode::specular(raw, format),
                None => decode::mer(raw, subsurface),
            };
            flags |= material_flags;
            material
        } else {
            solid([0, 0, 255, 0])
        };
        let height = companions
            .iter()
            .find_map(|pack| find(pack, alias, "_h").or_else(|| find(pack, alias, "_heightmap")))
            .map(|path| image(&path, &[1, 3, 4]))
            .transpose()?;
        return Ok(Some(PbrTexture {
            surface: PbrSurface {
                color,
                normal,
                material,
                flags,
            },
            timeline: animation(&color_path)?,
            height,
        }));
    }
    Ok(None)
}

/// A declared format applies to a grouped material set, so a companion pack
/// may contain only maps while the color owner carries the metadata. An
/// unspecified pair still rejects Java `_n`/`_s` guesses.
fn effective_format(owner: &PbrPack, companion: &PbrPack) -> Option<PbrFormat> {
    [companion.format, owner.format]
        .into_iter()
        .find(|format| *format != PbrFormat::Unspecified)
}
