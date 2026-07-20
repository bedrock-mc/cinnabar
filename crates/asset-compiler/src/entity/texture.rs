use std::{collections::BTreeMap, io::Cursor, path::Path};

use assets::{
    AssetError, EntityAssetKind, EntityAssetSource, EntityAssetSymbol, EntityRigBinding,
    EntityRigTexture,
};
use image::{ImageFormat, ImageReader, Limits};
use sha2::{Digest, Sha256};

use super::{SourcePayloads, invalid, json::parse_semantic_json};

pub(super) fn compile_default_rig_textures(
    root: &Path,
    sources: &[EntityAssetSource],
    symbols: &[EntityAssetSymbol],
    payloads: &SourcePayloads,
    bindings: &mut [EntityRigBinding],
) -> Result<Box<[EntityRigTexture]>, AssetError> {
    let mut textures = Vec::new();
    let mut by_symbol = BTreeMap::<u32, u32>::new();
    let mut texture_bytes = 0usize;
    for binding in bindings {
        let controller_symbol = &symbols[binding.render_controller as usize];
        let controller_source = &sources[controller_symbol.source_index as usize];
        let controller = parse_semantic_json(
            &root.join(controller_source.path.as_ref()),
            payloads
                .get(controller_source.path.as_ref())
                .ok_or_else(|| invalid("render controller payload is absent"))?,
        )?;
        let controller_textures = controller
            .get("render_controllers")
            .and_then(|controllers| controllers.get(controller_symbol.identifier.as_ref()))
            .and_then(|controller| controller.get("textures"))
            .and_then(serde_json::Value::as_array);
        if !controller_textures.is_some_and(|textures| {
            textures.len() == 1 && textures[0].as_str() == Some("Texture.default")
        }) {
            continue;
        }

        let entity_symbol = &symbols[binding.entity_symbol as usize];
        let entity_source = &sources[entity_symbol.source_index as usize];
        let entity = parse_semantic_json(
            &root.join(entity_source.path.as_ref()),
            payloads
                .get(entity_source.path.as_ref())
                .ok_or_else(|| invalid("client entity payload is absent"))?,
        )?;
        let Some(identifier) = entity
            .get("minecraft:client_entity")
            .and_then(|entity| entity.get("description"))
            .and_then(|description| description.get("textures"))
            .and_then(|textures| textures.get("default"))
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        let candidates = symbols
            .iter()
            .enumerate()
            .filter(|(_, symbol)| {
                symbol.kind == EntityAssetKind::Texture && symbol.identifier.as_ref() == identifier
            })
            .map(|(index, _)| index as u32)
            .collect::<Vec<_>>();
        if candidates.len() != 1 {
            continue;
        }
        let symbol = candidates[0];
        let texture = if let Some(&texture) = by_symbol.get(&symbol) {
            texture
        } else {
            let texture = decode_texture(sources, symbols, payloads, symbol, texture_bytes)?;
            texture_bytes = texture_bytes
                .checked_add(texture.rgba8.len())
                .ok_or_else(|| invalid("entity rig texture byte budget overflow"))?;
            if texture_bytes > assets::MAX_ENTITY_RIG_TEXTURE_BYTES
                || textures.len() == assets::MAX_ENTITY_RIG_TEXTURES
            {
                return Err(invalid("entity rig texture payload exceeds bound"));
            }
            let index = u32::try_from(textures.len())
                .map_err(|_| invalid("entity rig texture index overflow"))?;
            textures.push(texture);
            by_symbol.insert(symbol, index);
            index
        };
        binding.default_texture = Some(texture);
    }
    Ok(textures.into_boxed_slice())
}

fn decode_texture(
    sources: &[EntityAssetSource],
    symbols: &[EntityAssetSymbol],
    payloads: &SourcePayloads,
    symbol: u32,
    current_texture_bytes: usize,
) -> Result<EntityRigTexture, AssetError> {
    let source = symbols[symbol as usize].source_index;
    let source_path = &sources[source as usize].path;
    let bytes = payloads
        .get(source_path)
        .ok_or_else(|| invalid("entity rig texture source payload is absent"))?;
    let format = if source_path.ends_with(".png") {
        ImageFormat::Png
    } else if source_path.ends_with(".tga") {
        ImageFormat::Tga
    } else {
        return Err(invalid(format!(
            "entity rig texture format is unsupported at {source_path}"
        )));
    };
    let mut reader = ImageReader::with_format(Cursor::new(bytes.as_ref()), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(assets::MAX_ENTITY_RIG_TEXTURE_SIDE as u32);
    limits.max_image_height = Some(assets::MAX_ENTITY_RIG_TEXTURE_SIDE as u32);
    limits.max_alloc = Some(
        u64::try_from(assets::MAX_ENTITY_RIG_TEXTURE_BYTES.saturating_sub(current_texture_bytes))
            .unwrap_or(0),
    );
    reader.limits(limits);
    let decoded = reader
        .decode()
        .map_err(|error| {
            invalid(format!(
                "entity rig texture decode failed at {source_path}: {error}"
            ))
        })?
        .into_rgba8();
    let (width, height) = decoded.dimensions();
    let rgba8 = decoded.into_raw().into_boxed_slice();
    Ok(EntityRigTexture {
        symbol,
        source,
        width: u16::try_from(width)
            .map_err(|_| invalid("entity rig texture width exceeds bound"))?,
        height: u16::try_from(height)
            .map_err(|_| invalid("entity rig texture height exceeds bound"))?,
        pixels_sha256: Sha256::digest(&rgba8).into(),
        rgba8,
    })
}
