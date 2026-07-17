use std::{collections::BTreeMap, path::Path};

use assets::{
    AssetError, BlockVisualId, EntityAssetSource, ItemDisplayTransform, ItemTextureReference,
    ItemVisualAlias, ItemVisualDefinition, ItemVisualDefinitionRoute, ItemVisualKey,
};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{SourcePayloads, invalid, json::parse_semantic_json};

pub(super) const BLOCK_ITEM_ROUTES: &[u8] =
    include_bytes!("../../../assets/data/block-item-routes-v1001.json");
const BLOCK_REGISTRY: &[u8] = include_bytes!("../../../assets/data/block-registry-v1001.bin");
const ROUTE_SCHEMA: u32 = 1;
const ROUTE_PROTOCOL: u32 = 1001;

pub(super) struct ItemPayload {
    pub block_visual_count: u32,
    pub visuals: Box<[ItemVisualDefinition]>,
    pub aliases: Box<[ItemVisualAlias]>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BlockItemRouteTable {
    schema: u32,
    protocol: u32,
    canonical_block_states: u32,
    dragonfly_module: Box<str>,
    dragonfly_version: Box<str>,
    dragonfly_module_sum: Box<str>,
    breg_sha256: Box<str>,
    routes: Box<[BlockItemRoute]>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BlockItemRoute {
    identifier: Box<str>,
    metadata: u32,
    block_name: Box<str>,
    block_state: Value,
    block_visual: u32,
}

#[derive(Clone)]
struct TextureVariant {
    source_path: Box<str>,
    variant: u32,
}

pub(super) fn compile(
    root: &Path,
    payloads: &SourcePayloads,
    sources: &[EntityAssetSource],
) -> Result<ItemPayload, AssetError> {
    let source_indices = sources
        .iter()
        .enumerate()
        .map(|(index, source)| (source.path.as_ref(), index as u32))
        .collect::<BTreeMap<_, _>>();
    let routes = parse_block_item_routes()?;
    let route_source = *source_indices
        .get("registry/block-item-routes-v1001.json")
        .ok_or_else(|| invalid("reviewed block item authority source is absent"))?;
    let mut definitions = BTreeMap::<ItemVisualKey, (u32, ItemVisualDefinitionRoute)>::new();
    definitions.insert(
        ItemVisualKey {
            identifier: "minecraft:air".into(),
            metadata: 0,
        },
        (route_source, ItemVisualDefinitionRoute::EmptyHand),
    );
    for (key, block_visual) in &routes.routes {
        if key.identifier.as_ref() == "minecraft:air" && key.metadata == 0 {
            continue;
        }
        if definitions
            .insert(
                key.clone(),
                (
                    route_source,
                    ItemVisualDefinitionRoute::BlockItem {
                        block_visual: *block_visual,
                    },
                ),
            )
            .is_some()
        {
            return Err(invalid("duplicate reviewed block item definition"));
        }
    }
    if let Some(atlas_source) = sources
        .iter()
        .find(|source| source.path.as_ref() == "textures/item_texture.json")
    {
        let atlas_index = *source_indices
            .get(atlas_source.path.as_ref())
            .ok_or_else(|| invalid("item texture atlas source is absent"))?;
        let atlas = read_json(root, payloads, atlas_source)?;
        let texture_data = atlas
            .get("texture_data")
            .and_then(Value::as_object)
            .ok_or_else(|| invalid("item texture atlas lacks texture_data"))?;
        for (alias, definition) in texture_data {
            let variants = parse_texture_variants(definition)?;
            for variant in variants {
                let key = ItemVisualKey {
                    identifier: canonical_item_identifier(alias).into(),
                    metadata: variant.variant,
                };
                if routes.routes.contains_key(&key) {
                    continue;
                }
                let route = source_indices.get(variant.source_path.as_ref()).map_or(
                    ItemVisualDefinitionRoute::Missing,
                    |source| ItemVisualDefinitionRoute::Sprite {
                        texture: ItemTextureReference {
                            source: *source,
                            variant: variant.variant,
                        },
                    },
                );
                if definitions.insert(key, (atlas_index, route)).is_some() {
                    return Err(invalid("duplicate exact item texture metadata route"));
                }
            }
        }
    }
    let visuals = definitions
        .into_iter()
        .map(|(key, (source, route))| ItemVisualDefinition {
            key,
            source,
            route,
            first_person: ItemDisplayTransform::identity(),
            third_person: ItemDisplayTransform::identity(),
            dropped: ItemDisplayTransform::identity(),
        })
        .collect::<Vec<_>>();
    Ok(ItemPayload {
        block_visual_count: routes.block_visual_count,
        visuals: visuals.into_boxed_slice(),
        aliases: Box::new([]),
    })
}

struct ReviewedRoutes {
    block_visual_count: u32,
    routes: BTreeMap<ItemVisualKey, BlockVisualId>,
}

fn parse_block_item_routes() -> Result<ReviewedRoutes, AssetError> {
    let table: BlockItemRouteTable =
        serde_json::from_slice(BLOCK_ITEM_ROUTES).map_err(|source| AssetError::Json {
            path: "crates/assets/data/block-item-routes-v1001.json".into(),
            source,
        })?;
    let expected_hash = format!("{:x}", Sha256::digest(BLOCK_REGISTRY));
    if table.schema != ROUTE_SCHEMA
        || table.protocol != ROUTE_PROTOCOL
        || table.canonical_block_states == 0
        || table.breg_sha256.as_ref() != expected_hash
        || table.dragonfly_module.as_ref() != "github.com/df-mc/dragonfly"
        || table.dragonfly_version.is_empty()
        || !table.dragonfly_module_sum.starts_with("h1:")
    {
        return Err(invalid(
            "block item route provenance does not match reviewed inputs",
        ));
    }
    let mut routes = BTreeMap::new();
    for route in table.routes {
        if route.identifier.is_empty()
            || !route.identifier.starts_with("minecraft:")
            || route.block_name.is_empty()
            || !route.block_name.starts_with("minecraft:")
            || !route.block_state.is_object()
            || route.block_visual >= table.canonical_block_states
        {
            return Err(invalid("block item route is noncanonical or out of range"));
        }
        let key = ItemVisualKey {
            identifier: route.identifier,
            metadata: route.metadata,
        };
        if routes
            .insert(key, BlockVisualId(route.block_visual))
            .is_some()
        {
            return Err(invalid("duplicate exact block item route"));
        }
    }
    Ok(ReviewedRoutes {
        block_visual_count: table.canonical_block_states,
        routes,
    })
}

fn parse_texture_variants(definition: &Value) -> Result<Vec<TextureVariant>, AssetError> {
    let textures = definition
        .get("textures")
        .ok_or_else(|| invalid("item texture alias lacks textures"))?;
    let values = match textures {
        Value::String(texture) => vec![texture.as_str()],
        Value::Array(values) if !values.is_empty() => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .ok_or_else(|| invalid("item texture variant must be a string"))
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(invalid("item texture alias has invalid textures")),
    };
    values
        .into_iter()
        .enumerate()
        .map(|(variant, texture)| {
            Ok(TextureVariant {
                source_path: canonical_texture_path(texture).into(),
                variant: u32::try_from(variant)
                    .map_err(|_| invalid("item texture variant exceeds u32"))?,
            })
        })
        .collect()
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

fn read_json(
    root: &Path,
    payloads: &SourcePayloads,
    source: &EntityAssetSource,
) -> Result<Value, AssetError> {
    let path = root.join(source.path.as_ref());
    let bytes = payloads
        .get(source.path.as_ref())
        .ok_or_else(|| invalid("retained item source payload is absent"))?;
    parse_semantic_json(&path, bytes)
}
