use std::{collections::BTreeMap, fs, path::Path};

use assets::{
    AssetError, BlockVisualId, EntityAssetSource, ItemDisplayScalar, ItemDisplayTransform,
    ItemVisualAlias, ItemVisualDefinition, ItemVisualDefinitionRoute, ItemVisualId,
};
use serde_json::{Map, Value};

use super::{invalid, json::parse_semantic_json};

pub(super) struct ItemPayload {
    pub block_visual_count: u32,
    pub visuals: Box<[ItemVisualDefinition]>,
    pub aliases: Box<[ItemVisualAlias]>,
}

#[derive(Clone)]
struct TextureAlias {
    source_path: Box<str>,
}

pub(super) fn compile(
    root: &Path,
    sources: &[EntityAssetSource],
) -> Result<ItemPayload, AssetError> {
    let source_indices = sources
        .iter()
        .enumerate()
        .map(|(index, source)| (source.path.as_ref(), index as u32))
        .collect::<BTreeMap<_, _>>();
    let Some(atlas_source) = sources
        .iter()
        .find(|source| source.path.as_ref() == "textures/item_texture.json")
    else {
        return Ok(ItemPayload {
            block_visual_count: 0,
            visuals: Box::new([]),
            aliases: Box::new([]),
        });
    };
    let atlas = read_json(root, atlas_source)?;
    let texture_data = atlas
        .get("texture_data")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("item texture atlas lacks texture_data"))?;
    let mut texture_aliases: BTreeMap<Box<str>, TextureAlias> = BTreeMap::new();
    for (alias, definition) in texture_data {
        let textures = definition
            .get("textures")
            .ok_or_else(|| invalid("item texture alias lacks textures"))?;
        let texture = match textures {
            Value::String(texture) => texture.as_str(),
            Value::Array(values) => values
                .first()
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("item texture alias array is empty"))?,
            _ => return Err(invalid("item texture alias has invalid textures")),
        };
        if texture_aliases
            .insert(
                alias.as_str().into(),
                TextureAlias {
                    source_path: canonical_texture_path(texture).into(),
                },
            )
            .is_some()
        {
            return Err(invalid("duplicate item texture alias"));
        }
    }

    let Some(sidecar_source) = sources
        .iter()
        .find(|source| source.path.as_ref() == "textures/item_visuals.json")
    else {
        let atlas_index = *source_indices
            .get(atlas_source.path.as_ref())
            .ok_or_else(|| invalid("item texture atlas source is absent"))?;
        return compile_atlas_defaults(atlas_index, &texture_aliases, &source_indices);
    };
    let sidecar = read_json(root, sidecar_source)?;
    let object = sidecar
        .as_object()
        .ok_or_else(|| invalid("item visual rules must be an object"))?;
    if object
        .keys()
        .any(|field| !matches!(field.as_str(), "block_visual_count" | "items"))
    {
        return Err(invalid("unknown item visual rule field"));
    }
    let block_visual_count = object
        .get("block_visual_count")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid("item visual rules lack block_visual_count"))?;
    let block_visual_count =
        u32::try_from(block_visual_count).map_err(|_| invalid("block visual count exceeds u32"))?;
    let items = object
        .get("items")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("item visual rules lack items"))?;

    let sidecar_index = *source_indices
        .get(sidecar_source.path.as_ref())
        .ok_or_else(|| invalid("item visual source is absent"))?;
    let mut pending_aliases = Vec::<(Box<str>, Box<str>)>::new();
    let mut visuals = Vec::new();
    let mut visual_textures = BTreeMap::<Box<str>, Box<str>>::new();
    for (identifier, definition) in items {
        let definition = definition
            .as_object()
            .ok_or_else(|| invalid("item visual definition must be an object"))?;
        if definition.keys().any(|field| {
            !matches!(
                field.as_str(),
                "texture" | "block_visual" | "aliases" | "display" | "empty_hand"
            )
        }) {
            return Err(invalid("unknown item visual definition field"));
        }
        if definition
            .get("empty_hand")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            if identifier != "minecraft:air" || definition.len() != 1 {
                return Err(invalid("only canonical air may define empty hand"));
            }
            visuals.push(ItemVisualDefinition {
                identifier: identifier.as_str().into(),
                source: sidecar_index,
                route: ItemVisualDefinitionRoute::EmptyHand,
                first_person: ItemDisplayTransform::identity(),
                third_person: ItemDisplayTransform::identity(),
                dropped: ItemDisplayTransform::identity(),
            });
            continue;
        }
        let texture_alias = definition
            .get("texture")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("item visual lacks a texture alias"))?;
        let texture = texture_aliases
            .get(texture_alias)
            .ok_or_else(|| invalid("item visual references an unknown texture alias"))?;
        let block_visual = definition
            .get("block_visual")
            .map(|value| {
                let index = value
                    .as_u64()
                    .and_then(|value| u32::try_from(value).ok())
                    .ok_or_else(|| invalid("block item visual index is invalid"))?;
                if index >= block_visual_count {
                    return Err(invalid("block item visual index exceeds bound"));
                }
                Ok(BlockVisualId(index))
            })
            .transpose()?;
        let route = if let Some(block_visual) = block_visual {
            ItemVisualDefinitionRoute::BlockItem { block_visual }
        } else if let Some(texture_source) = source_indices.get(texture.source_path.as_ref()) {
            ItemVisualDefinitionRoute::Sprite {
                texture_source: *texture_source,
            }
        } else {
            ItemVisualDefinitionRoute::Missing
        };
        let display = definition.get("display").and_then(Value::as_object);
        let visual = ItemVisualDefinition {
            identifier: identifier.as_str().into(),
            source: sidecar_index,
            route,
            first_person: parse_transform(display.and_then(|display| display.get("first_person")))?,
            third_person: parse_transform(display.and_then(|display| display.get("third_person")))?,
            dropped: parse_transform(display.and_then(|display| display.get("dropped")))?,
        };
        if let Some(aliases) = definition.get("aliases") {
            for alias in aliases
                .as_array()
                .ok_or_else(|| invalid("item aliases must be an array"))?
            {
                pending_aliases.push((
                    alias
                        .as_str()
                        .ok_or_else(|| invalid("item alias must be a string"))?
                        .into(),
                    identifier.as_str().into(),
                ));
            }
        }
        visual_textures.insert(identifier.as_str().into(), texture.source_path.clone());
        visuals.push(visual);
    }
    visuals.sort_by(|left, right| left.identifier.cmp(&right.identifier));
    let visual_indices = visuals
        .iter()
        .enumerate()
        .map(|(index, visual)| (visual.identifier.as_ref(), index as u32))
        .collect::<BTreeMap<_, _>>();

    // Texture aliases that resolve to the exact same source reuse one dense visual.
    for (identifier, texture) in &visual_textures {
        for (alias, candidate) in &texture_aliases {
            let canonical_alias = canonical_item_identifier(alias);
            if candidate.source_path == *texture
                && canonical_alias != identifier.as_ref()
                && !visual_indices.contains_key(canonical_alias.as_str())
            {
                pending_aliases.push((canonical_alias.into(), identifier.clone()));
            }
        }
    }
    pending_aliases.sort();
    pending_aliases.dedup();
    let aliases = pending_aliases
        .into_iter()
        .map(|(alias, target)| {
            Ok(ItemVisualAlias {
                identifier: alias,
                visual: ItemVisualId(
                    *visual_indices
                        .get(target.as_ref())
                        .ok_or_else(|| invalid("item alias target is absent"))?,
                ),
            })
        })
        .collect::<Result<Vec<_>, AssetError>>()?;
    Ok(ItemPayload {
        block_visual_count,
        visuals: visuals.into_boxed_slice(),
        aliases: aliases.into_boxed_slice(),
    })
}

fn compile_atlas_defaults(
    atlas_source: u32,
    texture_aliases: &BTreeMap<Box<str>, TextureAlias>,
    source_indices: &BTreeMap<&str, u32>,
) -> Result<ItemPayload, AssetError> {
    let mut groups = BTreeMap::<Box<str>, Vec<Box<str>>>::new();
    for (alias, texture) in texture_aliases {
        groups
            .entry(texture.source_path.clone())
            .or_default()
            .push(canonical_item_identifier(alias).into());
    }
    let mut visuals = vec![ItemVisualDefinition {
        identifier: "minecraft:air".into(),
        source: atlas_source,
        route: ItemVisualDefinitionRoute::EmptyHand,
        first_person: ItemDisplayTransform::identity(),
        third_person: ItemDisplayTransform::identity(),
        dropped: ItemDisplayTransform::identity(),
    }];
    let mut pending_aliases = Vec::<(Box<str>, Box<str>)>::new();
    for (texture_path, mut identifiers) in groups {
        identifiers.sort();
        identifiers.dedup();
        identifiers.retain(|identifier| identifier.as_ref() != "minecraft:air");
        let Some(identifier) = identifiers.first().cloned() else {
            continue;
        };
        let route = source_indices
            .get(texture_path.as_ref())
            .copied()
            .map_or(ItemVisualDefinitionRoute::Missing, |texture_source| {
                ItemVisualDefinitionRoute::Sprite { texture_source }
            });
        visuals.push(ItemVisualDefinition {
            identifier: identifier.clone(),
            source: atlas_source,
            route,
            first_person: ItemDisplayTransform::identity(),
            third_person: ItemDisplayTransform::identity(),
            dropped: ItemDisplayTransform::identity(),
        });
        pending_aliases.extend(
            identifiers
                .into_iter()
                .skip(1)
                .map(|alias| (alias, identifier.clone())),
        );
    }
    visuals.sort_by(|left, right| left.identifier.cmp(&right.identifier));
    let visual_indices = visuals
        .iter()
        .enumerate()
        .map(|(index, visual)| (visual.identifier.as_ref(), index as u32))
        .collect::<BTreeMap<_, _>>();
    pending_aliases.sort();
    let aliases = pending_aliases
        .into_iter()
        .map(|(identifier, target)| {
            Ok(ItemVisualAlias {
                identifier,
                visual: ItemVisualId(
                    *visual_indices
                        .get(target.as_ref())
                        .ok_or_else(|| invalid("default item alias target is absent"))?,
                ),
            })
        })
        .collect::<Result<Vec<_>, AssetError>>()?;
    Ok(ItemPayload {
        block_visual_count: 0,
        visuals: visuals.into_boxed_slice(),
        aliases: aliases.into_boxed_slice(),
    })
}

fn parse_transform(value: Option<&Value>) -> Result<ItemDisplayTransform, AssetError> {
    let Some(value) = value else {
        return Ok(ItemDisplayTransform::identity());
    };
    let value = value
        .as_object()
        .ok_or_else(|| invalid("item display transform must be an object"))?;
    if value
        .keys()
        .any(|field| !matches!(field.as_str(), "translation" | "rotation" | "scale"))
    {
        return Err(invalid("unknown item display transform field"));
    }
    let identity = ItemDisplayTransform::identity();
    Ok(ItemDisplayTransform {
        translation: parse_vector(value, "translation", identity.translation)?,
        rotation: parse_vector(value, "rotation", identity.rotation)?,
        scale: parse_vector(value, "scale", identity.scale)?,
    })
}

fn parse_vector(
    object: &Map<String, Value>,
    field: &str,
    default: [ItemDisplayScalar; 3],
) -> Result<[ItemDisplayScalar; 3], AssetError> {
    let Some(value) = object.get(field) else {
        return Ok(default);
    };
    let values = value
        .as_array()
        .filter(|values| values.len() == 3)
        .ok_or_else(|| invalid("item display vector must have three values"))?;
    Ok([
        display_scalar(&values[0])?,
        display_scalar(&values[1])?,
        display_scalar(&values[2])?,
    ])
}

fn display_scalar(value: &Value) -> Result<ItemDisplayScalar, AssetError> {
    let value = value
        .as_f64()
        .ok_or_else(|| invalid("item display scalar must be finite numeric"))?
        as f32;
    ItemDisplayScalar::new(value).ok_or_else(|| invalid("item display scalar exceeds bound"))
}

fn canonical_texture_path(texture: &str) -> String {
    if texture.ends_with(".png") || texture.ends_with(".tga") {
        texture.replace('\\', "/")
    } else {
        format!("{}.png", texture.replace('\\', "/"))
    }
}

fn canonical_item_identifier(alias: &str) -> String {
    if alias.contains(':') {
        alias.to_owned()
    } else {
        format!("minecraft:{alias}")
    }
}

fn read_json(root: &Path, source: &EntityAssetSource) -> Result<Value, AssetError> {
    let path = root.join(source.path.as_ref());
    let bytes = fs::read(&path).map_err(|source| AssetError::Io {
        path: path.clone(),
        source,
    })?;
    parse_semantic_json(&path, &bytes)
}
