use serde_json::Value;

use super::{
    ServerResourcePackError,
    resource_pack_items::{TextureRoutes, canonical_texture_path, strip_json_comments},
};

const MAX_TEXTURE_MANIFEST_BYTES: usize = 8 * 1024 * 1024;
const MAX_TEXTURE_ALIASES: usize = 16_384;
const MAX_TEXTURE_VARIANTS: usize = 256;

pub(super) fn merge_block_texture_manifest(
    pack: &str,
    bytes: &[u8],
    routes: &mut TextureRoutes,
) -> Result<(), ServerResourcePackError> {
    if bytes.len() > MAX_TEXTURE_MANIFEST_BYTES {
        return Err(manifest_error(pack, "manifest exceeds its byte limit"));
    }
    let json = strip_json_comments(bytes).ok_or_else(|| {
        manifest_error(
            pack,
            "manifest is not valid UTF-8 or has an unterminated string",
        )
    })?;
    let value: Value =
        serde_json::from_slice(&json).map_err(|source| manifest_error(pack, source.to_string()))?;
    let texture_data = value
        .get("texture_data")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            manifest_error(pack, "manifest lacks an object-valued texture_data field")
        })?;
    if texture_data.len() > MAX_TEXTURE_ALIASES {
        return Err(manifest_error(pack, "texture_data has too many aliases"));
    }
    for (key, definition) in texture_data {
        let textures = definition
            .get("textures")
            .ok_or_else(|| manifest_error(pack, format!("terrain alias {key} lacks textures")))?;
        let variants = texture_variants(pack, key, textures)?;
        for (variant, texture) in variants.into_iter().enumerate() {
            let path = canonical_texture_path(texture).ok_or_else(|| {
                manifest_error(
                    pack,
                    format!("terrain alias {key} has an unsafe texture path"),
                )
            })?;
            let variant = u32::try_from(variant)
                .map_err(|_| manifest_error(pack, "terrain texture variant index overflows u32"))?;
            routes.insert((key.clone().into_boxed_str(), variant), (pack.into(), path));
        }
    }
    Ok(())
}

fn texture_variants<'a>(
    pack: &str,
    key: &str,
    textures: &'a Value,
) -> Result<Vec<&'a str>, ServerResourcePackError> {
    match textures {
        Value::String(texture) => Ok(vec![texture.as_str()]),
        Value::Object(object) => object
            .get("path")
            .and_then(Value::as_str)
            .map(|texture| vec![texture])
            .ok_or_else(|| {
                manifest_error(
                    pack,
                    format!("terrain alias {key} has an object without path"),
                )
            }),
        Value::Array(values) if !values.is_empty() => {
            if values.len() > MAX_TEXTURE_VARIANTS {
                return Err(manifest_error(
                    pack,
                    format!("terrain alias {key} has too many variants"),
                ));
            }
            values
                .iter()
                .map(|value| match value {
                    Value::String(texture) => Ok(texture.as_str()),
                    Value::Object(object) => {
                        object.get("path").and_then(Value::as_str).ok_or_else(|| {
                            manifest_error(
                                pack,
                                format!("terrain alias {key} has an object variant without path"),
                            )
                        })
                    }
                    _ => Err(manifest_error(
                        pack,
                        format!("terrain alias {key} has an invalid variant"),
                    )),
                })
                .collect()
        }
        _ => Err(manifest_error(
            pack,
            format!("terrain alias {key} has invalid textures"),
        )),
    }
}

fn manifest_error(pack: &str, detail: impl Into<Box<str>>) -> ServerResourcePackError {
    ServerResourcePackError::BlockTextureManifest {
        pack: pack.into(),
        path: "textures/terrain_texture.json".into(),
        detail: detail.into(),
    }
}
