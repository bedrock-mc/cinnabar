//! Completes omitted startup bindings using the pack's harness artwork and controller.

use super::*;

const GEOMETRY: &str = "geometry.harness";
const MODEL: &str = "models/entity/harness.geo.json";
const CONTROLLERS: &str = "render_controllers/harness.render_controllers.json";
const CONTROLLER: &str = "controller.render.harness";

/// Adds missing sample definitions only when their referenced runtime resources are present.
pub(super) fn complete_bindings(
    root: &Path,
    sources: &mut Vec<EntityAssetSource>,
    payloads: &mut SourcePayloads,
    symbols: &mut BTreeMap<(EntityAssetKind, Box<str>, Box<str>), PendingSymbol>,
    geometries: &mut BTreeMap<(Box<str>, Box<str>), PendingGeometry>,
) -> Result<usize, AssetError> {
    if ![MODEL, CONTROLLERS]
        .iter()
        .all(|path| payloads.contains_key(*path))
    {
        return Ok(0);
    }
    let facts =
        assets::facts::ItemFacts::builtin().map_err(|_| invalid("item facts are unavailable"))?;
    let Some(tag) = facts.tag(assets::facts::HARNESS_TAG) else {
        return Ok(0);
    };
    let authored: BTreeSet<Box<str>> = payloads
        .iter()
        .filter(|(path, _)| path.starts_with("attachables/"))
        .map(|(path, bytes)| parse_unique_json(&root.join(path.as_ref()), bytes))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flat_map(|value| {
            let description = &value["minecraft:attachable"]["description"];
            if let Some(items) = description["item"].as_object() {
                items
                    .keys()
                    .map(|item| item.clone().into_boxed_str())
                    .collect::<Vec<_>>()
            } else {
                description["identifier"]
                    .as_str()
                    .map(Box::<str>::from)
                    .into_iter()
                    .collect()
            }
        })
        .collect();
    let mut size = 0;
    for record in &facts.payload().records {
        if !record.fact.tags.contains(tag) {
            continue;
        }
        let Some(color) = record
            .identifier
            .strip_prefix("minecraft:")
            .and_then(|name| name.strip_suffix("_harness"))
        else {
            continue;
        };
        let texture = format!("textures/entity/harness/harness_{color}");
        if !["png", "tga"]
            .iter()
            .any(|extension| payloads.contains_key(format!("{texture}.{extension}").as_str()))
            || authored.contains(&record.identifier)
        {
            continue;
        }
        let source = format!(
            "attachables/_cinnabar/{}.json",
            record.identifier.replace(':', "/")
        );
        let value = serde_json::json!({
            "format_version":"1.10.0",
            "minecraft:attachable":{"description":{
                "identifier":record.identifier,
                "materials":{"default":"harness"},
                "textures":{"default":texture},
                "geometry":{"default":GEOMETRY},
                "render_controllers":[CONTROLLER]
            }}
        });
        let bytes = serde_json::to_vec(&value)
            .map_err(|_| invalid("harness binding could not be encoded"))?;
        parse_source(&source, &root.join(&source), &bytes, symbols, geometries)?;
        sources.push(EntityAssetSource {
            path: source.clone().into(),
            source_bytes: bytes.len() as u32,
            source_sha256: Sha256::digest(&bytes).into(),
        });
        size += bytes.len();
        payloads.insert(source.into(), bytes.into());
    }
    Ok(size)
}

#[cfg(test)]
mod tests;
