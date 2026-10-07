//! Gathers a pack stack's entity sources: unique definitions, geometry, and referenced rasters.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Component, Path};

use assets::VanillaEntityRefs;
use resource_pack::LayeredPackView;
use serde_json::{Map, Value};

use super::super::resource_packs::parse_pack_json;

const DEFINITION_FAMILIES: [(&str, &str, &str); 3] = [
    ("animations/", "animations", "animations/_pack.json"),
    (
        "animation_controllers/",
        "animation_controllers",
        "animation_controllers/_pack.json",
    ),
    (
        "render_controllers/",
        "render_controllers",
        "render_controllers/_pack.json",
    ),
];

/// Names an actor description refers to, by family.
#[derive(Default)]
struct References {
    render_controllers: BTreeSet<String>,
    animations: BTreeSet<String>,
    animation_controllers: BTreeSet<String>,
    geometry: BTreeSet<String>,
}

impl References {
    fn read(&mut self, actor: &Value) {
        for kind in ["minecraft:client_entity", "minecraft:attachable"] {
            self.read_description(&actor[kind]["description"]);
        }
    }

    fn read_description(&mut self, description: &Value) {
        for entry in description["render_controllers"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let name = entry
                .as_str()
                .map(str::to_owned)
                .or_else(|| entry.as_object()?.keys().next().cloned());
            self.render_controllers.extend(name);
        }
        for target in description["animations"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(_, value)| value.as_str())
        {
            if target.starts_with("controller.animation.") {
                self.animation_controllers.insert(target.to_owned());
            } else {
                self.animations.insert(target.to_owned());
            }
        }
        for entry in description["animation_controllers"]
            .as_array()
            .into_iter()
            .flatten()
        {
            self.animation_controllers.extend(
                entry
                    .as_object()
                    .into_iter()
                    .flatten()
                    .filter_map(|(_, value)| value.as_str().map(str::to_owned)),
            );
        }
        self.geometry.extend(
            description["geometry"]
                .as_object()
                .into_iter()
                .flatten()
                .filter_map(|(_, value)| value.as_str().map(str::to_owned)),
        );
    }
}

/// Every entity source of the stack as `(pack-relative path, bytes)`. Named definitions are
/// merged into one file per family with the highest layer winning each identifier (a vanilla
/// client keeps the last definition it loads); definitions the actors reference but the
/// stack lacks come from `vanilla`. Only rasters the sources name are read.
pub(super) fn collect_files(
    view: &LayeredPackView,
    vanilla: Option<&VanillaEntityRefs>,
    vanilla_pack_dir: Option<&Path>,
) -> Vec<(Box<str>, Vec<u8>)> {
    let mut files = Vec::new();
    let entities = unique_entities(view);
    let attachables = view
        .list("attachables/")
        .into_iter()
        .filter(|path| path.ends_with(".json"))
        .filter_map(|path| {
            let bytes = view.read(path)?;
            Some((Box::<str>::from(path), canonical_json(&bytes)?))
        })
        .collect::<Vec<_>>();
    let mut references = References::default();
    for (_, bytes) in entities.iter().chain(&attachables) {
        if let Ok(actor) = serde_json::from_slice::<Value>(bytes) {
            references.read(&actor);
        }
    }
    let mut geometry = unshadowed_geometry(view);
    let pack_geometry = geometry
        .iter()
        .flat_map(|(_, bytes)| geometry_identifiers(bytes))
        .collect::<BTreeSet<_>>();
    for (prefix, key, merged) in DEFINITION_FAMILIES {
        let mut definitions = merged_definitions(view, prefix, key);
        if let Some(vanilla) = vanilla {
            let (source, wanted) = match key {
                "animations" => (&vanilla.animations, &references.animations),
                "animation_controllers" => (
                    &vanilla.animation_controllers,
                    &references.animation_controllers,
                ),
                _ => (&vanilla.render_controllers, &references.render_controllers),
            };
            for name in wanted {
                if !definitions.contains_key(name)
                    && let Some(definition) = source.get(name)
                {
                    definitions.insert(name.clone(), definition.clone());
                }
            }
        }
        if definitions.is_empty() {
            continue;
        }
        let mut root = Map::new();
        root.insert("format_version".to_owned(), Value::from("1.8.0"));
        root.insert(key.to_owned(), Value::Object(definitions));
        files.push((
            merged.into(),
            serde_json::to_vec(&Value::Object(root)).unwrap_or_default(),
        ));
    }
    if let Some(vanilla) = vanilla {
        // Geometry the actors reference (and its parents) that the stack does not define.
        let mut wanted = references.geometry.iter().cloned().collect::<Vec<_>>();
        let mut added = BTreeSet::new();
        while let Some(identifier) = wanted.pop() {
            if pack_geometry.contains(&identifier) {
                continue;
            }
            let Some(&index) = vanilla.geometry_index.get(&identifier) else {
                continue;
            };
            if added.insert(index)
                && let Some(file) = vanilla.geometry_files.get(index as usize)
            {
                geometry.push((
                    format!("models/vanilla/{}", file.path.trim_start_matches("models/")).into(),
                    file.text.clone().into_bytes(),
                ));
            }
            wanted.extend(vanilla.geometry_parent.get(&identifier).cloned());
        }
    }
    files.extend(entities);
    if let Some(materials) = material_definitions(view) {
        files.push(("materials/_pack.material".into(), materials));
    }
    files.extend(attachables);
    files.extend(geometry);
    for stem in referenced_textures(&files) {
        if let Some(texture) = texture_file(view, vanilla_pack_dir, &stem) {
            files.push(texture);
        }
    }
    files
}

/// All server formats have precedence over the installed vanilla layer.
fn texture_file(
    view: &LayeredPackView,
    vanilla_pack_dir: Option<&Path>,
    stem: &str,
) -> Option<(Box<str>, Vec<u8>)> {
    let paths = [format!("{stem}.png"), format!("{stem}.tga")];
    for path in &paths {
        if let Some(bytes) = view.read(path) {
            return Some((path.clone().into(), bytes.into_vec()));
        }
    }
    let root = vanilla_pack_dir?;
    if !stem.starts_with("textures/")
        || stem.contains(['\\', ':', '\0'])
        || stem
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
        || !Path::new(stem)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return None;
    }
    for path in paths {
        let Ok(file) = std::fs::File::open(root.join(&path)) else {
            continue;
        };
        let mut bytes = Vec::new();
        if file
            .take(resource_pack::MAX_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .is_ok()
            && bytes.len() as u64 <= resource_pack::MAX_FILE_BYTES
        {
            return Some((path.into(), bytes));
        }
    }
    None
}

/// JSON with comments and duplicate keys resolved (the last key wins), re-serialised.
fn canonical_json(bytes: &[u8]) -> Option<Vec<u8>> {
    serde_json::to_vec(&parse_pack_json(bytes)?).ok()
}

/// Material identity is the child name; its parent remains part of the winning declaration.
fn material_definitions(view: &LayeredPackView) -> Option<Vec<u8>> {
    let mut definitions = BTreeMap::new();
    for layer in view.layers() {
        for path in layer
            .files_under("materials/")
            .into_iter()
            .filter(|path| path.ends_with(".material"))
        {
            let Ok(Some(bytes)) = layer.read_file(path) else {
                continue;
            };
            let Some(root) = parse_pack_json(&bytes) else {
                continue;
            };
            let Some(materials) = root.get("materials").and_then(Value::as_object) else {
                continue;
            };
            for (declaration, definition) in materials {
                if !definition.is_object() {
                    continue;
                }
                let child = declaration.split(':').next()?.trim_start_matches('+');
                if !child.is_empty() {
                    definitions.insert(child.to_owned(), (declaration.clone(), definition.clone()));
                }
            }
        }
    }
    if definitions.is_empty() {
        return None;
    }
    let mut materials = definitions.into_values().collect::<Map<_, _>>();
    materials.insert("version".into(), Value::from("1.0.0"));
    serde_json::to_vec(&serde_json::json!({"materials": materials})).ok()
}

/// Named definitions of `prefix` files, lowest layer first so a higher one replaces a name.
fn merged_definitions(view: &LayeredPackView, prefix: &str, key: &str) -> Map<String, Value> {
    let mut merged = Map::new();
    for layer in view.layers() {
        for path in layer.files_under(prefix) {
            if !path.ends_with(".json") {
                continue;
            }
            let Ok(Some(bytes)) = layer.read_file(path) else {
                continue;
            };
            let Some(Value::Object(root)) = parse_pack_json(&bytes) else {
                continue;
            };
            if let Some(Value::Object(definitions)) = root.get(key) {
                merged.extend(
                    definitions
                        .iter()
                        .map(|(name, value)| (name.clone(), value.clone())),
                );
            }
        }
    }
    merged
}

/// The highest layer's `entity/` file for each client entity identifier.
fn unique_entities(view: &LayeredPackView) -> Vec<(Box<str>, Vec<u8>)> {
    let mut owners = BTreeMap::<String, (Box<str>, Vec<u8>)>::new();
    for layer in view.layers() {
        for path in layer.files_under("entity/") {
            if !path.ends_with(".json") {
                continue;
            }
            let Ok(Some(bytes)) = layer.read_file(path) else {
                continue;
            };
            if let Some(identifier) = super::entity_identifier(&bytes)
                && let Some(canonical) = canonical_json(&bytes)
            {
                owners.insert(identifier, (path.into(), canonical));
            }
        }
    }
    let mut seen = BTreeSet::new();
    owners
        .into_values()
        .filter(|(path, _)| seen.insert(path.clone()))
        .collect()
}

/// Geometry files under `models/`, dropping a file when every geometry it defines is
/// redefined by a higher layer or a later file.
fn unshadowed_geometry(view: &LayeredPackView) -> Vec<(Box<str>, Vec<u8>)> {
    let mut winners = BTreeMap::<String, Box<str>>::new();
    let mut candidates = Vec::new();
    for layer in view.layers() {
        for path in layer.files_under("models/") {
            if !path.ends_with(".json") {
                continue;
            }
            let Ok(Some(bytes)) = layer.read_file(path) else {
                continue;
            };
            let identifiers = geometry_identifiers(&bytes);
            if identifiers.is_empty() {
                continue;
            }
            for identifier in identifiers.iter() {
                winners.insert(identifier.clone(), path.into());
            }
            if let Some(canonical) = canonical_json(&bytes) {
                candidates.push((Box::<str>::from(path), canonical, identifiers));
            }
        }
    }
    let mut kept = BTreeMap::<Box<str>, Vec<u8>>::new();
    for (path, bytes, identifiers) in candidates {
        if identifiers
            .iter()
            .any(|identifier| winners.get(identifier) == Some(&path))
        {
            kept.insert(path, bytes);
        }
    }
    kept.into_iter().collect()
}

fn geometry_identifiers(bytes: &[u8]) -> Vec<String> {
    let Some(Value::Object(root)) = parse_pack_json(bytes) else {
        return Vec::new();
    };
    if let Some(Value::Array(entries)) = root.get("minecraft:geometry") {
        return entries
            .iter()
            .filter_map(|entry| {
                entry["description"]["identifier"]
                    .as_str()
                    .map(str::to_owned)
            })
            .collect();
    }
    root.keys()
        .filter(|key| key.starts_with("geometry."))
        .map(|key| key.split(':').next().unwrap_or(key).to_owned())
        .collect()
}

/// Texture path stems (no extension) named by any string in the entity sources.
fn referenced_textures(files: &[(Box<str>, Vec<u8>)]) -> BTreeSet<String> {
    let mut stems = BTreeSet::new();
    for (path, bytes) in files {
        if !(path.starts_with("entity/")
            || path.starts_with("attachables/")
            || path.starts_with("render_controllers/"))
        {
            continue;
        }
        if let Some(root) = parse_pack_json(bytes) {
            collect_texture_strings(&root, &mut stems);
        }
    }
    stems
}

fn collect_texture_strings(value: &Value, stems: &mut BTreeSet<String>) {
    match value {
        Value::String(text) if text.starts_with("textures/") => {
            stems.insert(
                text.trim_end_matches(".png")
                    .trim_end_matches(".tga")
                    .to_owned(),
            );
        }
        Value::Array(items) => items
            .iter()
            .for_each(|item| collect_texture_strings(item, stems)),
        Value::Object(map) => map
            .values()
            .for_each(|item| collect_texture_strings(item, stems)),
        _ => {}
    }
}

#[cfg(test)]
mod tests;
