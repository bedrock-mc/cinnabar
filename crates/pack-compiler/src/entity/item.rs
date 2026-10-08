use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use assets::{
    AssetError, BlockVisualId, EntityAssetSource, ItemDisplayTransform, ItemTextureReference,
    ItemVisualAlias, ItemVisualDefinition, ItemVisualDefinitionRoute, ItemVisualKey,
};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{
    SourcePayloads, attachable::ItemTransforms, invalid, item_bindings, json::parse_semantic_json,
    legacy_icons,
};

mod spawn_eggs;

pub(super) const BLOCK_ITEM_ROUTES: &[u8] =
    include_bytes!("../../../assets/data/block-item-routes-v2193.json");
const BLOCK_REGISTRY: &[u8] = include_bytes!("../../../assets/data/block-registry-v2193.bin");
const ROUTE_SCHEMA: u32 = 1;
const ROUTE_PROTOCOL: u32 = 2193;
const DRAGONFLY_VERSION: &str = "v0.11.5";
const DRAGONFLY_MODULE_SUM: &str = "h1:amqepXVBRBi/e5j1K2H8GjNFgpMs6FP1RQgNH0Myfn0=";

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

#[derive(Clone, PartialEq)]
struct TextureVariant {
    source_path: Box<str>,
    variant: u32,
}

pub(super) fn compile(
    root: &Path,
    payloads: &SourcePayloads,
    sources: &[EntityAssetSource],
    transforms: &BTreeMap<Box<str>, ItemTransforms>,
) -> Result<ItemPayload, AssetError> {
    let source_indices = sources
        .iter()
        .enumerate()
        .map(|(index, source)| (source.path.as_ref(), index as u32))
        .collect::<BTreeMap<_, _>>();
    let routes = parse_block_item_routes()?;
    let bindings = item_bindings::reviewed()?;
    let legacy = legacy_icons::reviewed()?;
    // Vanilla draws an item as its block only when it is that block's own block item: an item
    // the retail client gives a legacy icon, or one placing a differently named block, keeps
    // its sprite.
    let sprite_first = legacy
        .iter()
        .map(|row| ItemVisualKey {
            identifier: row.identifier.into(),
            metadata: row.metadata,
        })
        .chain(routes.placers.iter().cloned())
        .collect::<BTreeSet<_>>();
    // Banner models replace their legacy sign fallback; beds keep their dye-selected sprites.
    let block_wins = |key: &ItemVisualKey| {
        routes.routes.contains_key(key)
            && (key.identifier.as_ref() == "minecraft:banner"
                || (!sprite_first.contains(key) && key.identifier.as_ref() != "minecraft:bed"))
    };
    let binding_source = *source_indices
        .get(item_bindings::SOURCE_PATH)
        .ok_or_else(|| invalid("reviewed default sprite binding source is absent"))?;
    let route_source = *source_indices
        .get("registry/block-item-routes-v2193.json")
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
                if block_wins(&key) {
                    continue;
                }
                let route = texture_source_index(&source_indices, &variant.source_path).map_or(
                    ItemVisualDefinitionRoute::Missing,
                    |source| ItemVisualDefinitionRoute::Sprite {
                        texture: ItemTextureReference {
                            source,
                            variant: variant.variant,
                        },
                    },
                );
                if definitions
                    .insert(key, (atlas_index, route))
                    .is_some_and(|(_, previous)| {
                        !matches!(previous, ItemVisualDefinitionRoute::BlockItem { .. })
                    })
                {
                    return Err(invalid("duplicate exact item texture metadata route"));
                }
            }
        }
        for binding in bindings {
            let Some(definition) = texture_data.get(binding.default_alias.as_ref()) else {
                // A partial atlas does not authorize inventing an absent route.
                continue;
            };
            let variants = parse_texture_variants(definition)?;
            let variant = variants
                .get(binding.atlas_variant as usize)
                .ok_or_else(|| invalid("default sprite binding variant is absent"))?;
            let key = ItemVisualKey {
                identifier: binding.identifier,
                metadata: 0,
            };
            if routes.routes.contains_key(&key) && !routes.placers.contains(&key) {
                return Err(invalid(
                    "default sprite binding conflicts with a reviewed block route",
                ));
            }
            let canonical_alias = key
                .identifier
                .strip_prefix("minecraft:")
                .unwrap_or(&key.identifier);
            if let Some(existing_definition) = texture_data
                .get(canonical_alias)
                .or_else(|| texture_data.get(key.identifier.as_ref()))
            {
                let existing_variants = parse_texture_variants(existing_definition)?;
                if existing_variants.first() != Some(variant) {
                    return Err(invalid(
                        "default sprite binding conflicts with an exact atlas source",
                    ));
                }
            }
            let route = texture_source_index(&source_indices, &variant.source_path).map_or(
                ItemVisualDefinitionRoute::Missing,
                |source| ItemVisualDefinitionRoute::Sprite {
                    texture: ItemTextureReference {
                        source,
                        variant: variant.variant,
                    },
                },
            );
            if definitions.get(&key).is_some_and(|(_, existing)| {
                *existing != route
                    && !(routes.placers.contains(&key)
                        && matches!(existing, ItemVisualDefinitionRoute::BlockItem { .. }))
            }) {
                return Err(invalid(
                    "default sprite binding conflicts with an exact atlas route",
                ));
            }
            definitions.insert(key, (binding_source, route));
        }
        let legacy_source = *source_indices
            .get(legacy_icons::SOURCE_PATH)
            .ok_or_else(|| invalid("legacy icon route source is absent"))?;
        for legacy in &legacy {
            let key = ItemVisualKey {
                identifier: legacy.identifier.into(),
                metadata: legacy.metadata,
            };
            if block_wins(&key) {
                continue;
            }
            // Exact atlas keys stay authoritative; a legacy icon replaces a block route.
            let exact_sprite = definitions.get(&key).is_some_and(|(_, route)| {
                !matches!(route, ItemVisualDefinitionRoute::BlockItem { .. })
            });
            if exact_sprite {
                continue;
            }
            let Some(definition) = texture_data.get(legacy.atlas_key) else {
                continue;
            };
            let variants = parse_texture_variants(definition)?;
            let Some(variant) = variants.get(legacy.variant) else {
                continue;
            };
            let route = texture_source_index(&source_indices, &variant.source_path).map_or(
                ItemVisualDefinitionRoute::Missing,
                |source| ItemVisualDefinitionRoute::Sprite {
                    texture: ItemTextureReference {
                        source,
                        variant: variant.variant,
                    },
                },
            );
            definitions.insert(key, (legacy_source, route));
        }
        spawn_eggs::compile(
            root,
            payloads,
            sources,
            &source_indices,
            texture_data,
            &mut definitions,
        )?;
    }
    let visuals = definitions
        .into_iter()
        .map(|(key, (source, route))| {
            // Attachable transforms are keyed to the base (metadata 0) variant.
            let literal = (key.metadata == 0)
                .then(|| transforms.get(&key.identifier))
                .flatten();
            let display = |select: fn(&ItemTransforms) -> Option<ItemDisplayTransform>| {
                literal
                    .and_then(select)
                    .unwrap_or_else(ItemDisplayTransform::identity)
            };
            ItemVisualDefinition {
                first_person: display(|transforms| transforms.first_person),
                third_person: display(|transforms| transforms.third_person),
                dropped: display(|transforms| transforms.dropped),
                key,
                source,
                route,
            }
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
    /// Items whose placed block has another name (seeds, signs, string).
    placers: BTreeSet<ItemVisualKey>,
}

fn parse_block_item_routes() -> Result<ReviewedRoutes, AssetError> {
    let table: BlockItemRouteTable =
        serde_json::from_slice(BLOCK_ITEM_ROUTES).map_err(|source| AssetError::Json {
            path: "crates/assets/data/block-item-routes-v2193.json".into(),
            source,
        })?;
    let expected_hash = format!("{:x}", Sha256::digest(BLOCK_REGISTRY));
    validate_route_provenance(&table, &expected_hash)?;
    let mut routes = BTreeMap::new();
    let mut placers = BTreeSet::new();
    let mut reviewed_blocks = BTreeSet::new();
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
        if key.identifier != route.block_name {
            placers.insert(key.clone());
        }
        reviewed_blocks.insert(route.block_name.clone());
        if routes
            .insert(key, BlockVisualId(route.block_visual))
            .is_some()
        {
            return Err(invalid("duplicate exact block item route"));
        }
    }
    add_retail_block_items(&mut routes, &reviewed_blocks)?;
    Ok(ReviewedRoutes {
        block_visual_count: table.canonical_block_states,
        routes,
        placers,
    })
}

/// Retail items named after a registry block the reviewed table omits (saplings, mushrooms,
/// torchflower) are that block's block item, drawn from its first canonical state.
fn add_retail_block_items(
    routes: &mut BTreeMap<ItemVisualKey, BlockVisualId>,
    reviewed_blocks: &BTreeSet<Box<str>>,
) -> Result<(), AssetError> {
    let records = assets::read_registry_for_protocol(BLOCK_REGISTRY, ROUTE_PROTOCOL)?;
    let mut first_state = BTreeMap::new();
    for record in records.iter() {
        first_state
            .entry(record.name.as_ref())
            .or_insert(record.sequential_id);
    }
    let retail = std::str::from_utf8(item_bindings::RETAIL_ITEMS)
        .map_err(|_| invalid("retail item list is not UTF-8"))?;
    for identifier in retail.lines().filter_map(|line| line.split('\t').nth(1)) {
        let key = ItemVisualKey {
            identifier: identifier.into(),
            metadata: 0,
        };
        if reviewed_blocks.contains(identifier) || routes.contains_key(&key) {
            continue;
        }
        if let Some(&state) = first_state.get(identifier)
            && identifier != "minecraft:air"
        {
            routes.insert(key, BlockVisualId(state));
        }
    }
    Ok(())
}

fn validate_route_provenance(
    table: &BlockItemRouteTable,
    expected_hash: &str,
) -> Result<(), AssetError> {
    if table.schema != ROUTE_SCHEMA
        || table.protocol != ROUTE_PROTOCOL
        || table.canonical_block_states == 0
        || table.breg_sha256.as_ref() != expected_hash
        || table.dragonfly_module.as_ref() != "github.com/df-mc/dragonfly"
        || table.dragonfly_version.as_ref() != DRAGONFLY_VERSION
        || table.dragonfly_module_sum.as_ref() != DRAGONFLY_MODULE_SUM
    {
        return Err(invalid(
            "block item route provenance does not match reviewed inputs",
        ));
    }
    Ok(())
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

/// Atlas stems prefer PNG and fall back to TGA, matching terrain texture resolution.
fn texture_source_index(sources: &BTreeMap<&str, u32>, path: &str) -> Option<u32> {
    sources.get(path).copied().or_else(|| {
        let stem = path.strip_suffix(".png")?;
        sources.get(format!("{stem}.tga").as_str()).copied()
    })
}

fn canonical_texture_path(texture: &str) -> String {
    if texture.ends_with(".png") || texture.ends_with(".tga") {
        texture.replace('\\', "/")
    } else {
        format!("{}.png", texture.replace('\\', "/"))
    }
}

/// Resolve the filled-map atlas name while retaining its metadata-selected variants.
fn canonical_item_identifier(alias: &str) -> String {
    if alias == "map_filled" {
        return "minecraft:filled_map".into();
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atlas_stems_resolve_tga_sources_and_prefer_an_existing_png() {
        let mut sources = BTreeMap::from([("textures/items/leather_helmet.tga", 4)]);
        assert_eq!(
            texture_source_index(&sources, "textures/items/leather_helmet.png"),
            Some(4)
        );
        sources.insert("textures/items/leather_helmet.png", 9);
        assert_eq!(
            texture_source_index(&sources, "textures/items/leather_helmet.png"),
            Some(9)
        );
        assert_eq!(
            texture_source_index(&sources, "textures/items/absent.png"),
            None
        );
    }

    #[test]
    fn reviewed_routes_require_the_exact_dragonfly_version_and_module_sum() {
        let expected_hash = format!("{:x}", Sha256::digest(BLOCK_REGISTRY));
        let mut table: BlockItemRouteTable = serde_json::from_slice(BLOCK_ITEM_ROUTES).unwrap();
        table.dragonfly_version = "v0.11.1".into();
        assert!(validate_route_provenance(&table, &expected_hash).is_err());

        let mut table: BlockItemRouteTable = serde_json::from_slice(BLOCK_ITEM_ROUTES).unwrap();
        table.dragonfly_module_sum = "h1:wrong".into();
        assert!(validate_route_provenance(&table, &expected_hash).is_err());
    }
}
