use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use assets::{
    AssetError, EntityAssetKind, EntityAssetSource, EntityAssetSymbol, EntityGeometry,
    MAX_MOLANG_COLLECTION_ITEMS, validate_entity_geometry_inheritance,
};
use serde_json::{Map, Value};

use super::{
    super::{SourcePayloads, invalid},
    clip::{read_json, required_object},
};
use crate::entity::molang::MolangCompiler;

pub(super) struct EntityEnvironment {
    pub entity_symbol: u32,
    pub geometry: Option<u32>,
    pub geometry_aliases: BTreeMap<Box<str>, Box<str>>,
    pub render_controllers: Vec<Box<str>>,
    pub animation_aliases: BTreeMap<Box<str>, Box<str>>,
    pub controller_aliases: BTreeMap<Box<str>, Box<str>>,
}

pub(super) struct SelectableGeometry {
    pub geometry: u32,
    pub condition: u32,
}

pub(super) enum GeometrySelection {
    Supported(Box<[SelectableGeometry]>),
    Unsupported,
}

pub(super) type GeometrySelections = BTreeMap<(Box<str>, u32), GeometrySelection>;

pub(super) fn collect(
    root: &Path,
    payloads: &SourcePayloads,
    sources: &[EntityAssetSource],
    symbols: &[EntityAssetSymbol],
    geometries: &[EntityGeometry],
) -> Result<Vec<EntityEnvironment>, AssetError> {
    let geometry_indices = unique_geometry_indices(geometries);
    let mut environments = Vec::new();
    for (entity_symbol, entity) in symbols
        .iter()
        .enumerate()
        .filter(|(_, symbol)| symbol.kind == EntityAssetKind::Entity)
    {
        let source = &sources[entity.source_index as usize];
        let value = read_json(root, payloads, source)?;
        let description = value
            .get("minecraft:client_entity")
            .and_then(|value| value.get("description"))
            .and_then(Value::as_object)
            .ok_or_else(|| invalid("client entity description is absent"))?;
        let geometry_aliases = description
            .get("geometry")
            .and_then(Value::as_object)
            .map(|aliases| {
                aliases
                    .iter()
                    .filter_map(|(alias, target)| {
                        target
                            .as_str()
                            .map(|target| (alias.as_str().into(), target.into()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let geometry = default_geometry(description.get("geometry"))
            .and_then(|identifier| geometry_indices.get(identifier).copied().flatten());
        let render_controllers = description
            .get("render_controllers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|entry| match entry {
                Value::String(identifier) => Some(identifier.as_str().into()),
                Value::Object(condition) if condition.len() == 1 => condition
                    .keys()
                    .next()
                    .map(|identifier| identifier.as_str().into()),
                _ => None,
            })
            .collect();
        let animation_aliases = parse_aliases(description.get("animations"))?;
        let mut controller_aliases = parse_aliases(description.get("animation_controllers"))?;
        for (alias, target) in &animation_aliases {
            if target.starts_with("controller.animation.") {
                match controller_aliases.get(alias) {
                    Some(existing) if existing != target => {
                        return Err(invalid("conflicting entity controller alias"));
                    }
                    Some(_) => {}
                    None => {
                        controller_aliases.insert(alias.clone(), target.clone());
                    }
                }
            }
        }
        environments.push(EntityEnvironment {
            entity_symbol: entity_symbol as u32,
            geometry,
            geometry_aliases,
            render_controllers,
            animation_aliases,
            controller_aliases,
        });
    }
    Ok(environments)
}

pub(super) fn compile_geometry_selections(
    root: &Path,
    payloads: &SourcePayloads,
    sources: &[EntityAssetSource],
    geometries: &[EntityGeometry],
    environments: &[EntityEnvironment],
    molang: &mut MolangCompiler,
) -> Result<GeometrySelections, AssetError> {
    let geometry_indices = unique_geometry_indices(geometries);
    let mut selections = BTreeMap::new();
    for source in sources
        .iter()
        .filter(|source| source.path.starts_with("render_controllers/"))
    {
        let value = read_json(root, payloads, source)?;
        for (controller_name, definition) in required_object(&value, "render_controllers")? {
            let definition = definition
                .as_object()
                .ok_or_else(|| invalid("render controller must be an object"))?;
            for environment in environments.iter().filter(|environment| {
                environment.geometry.is_some()
                    && environment
                        .render_controllers
                        .iter()
                        .any(|identifier| identifier.as_ref() == controller_name)
            }) {
                let mut transaction = molang.clone();
                let mut local_collections = BTreeMap::<Box<str>, Box<[u32]>>::new();
                if let Some(arrays) = definition.get("arrays") {
                    let arrays = arrays
                        .as_object()
                        .ok_or_else(|| invalid("render-controller arrays must be an object"))?;
                    if let Some(geometries_array) = arrays.get("geometries") {
                        let geometries_array = geometries_array.as_object().ok_or_else(|| {
                            invalid("render-controller arrays.geometries is invalid")
                        })?;
                        for (name, members) in geometries_array {
                            let members = members.as_array().ok_or_else(|| {
                                invalid("render-controller collection must be an array")
                            })?;
                            if members.is_empty() || members.len() > MAX_MOLANG_COLLECTION_ITEMS {
                                return Err(invalid(
                                    "render geometry collection member count exceeds bound",
                                ));
                            }
                            let values = members
                                .iter()
                                .map(|member| {
                                    let member = member.as_str().ok_or_else(|| {
                                        invalid("collection member must be a name")
                                    })?;
                                    let identifier = member
                                        .strip_prefix("Geometry.")
                                        .and_then(|alias| environment.geometry_aliases.get(alias))
                                        .map_or(member, |identifier| identifier.as_ref());
                                    geometry_indices
                                        .get(identifier)
                                        .copied()
                                        .flatten()
                                        .ok_or_else(|| {
                                            invalid(format!(
                                                "unknown or ambiguous geometry collection member `{member}`"
                                            ))
                                        })
                                })
                                .collect::<Result<Vec<_>, AssetError>>()?;
                            local_collections
                                .insert(name.as_str().into(), values.into_boxed_slice());
                        }
                    }
                }
                if let Some(geometry) = definition.get("geometry").and_then(Value::as_str)
                    && let Some((collection, index)) = split_collection_selection(geometry)
                {
                    let key = (controller_name.as_str().into(), environment.entity_symbol);
                    let Some(candidates) = local_collections.get(collection) else {
                        selections.insert(key, GeometrySelection::Unsupported);
                        continue;
                    };
                    let maximum = candidates.len() - 1;
                    let compiled = candidates
                        .iter()
                        .enumerate()
                        .map(|(candidate_index, geometry)| {
                            transaction
                                .compile(&format!(
                                    "math.clamp(math.floor(({index})), 0, {maximum}) == {candidate_index}"
                                ))
                                .map(|condition| SelectableGeometry {
                                    geometry: *geometry,
                                    condition,
                                })
                        })
                        .collect::<Result<Vec<_>, AssetError>>();
                    match compiled {
                        Ok(candidates) if !candidates.is_empty() => {
                            *molang = transaction;
                            selections.insert(
                                key,
                                GeometrySelection::Supported(candidates.into_boxed_slice()),
                            );
                        }
                        Err(_) => {
                            selections.insert(key, GeometrySelection::Unsupported);
                        }
                        Ok(_) => return Err(invalid("render geometry collection is empty")),
                    }
                }
            }
        }
    }
    Ok(selections)
}

pub(super) fn selection_for<'a>(
    environment: &EntityEnvironment,
    selections: &'a GeometrySelections,
) -> Option<&'a GeometrySelection> {
    environment
        .render_controllers
        .iter()
        .find_map(|controller| selections.get(&(controller.clone(), environment.entity_symbol)))
}

pub(super) fn unique_geometry_indices(
    geometries: &[EntityGeometry],
) -> BTreeMap<Box<str>, Option<u32>> {
    let mut indices = BTreeMap::new();
    for (index, geometry) in geometries.iter().enumerate() {
        indices
            .entry(geometry.identifier.clone())
            .and_modify(|value| *value = None)
            .or_insert(Some(index as u32));
    }
    indices
}

pub(super) fn default_geometry(value: Option<&Value>) -> Option<&str> {
    match value? {
        Value::String(value) => Some(value),
        Value::Object(values) => values.get("default").and_then(Value::as_str),
        _ => None,
    }
}

pub(super) fn controller_clip_references(
    root: &Path,
    payloads: &SourcePayloads,
    sources: &[EntityAssetSource],
) -> Result<BTreeMap<Box<str>, BTreeSet<Box<str>>>, AssetError> {
    let mut output = BTreeMap::new();
    for source in sources
        .iter()
        .filter(|source| source.path.starts_with("animation_controllers/"))
    {
        let value = read_json(root, payloads, source)?;
        for (identifier, definition) in required_object(&value, "animation_controllers")? {
            let mut references = BTreeSet::new();
            if let Some(states) = definition.get("states").and_then(Value::as_object) {
                for state in states.values().filter_map(Value::as_object) {
                    for animation in state
                        .get("animations")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                    {
                        match animation {
                            Value::String(identifier) => {
                                references.insert(identifier.as_str().into());
                            }
                            Value::Object(weighted) if weighted.len() == 1 => {
                                references.insert(
                                    weighted
                                        .keys()
                                        .next()
                                        .expect("one checked key")
                                        .as_str()
                                        .into(),
                                );
                            }
                            _ => {}
                        }
                    }
                }
            }
            output.insert(identifier.as_str().into(), references);
        }
    }
    Ok(output)
}

pub(super) fn parse_aliases(
    value: Option<&Value>,
) -> Result<BTreeMap<Box<str>, Box<str>>, AssetError> {
    let mut aliases = BTreeMap::new();
    let mut insert_object = |values: &Map<String, Value>| -> Result<(), AssetError> {
        for (alias, target) in values {
            let target = target
                .as_str()
                .ok_or_else(|| invalid("entity animation alias target must be a string"))?;
            if aliases
                .insert(alias.as_str().into(), target.into())
                .is_some()
            {
                return Err(invalid("duplicate entity animation alias"));
            }
        }
        Ok(())
    };
    match value {
        None => {}
        Some(Value::Object(values)) => insert_object(values)?,
        Some(Value::Array(values)) => {
            for value in values {
                insert_object(
                    value
                        .as_object()
                        .ok_or_else(|| invalid("entity animation aliases must be objects"))?,
                )?;
            }
        }
        Some(_) => return Err(invalid("entity animation aliases have an invalid shape")),
    }
    Ok(aliases)
}

pub(super) fn effective_bone_names(
    geometries: &[EntityGeometry],
) -> Result<Vec<Box<[Box<str>]>>, AssetError> {
    let parents = validate_entity_geometry_inheritance(geometries)?;
    let mut output = Vec::with_capacity(geometries.len());
    for start in 0..geometries.len() {
        let mut chain = Vec::new();
        let mut current = Some(start);
        while let Some(index) = current {
            chain.push(index);
            current = parents[index];
        }
        let mut names = Vec::<Box<str>>::new();
        for index in chain.into_iter().rev() {
            for bone in &geometries[index].bones {
                let name = bone.name.to_ascii_lowercase().into_boxed_str();
                if !names.iter().any(|candidate| candidate == &name) {
                    names.push(name);
                }
            }
        }
        output.push(names.into_boxed_slice());
    }
    Ok(output)
}

fn split_collection_selection(value: &str) -> Option<(&str, &str)> {
    let open = value.find('[')?;
    value
        .ends_with(']')
        .then(|| (&value[..open], &value[open + 1..value.len() - 1]))
}
