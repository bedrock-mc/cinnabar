//! Gathers a pack stack's entity sources: unique definitions, geometry, and referenced rasters.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Component, Path};

use assets::VanillaEntityRefs;
use rayon::prelude::*;
use resource_pack::LayeredPackView;
use serde_json::{Map, Value};

use crate::pack_textures::parse_pack_json;

#[cfg(test)]
thread_local! {
    static PEAK_ENTITY_PAYLOAD_BYTES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static PEAK_DEFINITION_DOCUMENTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static PEAK_GEOMETRY_BATCH_BYTES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

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
    textures: BTreeSet<String>,
}

/// Selected entity payloads and references extracted during the same collection parse.
struct CollectedEntities {
    files: Vec<(Box<str>, Vec<u8>)>,
    references: References,
}

impl References {
    /// Extracts all actor references while its document is available.
    fn read(&mut self, actor: &Value) {
        collect_texture_strings(actor, &mut self.textures);
        for kind in ["minecraft:client_entity", "minecraft:attachable"] {
            self.read_description(&actor[kind]["description"]);
        }
    }

    /// Combines references without retaining either source document.
    fn extend(&mut self, other: Self) {
        self.render_controllers.extend(other.render_controllers);
        self.animations.extend(other.animations);
        self.animation_controllers
            .extend(other.animation_controllers);
        self.geometry.extend(other.geometry);
        self.textures.extend(other.textures);
    }

    /// Extracts the definition families named by one actor description.
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
/// stack lacks come from `vanilla`. Reads named rasters, painting overrides, and shared
/// enchantment rasters. Missing painting overrides remain tracked reload dependencies.
pub fn collect_files(
    view: &LayeredPackView,
    vanilla: Option<&VanillaEntityRefs>,
    vanilla_pack_dir: Option<&Path>,
) -> Vec<(Box<str>, Vec<u8>)> {
    let mut files = Vec::new();
    let CollectedEntities {
        files: entities,
        mut references,
    } = unique_entities(view);
    let mut geometry = unshadowed_geometry(view);
    let mut pack_geometry = BTreeSet::new();
    for batch in bounded_batches(&geometry, |(_, bytes)| bytes.len() as u64) {
        pack_geometry.par_extend(
            batch
                .par_iter()
                .flat_map_iter(|(_, bytes)| geometry_identifiers(bytes)),
        );
    }
    let definitions = DEFINITION_FAMILIES
        .map(|(prefix, key, merged)| (key, merged, merged_definitions(view, prefix, key)));
    let mut overrides = pack_geometry.clone();
    for (_, _, definitions) in &definitions {
        overrides.extend(definitions.keys().cloned());
    }
    let materials = material_definitions(view);
    let material_document = materials.as_deref().and_then(parse_pack_json);
    let binding_inputs = pack_entity_compiler::EquipmentBindingDependencies::load(
        vanilla_pack_dir,
        material_document.as_ref(),
    )
    .ok();
    if let Some(inputs) = &binding_inputs {
        overrides.extend(inputs.authored_materials().iter().cloned());
    }
    let attachables = attachables::collect(
        view,
        vanilla_pack_dir,
        &overrides,
        vanilla,
        binding_inputs.as_ref(),
    );
    for (_, bytes) in &attachables {
        if let Ok(actor) = bedrock_packs::jsonc::parse_jsonc::<Value>(bytes) {
            references.read(&actor);
        }
    }
    for (key, merged, mut definitions) in definitions {
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
    let mut textures = referenced_textures(&files);
    textures.extend(references.textures);
    files.extend(entities);
    if let Some(materials) = materials {
        files.push(("materials/_pack.material".into(), materials));
    }
    files.extend(attachables);
    files.extend(geometry);
    textures.extend(assets::entity::GLINT_TEXTURE_IDENTIFIERS.map(str::to_owned));
    let paintings = assets::painting::PAINTING_MOTIVES
        .iter()
        .map(|motive| motive.texture())
        .chain(std::iter::once(assets::painting::PAINTING_ATLAS.to_owned()))
        .filter(|stem| !textures.contains(stem))
        .collect::<BTreeSet<_>>();
    textures.extend(paintings.iter().cloned());
    for stem in textures {
        let fallback = if paintings.contains(&stem) {
            None
        } else {
            vanilla_pack_dir
        };
        if let Some(texture) = texture_file(view, fallback, &stem) {
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

/// Splits `items` into parallel batches bounded by workers and one source's byte budget;
/// a larger item runs alone.
fn bounded_batches<T>(mut items: &[T], size: impl Fn(&T) -> u64) -> Vec<&[T]> {
    let mut batches = Vec::new();
    while !items.is_empty() {
        let end = items.len().min(rayon::current_num_threads());
        let mut bytes = 0u64;
        let mut len = end;
        for (index, item) in items[..end].iter().enumerate() {
            let next = bytes.saturating_add(size(item));
            if index > 0 && next > assets::MAX_ENTITY_SOURCE_BYTES as u64 {
                len = index;
                break;
            }
            bytes = next;
        }
        let (batch, rest) = items.split_at(len);
        batches.push(batch);
        items = rest;
    }
    batches
}

/// A JSON source's stored size bounds its parse; other files are not parsed.
fn json_size(path: &str, size: Option<u64>) -> u64 {
    if path.ends_with(".json") {
        size.unwrap_or(0)
    } else {
        0
    }
}

/// Named definitions of `prefix` files, lowest layer first so a higher one replaces a name.
fn merged_definitions(view: &LayeredPackView, prefix: &str, key: &str) -> Map<String, Value> {
    let mut merged = Map::new();
    for layer in view.layers() {
        let paths = layer.files_under(prefix);
        for batch in bounded_batches(&paths, |path| json_size(path, layer.file_size(path))) {
            let definitions = batch
                .par_iter()
                .copied()
                .map(|path| {
                    if !path.ends_with(".json") {
                        return None;
                    }
                    let bytes = layer.read_file(path).ok()??;
                    let Value::Object(mut root) = parse_pack_json(&bytes)? else {
                        return None;
                    };
                    match root.remove(key) {
                        Some(Value::Object(definitions)) => Some(definitions),
                        _ => None,
                    }
                })
                .collect::<Vec<_>>();
            #[cfg(test)]
            PEAK_DEFINITION_DOCUMENTS.update(|peak| {
                peak.max(definitions.iter().flatten().count() + usize::from(!merged.is_empty()))
            });
            for definitions in definitions.into_iter().flatten() {
                merged.extend(definitions);
            }
        }
    }
    merged
}

/// The highest layer's entity file and extracted references for each client entity identifier.
fn unique_entities(view: &LayeredPackView) -> CollectedEntities {
    let mut owners = BTreeMap::new();
    for layer in view.layers() {
        let paths = layer.files_under("entity/");
        for batch in bounded_batches(&paths, |path| json_size(path, layer.file_size(path))) {
            let entities = batch
                .par_iter()
                .copied()
                .map(|path| {
                    if !path.ends_with(".json") {
                        return None;
                    }
                    let bytes = layer.read_file(path).ok()??;
                    let document = parse_pack_json(&bytes)?;
                    let identifier = document
                        .pointer("/minecraft:client_entity/description/identifier")?
                        .as_str()?
                        .to_owned();
                    let canonical = serde_json::to_vec(&document).ok()?;
                    let mut references = References::default();
                    references.read(&document);
                    Some((identifier, (Box::<str>::from(path), canonical, references)))
                })
                .collect::<Vec<_>>();
            #[cfg(test)]
            PEAK_ENTITY_PAYLOAD_BYTES.update(|peak| {
                peak.max(
                    entities
                        .iter()
                        .flatten()
                        .map(|(_, (_, bytes, _))| bytes.len())
                        .sum::<usize>()
                        + owners
                            .values()
                            .map(|(_, bytes, _): &(Box<str>, Vec<u8>, References)| bytes.len())
                            .sum::<usize>(),
                )
            });
            for (identifier, file) in entities.into_iter().flatten() {
                owners.insert(identifier, file);
            }
        }
    }
    let mut seen = BTreeSet::new();
    let mut references = References::default();
    let files = owners
        .into_values()
        .filter_map(|(path, bytes, extracted)| {
            if !seen.insert(path.clone()) {
                return None;
            }
            references.extend(extracted);
            Some((path, bytes))
        })
        .collect();
    CollectedEntities { files, references }
}

/// Collect geometry documents in layer order for identifier selection by the compiler.
fn unshadowed_geometry(view: &LayeredPackView) -> Vec<(Box<str>, Vec<u8>)> {
    let mut files = Vec::new();
    for layer in view.layers() {
        let paths = layer.files_under("models/");
        for batch in bounded_batches(&paths, |path| json_size(path, layer.file_size(path))) {
            #[cfg(test)]
            PEAK_GEOMETRY_BATCH_BYTES.update(|peak| {
                peak.max(
                    batch
                        .iter()
                        .filter_map(|path| layer.file_size(path))
                        .sum::<u64>(),
                )
            });
            let parsed = batch
                .par_iter()
                .copied()
                .map(|path| {
                    if !path.ends_with(".json") {
                        return None;
                    }
                    let bytes = layer.read_file(path).ok()??;
                    Some((path.into(), canonical_json(&bytes)?))
                })
                .collect::<Vec<_>>();
            files.extend(parsed.into_iter().flatten());
        }
    }
    pack_entity_compiler::select_entity_geometry(files)
}

/// List modern and legacy identifiers used to fill missing geometry references.
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
    for batch in bounded_batches(files, |(_, bytes)| bytes.len() as u64) {
        stems.extend(batch_texture_stems(batch));
    }
    stems
}

/// Texture stems named by one bounded batch of sources.
fn batch_texture_stems(files: &[(Box<str>, Vec<u8>)]) -> BTreeSet<String> {
    files
        .par_iter()
        .map(|(path, bytes)| {
            let mut stems = BTreeSet::new();
            if (path.starts_with("entity/")
                || path.starts_with("attachables/")
                || path.starts_with("render_controllers/"))
                && let Some(root) = parse_pack_json(bytes)
            {
                collect_texture_strings(&root, &mut stems);
            }
            stems
        })
        .reduce(BTreeSet::new, |mut left, right| {
            left.extend(right);
            left
        })
}

/// Finds texture stems recursively while a source document is available.
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

mod attachables;

#[cfg(test)]
mod tests;
