use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use assets::{
    AssetError, EntityAssetKind, EntityAssetSource, EntityAssetSymbol, EntityGeometry,
    MAX_MOLANG_COLLECTION_ITEMS, validate_entity_geometry_inheritance,
};
use serde_json::{Map, Value};

use super::selection::{Selector, Step, condition_text};
use super::{
    super::{SourcePayloads, invalid},
    clip::{read_json, required_object},
};
use crate::entity::molang::MolangCompiler;

pub(super) struct EntityEnvironment {
    pub entity_symbol: u32,
    pub geometry: Option<u32>,
    pub geometry_aliases: BTreeMap<Box<str>, Box<str>>,
    /// Every geometry the aliases name, which render controllers may draw.
    pub alias_geometries: Vec<u32>,
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
    for (entity_symbol, entity) in symbols.iter().enumerate().filter(|(_, symbol)| {
        matches!(
            symbol.kind,
            EntityAssetKind::Entity | EntityAssetKind::Attachable
        )
    }) {
        let source = &sources[entity.source_index as usize];
        let value = read_json(root, payloads, source)?;
        let description = super::roots::description(&value)
            .ok_or_else(|| invalid("client entity description is absent"))?;
        let geometry_aliases: BTreeMap<Box<str>, Box<str>> = description
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
        let alias_geometries = geometry_aliases
            .values()
            .filter_map(|identifier| geometry_indices.get(identifier.as_ref()).copied().flatten())
            .collect();
        environments.push(EntityEnvironment {
            entity_symbol: entity_symbol as u32,
            geometry,
            alias_geometries,
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
                let Some(expression) = definition.get("geometry").and_then(Value::as_str) else {
                    continue;
                };
                // A plain alias names the entity's default geometry; nothing to select.
                if !expression.contains('?') && !expression.contains('[') {
                    continue;
                }
                let key = (controller_name.as_str().into(), environment.entity_symbol);
                let arrays: BTreeMap<String, Vec<String>> = definition
                    .get("arrays")
                    .and_then(|arrays| arrays.get("geometries"))
                    .and_then(Value::as_object)
                    .into_iter()
                    .flatten()
                    .map(|(name, members)| {
                        (
                            name.to_ascii_lowercase(),
                            members
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(|member| member.as_str().map(str::to_owned))
                                .collect(),
                        )
                    })
                    .collect();
                if arrays
                    .values()
                    .any(|members| members.len() > MAX_MOLANG_COLLECTION_ITEMS)
                {
                    // Too many members to select between: keep the default geometry.
                    selections.insert(key, GeometrySelection::Unsupported);
                    continue;
                }
                let resolve = |alias: &str| {
                    let identifier = environment
                        .geometry_aliases
                        .iter()
                        .find(|(name, _)| name.eq_ignore_ascii_case(alias))
                        .map(|(_, identifier)| identifier.as_ref())?;
                    Some(geometry_indices.get(identifier).copied().flatten())
                };
                let selector = Selector {
                    prefix: "geometry.",
                    arrays,
                    resolve: &resolve,
                };
                let mut transaction = molang.clone();
                let compiled = selector.leaves(expression).and_then(|leaves| {
                    leaves
                        .into_iter()
                        .map(|(path, geometry)| {
                            // Rig candidate conditions must end in a boolean op; a lone
                            // element test already does.
                            let text = condition_text(&path)?;
                            let text = if matches!(path.as_slice(), [Step::Element(..)]) {
                                text
                            } else {
                                format!("({text}) != 0")
                            };
                            let condition = transaction.compile(&text).ok()?;
                            Some(SelectableGeometry {
                                geometry: geometry?,
                                condition,
                            })
                        })
                        .collect::<Option<Vec<_>>>()
                });
                match compiled {
                    Some(candidates) if !candidates.is_empty() => {
                        *molang = transaction;
                        selections.insert(
                            key,
                            GeometrySelection::Supported(candidates.into_boxed_slice()),
                        );
                    }
                    _ => {
                        selections.insert(key, GeometrySelection::Unsupported);
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
        // Vanilla needs no `default` alias (tropical fish, variant-picked pack models): the
        // render controller selects among the aliases, so the first stands in as the rest model.
        Value::Object(values) => values
            .get("default")
            .or_else(|| values.values().next())
            .and_then(Value::as_str),
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
