//! Completes definitions the pinned samples omit from resources the pack does ship: harness
//! bindings and the render controllers named by the pack policy.

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
    let authored = payloads
        .iter()
        .filter(|(path, _)| path.starts_with("attachables/"))
        .map(|(path, bytes)| parse_unique_json(&root.join(path.as_ref()), bytes))
        .collect::<Result<Vec<_>, _>>()?;
    let mut missing =
        missing_harness_bindings(|path| payloads.contains_key(path), authored.iter())?;
    missing.extend(missing_render_controllers(root, payloads, symbols)?);
    let mut size = 0;
    for (source, value) in missing {
        let bytes = serde_json::to_vec(&value)
            .map_err(|_| invalid("harness binding could not be encoded"))?;
        parse_source(
            &source,
            &root.join(source.as_ref()),
            &bytes,
            symbols,
            geometries,
        )?;
        sources.push(EntityAssetSource {
            path: source.clone(),
            source_bytes: bytes.len() as u32,
            source_sha256: Sha256::digest(&bytes).into(),
        });
        size += bytes.len();
        payloads.insert(source, bytes.into());
    }
    Ok(size)
}

/// Policy-named render controllers that an entity references but no source defines, each as a
/// document holding a copy of the pack's template controller.
fn missing_render_controllers(
    root: &Path,
    payloads: &SourcePayloads,
    symbols: &BTreeMap<(EntityAssetKind, Box<str>, Box<str>), PendingSymbol>,
) -> Result<Vec<(Box<str>, Value)>, AssetError> {
    let paths = |kind: EntityAssetKind, name: Option<&str>| {
        symbols
            .keys()
            .filter(move |(symbol, identifier, _)| {
                *symbol == kind && name.is_none_or(|name| &**identifier == name)
            })
            .map(|(_, _, path)| path)
            .collect::<Vec<_>>()
    };
    let entities = paths(EntityAssetKind::Entity, None)
        .into_iter()
        .map(|path| parse_unique_json(&root.join(path.as_ref()), &payloads[path]))
        .collect::<Result<Vec<_>, _>>()?;
    let mut missing = Vec::new();
    for (name, template) in assets::facts::pack::rules().omitted_render_controllers() {
        let referenced = entities.iter().any(|entity| {
            entity["minecraft:client_entity"]["description"]["render_controllers"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|entry| entry.as_str() == Some(name) || entry.get(name).is_some())
        });
        let [template_path] = paths(EntityAssetKind::RenderController, Some(template))[..] else {
            continue;
        };
        if !referenced || !paths(EntityAssetKind::RenderController, Some(name)).is_empty() {
            continue;
        }
        let document =
            parse_unique_json(&root.join(template_path.as_ref()), &payloads[template_path])?;
        let definition = document["render_controllers"][template].clone();
        if !definition.is_object() {
            continue;
        }
        let mut controllers = serde_json::Map::new();
        controllers.insert(name.to_owned(), definition);
        missing.push((
            format!("render_controllers/_cinnabar/omitted/{name}.json").into(),
            serde_json::json!({
                "format_version": document["format_version"],
                "render_controllers": controllers,
            }),
        ));
    }
    Ok(missing)
}

/// Supplies omitted harness definitions from present resources without replacing authored items.
pub fn missing_harness_bindings<'a>(
    has_source: impl Fn(&str) -> bool,
    authored: impl IntoIterator<Item = &'a Value>,
) -> Result<Vec<(Box<str>, Value)>, AssetError> {
    if ![MODEL, CONTROLLERS].iter().all(|path| has_source(path)) {
        return Ok(Vec::new());
    }
    let facts =
        assets::facts::ItemFacts::builtin().map_err(|_| invalid("item facts are unavailable"))?;
    let Some(tag) = facts.tag(assets::facts::HARNESS_TAG) else {
        return Ok(Vec::new());
    };
    let authored: BTreeSet<Box<str>> = authored
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
    let mut missing = Vec::new();
    for record in &facts.payload().records {
        if !record.fact.tags.contains(tag) {
            continue;
        }
        let Some(color) = assets::facts::harness_color(&record.identifier) else {
            continue;
        };
        let texture = format!("textures/entity/harness/harness_{color}");
        if !["png", "tga"]
            .iter()
            .any(|extension| has_source(&format!("{texture}.{extension}")))
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
        missing.push((source.into(), value));
    }
    Ok(missing)
}

#[cfg(test)]
mod tests;
