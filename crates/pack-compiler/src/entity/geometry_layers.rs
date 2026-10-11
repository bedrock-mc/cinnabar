//! Select geometry definitions at their identifier boundary before compilation.

use serde_json::Value;
use std::collections::BTreeMap;

/// Select the last document defining each geometry identifier in low-to-high source order.
/// Keep each selected definition in its original document, including legacy inheritance keys.
/// Retained layers keep their authored paths; the compiler assigns distinct source keys.
pub fn select_entity_geometry(files: Vec<(Box<str>, Vec<u8>)>) -> Vec<(Box<str>, Vec<u8>)> {
    let mut candidates = Vec::new();
    let mut winners = BTreeMap::new();
    for (path, bytes) in files {
        let Ok(mut root) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        let Some(object) = root.as_object_mut() else {
            continue;
        };
        let index = candidates.len();
        if let Some(entries) = object.get("minecraft:geometry").and_then(Value::as_array) {
            for entry in entries {
                if let Some(id) = entry["description"]["identifier"].as_str() {
                    winners.insert(id.to_owned(), index);
                }
            }
        } else {
            for key in object.keys().filter(|key| key.starts_with("geometry.")) {
                winners.insert(key.split(':').next().unwrap_or(key).to_owned(), index);
            }
        }
        candidates.push((path, root));
    }
    let mut selected = Vec::new();
    for (index, (path, mut root)) in candidates.into_iter().enumerate() {
        let object = root.as_object_mut().expect("object candidate");
        let kept = if let Some(entries) = object
            .get_mut("minecraft:geometry")
            .and_then(Value::as_array_mut)
        {
            entries.retain(|entry| {
                entry["description"]["identifier"]
                    .as_str()
                    .is_some_and(|id| winners.get(id) == Some(&index))
            });
            !entries.is_empty()
        } else {
            object.retain(|key, _| {
                !key.starts_with("geometry.")
                    || winners.get(key.split(':').next().unwrap_or(key)) == Some(&index)
            });
            object.keys().any(|key| key.starts_with("geometry."))
        };
        if kept && let Ok(bytes) = serde_json::to_vec(&root) {
            selected.push((path, bytes));
        }
    }
    selected
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Build a modern document with a named bone in each geometry.
    fn document(entries: &[(&str, &str)]) -> Vec<u8> {
        serde_json::to_vec(&json!({"format_version":"1.12.0", "minecraft:geometry":
            entries.iter().map(|(id, bone)| json!({"description":{"identifier":id},
                "bones":[{"name":bone}]})).collect::<Vec<_>>()
        }))
        .unwrap()
    }

    /// Compile selected documents with one entity rig for each expected geometry.
    fn assert_rigs(files: Vec<(Box<str>, Vec<u8>)>, expected: &[(&str, &str)]) {
        let mut files = select_entity_geometry(files);
        for (index, (geometry, _)) in expected.iter().enumerate() {
            files.push((
                format!("entity/{index}.json").into(),
                serde_json::to_vec(&json!({
                    "format_version":"1.10.0", "minecraft:client_entity":{"description":{
                        "identifier":format!("fixture:{index}"), "geometry":{"default":geometry},
                        "render_controllers":["controller.render.fixture"]
                    }}
                }))
                .unwrap(),
            ));
        }
        files.push((
            "render_controllers/fixture.json".into(),
            serde_json::to_vec(&json!({
                "format_version":"1.8.0", "render_controllers":{
                    "controller.render.fixture":{"geometry":"Geometry.default"}
                }
            }))
            .unwrap(),
        ));
        let compiled = super::super::compile_entity_pack(files.clone())
            .unwrap()
            .unwrap();
        let repeated = super::super::compile_entity_pack(files).unwrap().unwrap();
        assert_eq!(
            compiled.assets, repeated.assets,
            "source identities must be stable"
        );
        assert_eq!(compiled.skipped, super::super::EntityPackSkips::default());
        let runtime = assets::RuntimeEntityAssets::from_compiled(compiled.assets).unwrap();
        assert_eq!(runtime.rig_bindings().len(), expected.len());
        assert_eq!(runtime.geometries().len(), expected.len());
        for (id, bone) in expected {
            let geometry = runtime
                .geometries()
                .iter()
                .find(|geometry| geometry.identifier.as_ref() == *id)
                .unwrap();
            assert_eq!(geometry.bones[0].name.as_ref(), *bone);
        }
    }

    #[test]
    fn retained_layers_cannot_hide_an_authored_source() {
        assert_rigs(
            vec![
                (
                    "models/entity/shared.json".into(),
                    document(&[("geometry.a", "old"), ("geometry.b", "retained")]),
                ),
                (
                    "models/entity/shared.json".into(),
                    document(&[("geometry.a", "replacement")]),
                ),
                (
                    "models/entity/_layers/0/entity/shared.json".into(),
                    document(&[("geometry.c", "authored")]),
                ),
                (
                    format!(
                        "{}0.json",
                        super::super::pack::RETAINED_GEOMETRY_SOURCE_PREFIX
                    )
                    .into(),
                    document(&[("geometry.d", "reserved")]),
                ),
            ],
            &[
                ("geometry.a", "replacement"),
                ("geometry.b", "retained"),
                ("geometry.c", "authored"),
                ("geometry.d", "reserved"),
            ],
        );
    }

    #[test]
    fn retained_layers_keep_long_paths_loadable() {
        let prefix = "models/entity/";
        let suffix = ".json";
        let path = format!(
            "{prefix}{}{suffix}",
            "a".repeat(assets::MAX_ENTITY_ASSET_PATH_BYTES - prefix.len() - suffix.len())
        );
        assert_rigs(
            vec![
                (
                    path.clone().into(),
                    document(&[("geometry.a", "old"), ("geometry.b", "retained")]),
                ),
                (path.into(), document(&[("geometry.a", "replacement")])),
            ],
            &[("geometry.a", "replacement"), ("geometry.b", "retained")],
        );
    }

    #[test]
    fn partial_shadowing_keeps_both_rigs_and_the_replacement_bone() {
        for upper_path in ["models/entity/upper.json", "models/entity/lower.json"] {
            assert_rigs(
                vec![
                    (
                        "models/entity/lower.json".into(),
                        document(&[("geometry.a", "old"), ("geometry.b", "retained")]),
                    ),
                    (
                        upper_path.into(),
                        document(&[("geometry.a", "replacement")]),
                    ),
                ],
                &[("geometry.a", "replacement"), ("geometry.b", "retained")],
            );
        }
    }

    #[test]
    fn legacy_child_keeps_its_inheritance_and_unshadowed_sibling() {
        let lower = json!({"format_version":"1.8.0", "geometry.a:geometry.parent":{"bones":[]},
            "geometry.b":{"bones":[]}});
        let upper = json!({"format_version":"1.8.0", "geometry.a:geometry.other":{"bones":[]}});
        let selected = select_entity_geometry(vec![
            (
                "models/lower.json".into(),
                serde_json::to_vec(&lower).unwrap(),
            ),
            (
                "models/upper.json".into(),
                serde_json::to_vec(&upper).unwrap(),
            ),
        ]);
        let lower: Value = serde_json::from_slice(&selected[0].1).unwrap();
        let upper: Value = serde_json::from_slice(&selected[1].1).unwrap();
        assert!(lower.get("geometry.a:geometry.parent").is_none());
        assert!(lower.get("geometry.b").is_some());
        assert!(upper.get("geometry.a:geometry.other").is_some());
    }
}
