use std::{collections::BTreeMap, io::Cursor};

use png::{Decoder, Transformations};
use serde_json::Value;

use super::{
    MAX_ITEM_ICON_COUNT, MAX_ITEM_ICON_DECODED_BYTES, MAX_ITEM_ICON_SIDE, ServerResourcePackError,
};

const MAX_ITEM_TEXTURE_MANIFEST_BYTES: usize = 8 * 1024 * 1024;
const MAX_ITEM_TEXTURE_ALIASES: usize = 16_384;
const MAX_ITEM_TEXTURE_VARIANTS: usize = 256;

pub(super) type TextureRoutes = BTreeMap<(Box<str>, u32), (Box<str>, Box<str>)>;
type DecodedTexture = (u16, u16, Box<[u8]>);

/// One effective item-atlas entry decoded from the server pack stack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerResourcePackItemIcon {
    identifier: Box<str>,
    metadata: u32,
    width: u16,
    height: u16,
    rgba8: Box<[u8]>,
}

impl ServerResourcePackItemIcon {
    pub fn identifier(&self) -> &str {
        &self.identifier
    }

    pub const fn metadata(&self) -> u32 {
        self.metadata
    }

    pub const fn width(&self) -> u16 {
        self.width
    }

    pub const fn height(&self) -> u16 {
        self.height
    }

    pub fn rgba8(&self) -> &[u8] {
        &self.rgba8
    }
}

pub(super) fn merge_item_texture_manifest(
    pack: &str,
    bytes: &[u8],
    routes: &mut TextureRoutes,
) -> Result<(), ServerResourcePackError> {
    if bytes.len() > MAX_ITEM_TEXTURE_MANIFEST_BYTES {
        return Err(ServerResourcePackError::ItemTextureManifest {
            pack: pack.into(),
            path: "textures/item_texture.json".into(),
            detail: "manifest exceeds its byte limit".into(),
        });
    }
    let json =
        strip_json_comments(bytes).ok_or_else(|| ServerResourcePackError::ItemTextureManifest {
            pack: pack.into(),
            path: "textures/item_texture.json".into(),
            detail: "manifest is not valid UTF-8 or has an unterminated string".into(),
        })?;
    let value: Value = serde_json::from_slice(&json).map_err(|source| {
        ServerResourcePackError::ItemTextureManifest {
            pack: pack.into(),
            path: "textures/item_texture.json".into(),
            detail: source.to_string().into_boxed_str(),
        }
    })?;
    let texture_data = value
        .get("texture_data")
        .and_then(Value::as_object)
        .ok_or_else(|| ServerResourcePackError::ItemTextureManifest {
            pack: pack.into(),
            path: "textures/item_texture.json".into(),
            detail: "manifest lacks an object-valued texture_data field".into(),
        })?;
    if texture_data.len() > MAX_ITEM_TEXTURE_ALIASES {
        return Err(ServerResourcePackError::ItemTextureManifest {
            pack: pack.into(),
            path: "textures/item_texture.json".into(),
            detail: "texture_data has too many aliases".into(),
        });
    }
    for (alias, definition) in texture_data {
        let identifier = canonical_item_identifier(alias);
        let textures = definition.get("textures").ok_or_else(|| {
            ServerResourcePackError::ItemTextureManifest {
                pack: pack.into(),
                path: "textures/item_texture.json".into(),
                detail: format!("item alias {alias} lacks textures").into_boxed_str(),
            }
        })?;
        let variants = match textures {
            Value::String(texture) => vec![texture.as_str()],
            Value::Array(values) if !values.is_empty() => {
                if values.len() > MAX_ITEM_TEXTURE_VARIANTS {
                    return Err(ServerResourcePackError::ItemTextureManifest {
                        pack: pack.into(),
                        path: "textures/item_texture.json".into(),
                        detail: format!("item alias {alias} has too many variants")
                            .into_boxed_str(),
                    });
                }
                values
                    .iter()
                    .map(|value| {
                        value
                            .as_str()
                            .ok_or_else(|| ServerResourcePackError::ItemTextureManifest {
                                pack: pack.into(),
                                path: "textures/item_texture.json".into(),
                                detail: format!("item alias {alias} has a non-string variant")
                                    .into_boxed_str(),
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?
            }
            _ => {
                return Err(ServerResourcePackError::ItemTextureManifest {
                    pack: pack.into(),
                    path: "textures/item_texture.json".into(),
                    detail: format!("item alias {alias} has invalid textures").into_boxed_str(),
                });
            }
        };
        for (metadata, texture) in variants.into_iter().enumerate() {
            let path = canonical_item_texture_path(texture).ok_or_else(|| {
                ServerResourcePackError::ItemTextureManifest {
                    pack: pack.into(),
                    path: "textures/item_texture.json".into(),
                    detail: format!("item alias {alias} has an unsafe texture path")
                        .into_boxed_str(),
                }
            })?;
            let metadata = u32::try_from(metadata).map_err(|_| {
                ServerResourcePackError::ItemTextureManifest {
                    pack: pack.into(),
                    path: "textures/item_texture.json".into(),
                    detail: "item texture variant index overflows u32".into(),
                }
            })?;
            routes.insert(
                ((identifier.clone()).into_boxed_str(), metadata),
                (pack.into(), path),
            );
        }
    }
    Ok(())
}

pub(super) fn decode_item_icons(
    routes: &TextureRoutes,
    item_assets: &BTreeMap<Box<str>, Box<[u8]>>,
    block_assets: &BTreeMap<Box<str>, Box<[u8]>>,
) -> Result<Box<[ServerResourcePackItemIcon]>, ServerResourcePackError> {
    if routes.len() > MAX_ITEM_ICON_COUNT {
        return Err(ServerResourcePackError::ItemTextureManifest {
            pack: routes
                .values()
                .next()
                .map_or_else(|| "resource-pack-stack".into(), |(pack, _)| pack.clone()),
            path: "textures/item_texture.json".into(),
            detail: "effective item icon count exceeds its bound".into(),
        });
    }
    let mut icons = Vec::with_capacity(routes.len());
    for ((identifier, metadata), (pack, path)) in routes {
        let Some(bytes) = item_assets.get(path).or_else(|| block_assets.get(path)) else {
            continue;
        };
        let (width, height, rgba8) = decode_pack_texture(path, bytes).map_err(|detail| {
            ServerResourcePackError::ItemTextureDecode {
                pack: pack.clone(),
                path: path.clone(),
                detail,
            }
        })?;
        icons.push(ServerResourcePackItemIcon {
            identifier: identifier.clone(),
            metadata: *metadata,
            width,
            height,
            rgba8,
        });
    }
    Ok(icons.into_boxed_slice())
}

fn canonical_item_identifier(alias: &str) -> String {
    if alias.contains(':') {
        alias.to_owned()
    } else {
        format!("minecraft:{alias}")
    }
}

fn canonical_item_texture_path(texture: &str) -> Option<Box<str>> {
    canonical_texture_path(texture)
}

pub(super) fn canonical_texture_path(texture: &str) -> Option<Box<str>> {
    let mut path = texture.replace('\\', "/");
    if path.starts_with("items/")
        || path.starts_with("item/")
        || path.starts_with("blocks/")
        || path.starts_with("block/")
    {
        path.insert_str(0, "textures/");
    }
    if !path.starts_with("textures/")
        || path.starts_with('/')
        || path.contains("..")
        || path.bytes().any(|byte| byte.is_ascii_control())
    {
        return None;
    }
    if !path.ends_with(".png") && !path.ends_with(".tga") {
        path.push_str(".png");
    }
    Some(path.into_boxed_str())
}

pub(super) fn strip_json_comments(bytes: &[u8]) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut output = Vec::with_capacity(bytes.len());
    let mut chars = text.char_indices().peekable();
    let mut in_string = false;
    let mut escaped = false;
    while let Some((index, character)) = chars.next() {
        if in_string {
            output.extend_from_slice(character.encode_utf8(&mut [0; 4]).as_bytes());
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        if character == '"' {
            in_string = true;
            output.push(b'"');
            continue;
        }
        if character == '/' && chars.peek().is_some_and(|(_, next)| *next == '/') {
            chars.next();
            for (_, next) in chars.by_ref() {
                if next == '\n' || next == '\r' {
                    output.push(next as u8);
                    break;
                }
            }
            continue;
        }
        if character == '/' && chars.peek().is_some_and(|(_, next)| *next == '*') {
            chars.next();
            let mut terminated = false;
            loop {
                let Some((_, next)) = chars.next() else {
                    break;
                };
                if next == '*' && chars.peek().is_some_and(|(_, end)| *end == '/') {
                    chars.next();
                    terminated = true;
                    break;
                }
            }
            if !terminated {
                return None;
            }
            continue;
        }
        let _ = index;
        output.extend_from_slice(character.encode_utf8(&mut [0; 4]).as_bytes());
    }
    if in_string {
        return None;
    }
    Some(strip_trailing_json_commas(output))
}

fn strip_trailing_json_commas(bytes: Vec<u8>) -> Vec<u8> {
    let mut output = Vec::with_capacity(bytes.len());
    let mut in_string = false;
    let mut escaped = false;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            output.push(byte);
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if byte == b'"' {
            in_string = true;
            output.push(byte);
            index += 1;
            continue;
        }
        if byte == b',' {
            let mut next = index + 1;
            while bytes
                .get(next)
                .is_some_and(|candidate| matches!(*candidate, b' ' | b'\t' | b'\r' | b'\n'))
            {
                next += 1;
            }
            if matches!(bytes.get(next), Some(b'}' | b']')) {
                index += 1;
                continue;
            }
        }
        output.push(byte);
        index += 1;
    }
    output
}

pub(super) fn decode_pack_texture(path: &str, bytes: &[u8]) -> Result<DecodedTexture, Box<str>> {
    if path.ends_with(".png") {
        decode_item_png(bytes)
    } else if path.ends_with(".tga") {
        decode_item_tga(bytes)
    } else {
        Err("item texture uses an unsupported image extension".into())
    }
}

/// Resamples a decoded pack texture into the 16x16 tile shape used by the
/// immutable terrain texture arrays. Alpha-weighted averaging avoids dark
/// fringes around transparent pixels when a pack supplies a higher-resolution
/// texture.
pub(super) fn normalize_texture_tile(
    width: u16,
    height: u16,
    rgba8: &[u8],
) -> Result<Box<[u8]>, Box<str>> {
    let width = usize::from(width);
    let height = usize::from(height);
    let expected = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| Box::<str>::from("texture dimensions overflow the decoder bound"))?;
    if rgba8.len() != expected {
        return Err("decoded texture bytes do not match its dimensions".into());
    }
    if width == 16 && height == 16 {
        return Ok(rgba8.to_vec().into_boxed_slice());
    }
    let mut output = vec![0; 16 * 16 * 4];
    for output_y in 0..16 {
        let source_y_start = output_y * height / 16;
        let source_y_end = ((output_y + 1) * height / 16).max(source_y_start + 1);
        for output_x in 0..16 {
            let source_x_start = output_x * width / 16;
            let source_x_end = ((output_x + 1) * width / 16).max(source_x_start + 1);
            let mut alpha_sum = 0u32;
            let mut color_sum = [0u32; 3];
            let mut samples = 0u32;
            for source_y in source_y_start..source_y_end.min(height) {
                for source_x in source_x_start..source_x_end.min(width) {
                    let source = (source_y * width + source_x) * 4;
                    let alpha = u32::from(rgba8[source + 3]);
                    alpha_sum += alpha;
                    for (channel, sum) in color_sum.iter_mut().enumerate() {
                        *sum += u32::from(rgba8[source + channel]) * alpha;
                    }
                    samples += 1;
                }
            }
            let destination = (output_y * 16 + output_x) * 4;
            let alpha = if samples == 0 {
                0
            } else {
                (alpha_sum / samples).min(255)
            };
            output[destination + 3] = alpha as u8;
            for (channel, sum) in color_sum.into_iter().enumerate() {
                output[destination + channel] = if alpha_sum == 0 {
                    0
                } else {
                    (sum / alpha_sum).min(255) as u8
                };
            }
        }
    }
    Ok(output.into_boxed_slice())
}

fn decode_item_png(bytes: &[u8]) -> Result<DecodedTexture, Box<str>> {
    let mut decoder = Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(
        Transformations::EXPAND | Transformations::STRIP_16 | Transformations::ALPHA,
    );
    let mut reader = decoder
        .read_info()
        .map_err(|source| source.to_string().into_boxed_str())?;
    let width = reader.info().width;
    let height = reader.info().height;
    validate_item_texture_dimensions(width, height)?;
    let output_size = reader
        .output_buffer_size()
        .ok_or_else(|| Box::<str>::from("item PNG output size overflows the decoder bound"))?;
    if output_size > MAX_ITEM_ICON_DECODED_BYTES {
        return Err("item PNG decoded bytes exceed the decoder bound".into());
    }
    let mut decoded = vec![0; output_size];
    let output = reader
        .next_frame(&mut decoded)
        .map_err(|source| source.to_string().into_boxed_str())?;
    let rgba8 = super::rgba8_from_png(
        output.color_type,
        &decoded[..output.buffer_size()],
        width,
        height,
    )
    .ok_or_else(|| {
        Box::<str>::from("item PNG did not decode to an 8-bit RGBA-compatible format")
    })?;
    Ok((
        u16::try_from(width).map_err(|_| Box::<str>::from("item PNG width exceeds u16"))?,
        u16::try_from(height).map_err(|_| Box::<str>::from("item PNG height exceeds u16"))?,
        rgba8.into_boxed_slice(),
    ))
}

fn decode_item_tga(bytes: &[u8]) -> Result<DecodedTexture, Box<str>> {
    if bytes.len() < 18 || bytes[1] != 0 || !matches!(bytes[2], 2 | 10) {
        return Err("item TGA is not an uncompressed or RLE true-colour image".into());
    }
    let width = u16::from_le_bytes([bytes[12], bytes[13]]);
    let height = u16::from_le_bytes([bytes[14], bytes[15]]);
    validate_item_texture_dimensions(u32::from(width), u32::from(height))?;
    let bytes_per_pixel = match bytes[16] {
        24 => 3usize,
        32 => 4usize,
        _ => return Err("item TGA must use 24-bit or 32-bit pixels".into()),
    };
    let pixel_count = usize::from(width)
        .checked_mul(usize::from(height))
        .ok_or_else(|| Box::<str>::from("item TGA pixel count overflows the decoder bound"))?;
    let decoded_bytes = pixel_count
        .checked_mul(4)
        .ok_or_else(|| Box::<str>::from("item TGA decoded bytes overflow the decoder bound"))?;
    if decoded_bytes > MAX_ITEM_ICON_DECODED_BYTES {
        return Err("item TGA decoded bytes exceed the decoder bound".into());
    }
    let mut rgba8 = vec![0; decoded_bytes];
    let mut source = 18usize + usize::from(bytes[0]);
    let top_origin = bytes[17] & 0x20 != 0;
    let mut write_pixel = |source_pixel: &[u8], logical_index: usize| {
        let x = logical_index % usize::from(width);
        let y = logical_index / usize::from(width);
        let output_y = if top_origin {
            y
        } else {
            usize::from(height) - 1 - y
        };
        let target = (output_y * usize::from(width) + x) * 4;
        rgba8[target..target + 4].copy_from_slice(&[
            source_pixel[2],
            source_pixel[1],
            source_pixel[0],
            source_pixel.get(3).copied().unwrap_or(255),
        ]);
    };
    let mut logical_index = 0usize;
    while logical_index < pixel_count {
        if source >= bytes.len() {
            return Err("item TGA pixel stream is truncated".into());
        }
        let packet = bytes[source];
        source += 1;
        let count = usize::from(packet & 0x7f) + 1;
        if count > pixel_count - logical_index {
            return Err("item TGA packet exceeds the declared pixel count".into());
        }
        if packet & 0x80 != 0 {
            let end = source
                .checked_add(bytes_per_pixel)
                .ok_or_else(|| Box::<str>::from("item TGA packet overflows the source"))?;
            let pixel = bytes
                .get(source..end)
                .ok_or_else(|| Box::<str>::from("item TGA RLE packet is truncated"))?;
            for offset in 0..count {
                write_pixel(pixel, logical_index + offset);
            }
            source = end;
        } else {
            for offset in 0..count {
                let end = source
                    .checked_add(bytes_per_pixel)
                    .ok_or_else(|| Box::<str>::from("item TGA packet overflows the source"))?;
                let pixel = bytes
                    .get(source..end)
                    .ok_or_else(|| Box::<str>::from("item TGA raw packet is truncated"))?;
                write_pixel(pixel, logical_index + offset);
                source = end;
            }
        }
        logical_index += count;
    }
    Ok((width, height, rgba8.into_boxed_slice()))
}

fn validate_item_texture_dimensions(width: u32, height: u32) -> Result<(), Box<str>> {
    if width == 0 || height == 0 || width > MAX_ITEM_ICON_SIDE || height > MAX_ITEM_ICON_SIDE {
        return Err(format!(
            "item texture dimensions {width}x{height} exceed the {MAX_ITEM_ICON_SIDE}px bound"
        )
        .into_boxed_str());
    }
    let decoded_bytes = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| Box::<str>::from("item texture dimensions overflow the decoder bound"))?;
    if decoded_bytes > MAX_ITEM_ICON_DECODED_BYTES {
        return Err("item texture dimensions exceed the decoder byte bound".into());
    }
    Ok(())
}
