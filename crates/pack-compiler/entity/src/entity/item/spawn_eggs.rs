//! Spawn eggs resolve icons by actor identifier, not item-atlas key spelling.
//!
//! 26.50.26 reads the actor icon map. Vanilla reads description.spawn_egg texture/texture_index; spawn eggs accept
//! only aux zero.
//! The pinned pack supplies precolored raster variants, including legacy eggs.

use serde_json::Map;

use super::*;

type Definitions = BTreeMap<ItemVisualKey, (u32, ItemVisualDefinitionRoute)>;

struct ActorEgg {
    source: u32,
    minimum_version: [u32; 3],
    egg: Option<Value>,
}

pub(super) fn compile(
    root: &Path,
    payloads: &SourcePayloads,
    sources: &[EntityAssetSource],
    source_indices: &BTreeMap<&str, u32>,
    texture_data: &Map<String, Value>,
    definitions: &mut Definitions,
) -> Result<(), AssetError> {
    let supported_version = version(assets::vanilla_source().tag.trim_start_matches('v'))?;
    let registry = assets::item::names::identifiers().collect::<BTreeSet<_>>();
    let mut actors = BTreeMap::<Box<str>, ActorEgg>::new();
    for (index, source) in sources.iter().enumerate() {
        if !source.path.starts_with("entity/") || !source.path.ends_with(".json") {
            continue;
        }
        let value = read_json(root, payloads, source)?;
        let Some(description) = value
            .get("minecraft:client_entity")
            .and_then(|value| value.get("description"))
        else {
            continue;
        };
        let Some(identifier) = description.get("identifier").and_then(Value::as_str) else {
            continue;
        };
        if !registry.contains(
            assets::facts::pack::rules()
                .egg_identifier(identifier)
                .as_str(),
        ) {
            continue;
        }
        let minimum_version = version(
            description
                .get("min_engine_version")
                .and_then(Value::as_str)
                .unwrap_or("0.0.0"),
        )?;
        if minimum_version > supported_version
            || actors
                .get(identifier)
                .is_some_and(|existing| existing.minimum_version > minimum_version)
        {
            continue;
        }
        let candidate = ActorEgg {
            source: index as u32,
            minimum_version,
            egg: description.get("spawn_egg").cloned(),
        };
        if actors.get(identifier).is_some_and(|existing| {
            existing.minimum_version == minimum_version && existing.egg != candidate.egg
        }) {
            return Err(invalid("ambiguous actor spawn egg definition version"));
        }
        actors.insert(identifier.into(), candidate);
    }
    for (identifier, actor) in &actors {
        // The pinned retail catalog uses the current villager actors. Their old
        // compatibility definitions remain in the same pack under legacy IDs.
        if assets::facts::pack::rules()
            .superseding_egg(identifier)
            .is_some_and(|newer| actors.contains_key(newer))
        {
            continue;
        }
        let key = ItemVisualKey {
            identifier: assets::facts::pack::rules()
                .egg_identifier(identifier)
                .into(),
            metadata: 0,
        };
        let route = resolve(actor.egg.as_ref(), texture_data, source_indices)?;
        if definitions
            .get(&key)
            .is_some_and(|(_, existing)| *existing != route)
        {
            return Err(invalid(
                "actor spawn egg conflicts with an exact item route",
            ));
        }
        definitions.insert(key, (actor.source, route));
    }
    Ok(())
}

fn resolve(
    egg: Option<&Value>,
    texture_data: &Map<String, Value>,
    source_indices: &BTreeMap<&str, u32>,
) -> Result<ItemVisualDefinitionRoute, AssetError> {
    let Some(egg) = egg.and_then(Value::as_object) else {
        return Ok(ItemVisualDefinitionRoute::Missing);
    };
    // Custom color-composited eggs require a separate renderer/carrier contract;
    // don't silently draw their untinted source. Pinned vanilla uses precolored art.
    if egg.contains_key("base_color") || egg.contains_key("overlay_color") {
        return Ok(ItemVisualDefinitionRoute::Missing);
    }
    let Some(definition) = egg
        .get("texture")
        .and_then(Value::as_str)
        .and_then(|alias| texture_data.get(alias))
    else {
        return Ok(ItemVisualDefinitionRoute::Missing);
    };
    let variant = match egg.get("texture_index") {
        None => 0,
        Some(value) => value
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| invalid("actor spawn egg texture index is not u32"))?,
    };
    let variants = parse_texture_variants(definition)?;
    let Some(texture) = variants.get(variant as usize) else {
        return Ok(ItemVisualDefinitionRoute::Missing);
    };
    Ok(source_indices.get(texture.source_path.as_ref()).map_or(
        ItemVisualDefinitionRoute::Missing,
        |source| ItemVisualDefinitionRoute::Sprite {
            texture: ItemTextureReference {
                source: *source,
                variant,
            },
        },
    ))
}

fn version(value: &str) -> Result<[u32; 3], AssetError> {
    let mut components = value.split('.');
    let mut result = [0; 3];
    for component in &mut result {
        *component = components
            .next()
            .and_then(|component| component.parse().ok())
            .ok_or_else(|| invalid("actor spawn egg engine version is invalid"))?;
    }
    Ok(result)
}
