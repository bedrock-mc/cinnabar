//! Resolves geometry winners and legacy parent chains before block projection.
use super::{Geometry, MAX_BONES, MAX_CUBES, number, parse_bones};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// Maximum definitions traversed in one geometry inheritance chain.
const MAX_PARENT_DEPTH: usize = 256;

#[derive(Clone)]
struct Definition {
    parent: Option<String>,
    size: [Option<f32>; 2],
    bones: Vec<Value>,
}

/// Precedence-ordered geometry definitions supplied by admitted pack readers.
#[derive(Default)]
pub struct BlockGeometryCatalog {
    definitions: HashMap<String, Definition>,
}

impl BlockGeometryCatalog {
    /// Retains each winning definition, including parents that no block names directly.
    pub fn insert(&mut self, root: &Value) {
        if let Some(entries) = root["minecraft:geometry"].as_array() {
            for entry in entries {
                let description = &entry["description"];
                let Some(identifier) = description["identifier"].as_str() else {
                    continue;
                };
                self.definitions.insert(
                    identifier.into(),
                    Definition {
                        parent: None,
                        size: [
                            number(&description["texture_width"]),
                            number(&description["texture_height"]),
                        ],
                        bones: entry["bones"].as_array().cloned().unwrap_or_default(),
                    },
                );
            }
        } else if let Some(entries) = root.as_object() {
            for (key, entry) in entries {
                if !key.starts_with("geometry.") {
                    continue;
                }
                let Some((identifier, parent)) = crate::geometry_key::legacy_key(key) else {
                    continue;
                };
                self.definitions.insert(
                    identifier.into(),
                    Definition {
                        parent: parent.map(str::to_owned),
                        size: [
                            number(&entry["texturewidth"]),
                            number(&entry["textureheight"]),
                        ],
                        bones: entry["bones"].as_array().cloned().unwrap_or_default(),
                    },
                );
            }
        }
    }

    /// Adds lower-priority definitions without replacing models already admitted.
    pub fn insert_fallback(&mut self, root: &Value) {
        let mut fallback = Self::default();
        fallback.insert(root);
        for (identifier, definition) in fallback.definitions {
            self.definitions.entry(identifier).or_insert(definition);
        }
    }

    /// Projects requested models after inheritance; missing parents and cycles are omitted.
    pub fn compile(&self, wanted: &HashSet<&str>) -> HashMap<String, Geometry> {
        wanted
            .iter()
            .filter_map(|identifier| {
                let resolved = self.resolve(identifier)?;
                Some((
                    (*identifier).into(),
                    parse_bones(
                        &Value::Array(resolved.bones),
                        resolved.size.map(|size| size.unwrap_or(16.0)),
                    ),
                ))
            })
            .collect()
    }

    /// Resolves an iterative bounded parent chain before allocating merged bone cubes.
    fn resolve(&self, identifier: &str) -> Option<Definition> {
        let chain = parent_chain(identifier, |identifier| self.definitions.get(identifier))?;
        let mut result = Definition {
            parent: None,
            size: [None; 2],
            bones: Vec::new(),
        };
        for definition in chain.into_iter().rev() {
            for (size, override_size) in result.size.iter_mut().zip(definition.size) {
                if let Some(value) = override_size {
                    *size = Some(value);
                }
            }
            for child in &definition.bones {
                let name = child["name"].as_str()?;
                if let Some(base) = result
                    .bones
                    .iter_mut()
                    .find(|bone| bone["name"].as_str() == Some(name))
                {
                    let inherited = if child["reset"].as_bool() == Some(true) {
                        &[][..]
                    } else {
                        base["cubes"].as_array().map_or(&[][..], Vec::as_slice)
                    };
                    let authored = child["cubes"].as_array().map_or(&[][..], Vec::as_slice);
                    if inherited.len().checked_add(authored.len())? > MAX_CUBES {
                        return None;
                    }
                    let cubes = inherited.iter().chain(authored).cloned().collect();
                    let fields = base.as_object_mut()?;
                    fields.extend(
                        child
                            .as_object()?
                            .iter()
                            .filter(|(key, _)| key.as_str() != "cubes")
                            .map(|(key, value)| (key.clone(), value.clone())),
                    );
                    fields.insert("cubes".into(), Value::Array(cubes));
                } else {
                    if result.bones.len() >= MAX_BONES
                        || child["cubes"]
                            .as_array()
                            .is_some_and(|cubes| cubes.len() > MAX_CUBES)
                    {
                        return None;
                    }
                    result.bones.push(child.clone());
                }
            }
            if result
                .bones
                .iter()
                .filter_map(|bone| bone["cubes"].as_array())
                .map(Vec::len)
                .sum::<usize>()
                > MAX_CUBES
            {
                return None;
            }
        }
        let names: HashSet<_> = result
            .bones
            .iter()
            .filter_map(|bone| bone["name"].as_str())
            .collect();
        for bone in &result.bones {
            let mut current = bone["parent"].as_str();
            let mut visited = HashSet::new();
            while let Some(parent) = current {
                if !names.contains(parent) || !visited.insert(parent) {
                    return None;
                }
                current = result
                    .bones
                    .iter()
                    .find(|bone| bone["name"].as_str() == Some(parent))?["parent"]
                    .as_str();
            }
        }
        Some(result)
    }
}

/// Collects each parent once, rejecting cycles and chains beyond the depth limit.
fn parent_chain<'a>(
    identifier: &str,
    mut lookup: impl FnMut(&str) -> Option<&'a Definition>,
) -> Option<Vec<&'a Definition>> {
    let mut chain = Vec::new();
    let mut visited = HashSet::new();
    let mut current = Some(identifier);
    while let Some(identifier) = current {
        if chain.len() >= MAX_PARENT_DEPTH || !visited.insert(identifier) {
            return None;
        }
        let definition = lookup(identifier)?;
        chain.push(definition);
        current = definition.parent.as_deref();
    }
    Some(chain)
}

/// Compiles the models in one normalized JSON document without requiring a pack reader.
pub fn parse_geometry_file(bytes: &[u8]) -> Vec<(String, Geometry)> {
    let Ok(root) = serde_json::from_slice::<Value>(bytes) else {
        return Vec::new();
    };
    let mut catalog = BlockGeometryCatalog::default();
    catalog.insert(&root);
    let wanted = catalog.definitions.keys().map(String::as_str).collect();
    catalog.compile(&wanted).into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Produces a cube with a single top face and an independently observable position.
    fn cube(y: i32) -> Value {
        json!({"origin":[-8,y,-8],"size":[16,1,16],"uv":{"up":{"uv":[0,0],"uv_size":[16,16],"material_instance":"top"}}})
    }

    #[test]
    fn cyclic_parent_lookup_work_does_not_grow_with_unrelated_definitions() {
        let mut catalog = BlockGeometryCatalog::default();
        catalog.insert(&json!({
            "geometry.self:geometry.self":{"bones":[]},
            "geometry.a:geometry.b":{"bones":[]},
            "geometry.b:geometry.a":{"bones":[]}
        }));
        for index in 0..50_000 {
            catalog.insert(&json!({format!("geometry.unrelated{index}"):{"bones":[]}}));
        }
        for (identifier, expected_lookups) in [("geometry.self", 1), ("geometry.a", 2)] {
            let mut lookups = 0;
            let chain = parent_chain(identifier, |identifier| {
                lookups += 1;
                catalog.definitions.get(identifier)
            });
            assert!(chain.is_none());
            assert_eq!(lookups, expected_lookups);
        }
    }

    #[test]
    fn inheritance_depth_is_bounded_without_rejecting_the_limit() {
        let mut catalog = BlockGeometryCatalog::default();
        catalog.insert(&json!({"geometry.base":{"bones":[{"name":"root","cubes":[cube(8)]}]}}));
        let mut parent = "geometry.base".to_owned();
        for depth in 1..=MAX_PARENT_DEPTH {
            let identifier = format!("geometry.depth{depth}");
            catalog.insert(&json!({format!("{identifier}:{parent}"):{"bones":[]}}));
            let compiled = catalog.compile(&HashSet::from([identifier.as_str()]));
            if depth < MAX_PARENT_DEPTH {
                assert_eq!(compiled[&identifier].cubes[0].min[1], 8.0);
            } else {
                assert!(compiled.is_empty());
            }
            parent = identifier;
        }
    }

    #[test]
    fn fallback_definitions_preserve_winners_and_document_order() {
        let mut catalog = BlockGeometryCatalog::default();
        catalog.insert(&json!({"geometry.model":{"bones":[{"name":"root","cubes":[cube(8)]}]}}));
        catalog.insert_fallback(&json!({"minecraft:geometry":[
            {"description":{"identifier":"geometry.model"},"bones":[{"name":"root","cubes":[cube(0)]}]},
            {"description":{"identifier":"geometry.extra"},"bones":[{"name":"root","cubes":[cube(2)]}]},
            {"description":{"identifier":"geometry.extra"},"bones":[{"name":"root","cubes":[cube(4)]}]}
        ]}));
        let compiled = catalog.compile(&HashSet::from(["geometry.model", "geometry.extra"]));
        assert_eq!(compiled["geometry.model"].cubes[0].min[1], 8.0);
        assert_eq!(compiled["geometry.extra"].cubes[0].min[1], 4.0);
    }

    #[test]
    fn inherited_models_resolve_unrequested_parents_and_append_or_reset_bones() {
        let mut catalog = BlockGeometryCatalog::default();
        catalog.insert(&json!({"format_version":"1.8.0","geometry.base":{
            "texturewidth":32,"textureheight":64,
            "bones":[{"name":"root","pivot":[0,0,0],"cubes":[cube(0)]}]
        }}));
        catalog.insert(&json!({"format_version":"1.8.0","geometry.child:geometry.base":{
            "bones":[{"name":"root","cubes":[cube(2)]},{"name":"extra","parent":"root","cubes":[cube(4)]}]
        }}));
        let wanted = HashSet::from(["geometry.child"]);
        let compiled = catalog.compile(&wanted);
        let child = &compiled["geometry.child"];
        assert_eq!(child.texture_size, [32.0, 64.0]);
        assert_eq!(
            child
                .cubes
                .iter()
                .map(|cube| cube.min[1])
                .collect::<Vec<_>>(),
            [0.0, 2.0, 4.0]
        );
        assert_eq!(child.cubes[0].quads()[0].1, Some("top"));
        catalog.insert(
            &json!({"format_version":"1.8.0","geometry.child:geometry.base":{
                "textureheight":32,"bones":[{"name":"root","reset":true,"cubes":[cube(8)]}]
            }}),
        );
        let replaced = catalog.compile(&wanted);
        assert_eq!(replaced["geometry.child"].cubes.len(), 1);
        assert_eq!(replaced["geometry.child"].cubes[0].min[1], 8.0);
        assert_eq!(replaced["geometry.child"].texture_size, [32.0, 32.0]);
    }

    #[test]
    fn missing_or_cyclic_parents_and_invalid_bone_hierarchies_are_not_drawn() {
        let mut catalog = BlockGeometryCatalog::default();
        catalog.insert(&json!({"format_version":"1.8.0",
            "geometry.a:geometry.b":{"bones":[]},
            "geometry.b:geometry.a":{"bones":[]},
            "geometry.missing:geometry.absent":{"bones":[]},
            "geometry.invalid":{"bones":[{"name":"root","parent":"absent","cubes":[cube(0)]}]}
        }));
        assert!(
            catalog
                .compile(&HashSet::from([
                    "geometry.a",
                    "geometry.b",
                    "geometry.missing",
                    "geometry.invalid"
                ]))
                .is_empty()
        );
    }
}
