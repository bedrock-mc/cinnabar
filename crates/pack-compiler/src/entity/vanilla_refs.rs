//! Builds the vanilla entity-reference sidecar from the pinned resource pack.

use assets::{AssetError, VanillaEntityRefs, VanillaGeometryFile};
use serde_json::Value;

use super::*;

const MAX_GEOMETRY_TEXT_BYTES: usize = 8 * 1024 * 1024;

/// Reads the pack's named render controllers, animations, animation controllers and geometry.
pub fn compile_vanilla_entity_refs(root: &Path) -> Result<VanillaEntityRefs, AssetError> {
    let mut refs = VanillaEntityRefs::new();
    for (family, key) in [
        ("render_controllers", "render_controllers"),
        ("animations", "animations"),
        ("animation_controllers", "animation_controllers"),
    ] {
        let mut files = Vec::new();
        collect_optional_family(root, family, &["json"], &mut files)?;
        for (relative, absolute) in files {
            let bytes = read_bounded_source(root, &absolute)?;
            let Ok(value) = parse_semantic_json(&absolute, &bytes) else {
                continue;
            };
            let Some(Value::Object(map)) = value.get(key) else {
                continue;
            };
            let target = match key {
                "render_controllers" => &mut refs.render_controllers,
                "animations" => &mut refs.animations,
                _ => &mut refs.animation_controllers,
            };
            for (name, definition) in map {
                target.insert(name.clone(), definition.clone());
            }
            let _ = relative;
        }
    }
    let mut files = Vec::new();
    collect_optional_family(root, "models/entity", &["json"], &mut files)?;
    for (relative, absolute) in files {
        let bytes = read_bounded_source(root, &absolute)?;
        if bytes.len() > MAX_GEOMETRY_TEXT_BYTES {
            continue;
        }
        let Ok(value) = parse_semantic_json(&absolute, &bytes) else {
            continue;
        };
        let Some(text) = std::str::from_utf8(&bytes)
            .ok()
            .map(|text| text.trim_start_matches('\u{feff}').to_owned())
        else {
            continue;
        };
        let mut identifiers = Vec::new();
        if let Some(Value::Array(entries)) = value.get("minecraft:geometry") {
            for entry in entries {
                if let Some(id) = entry["description"]["identifier"].as_str() {
                    identifiers.push((id.to_owned(), None));
                }
            }
        } else if let Value::Object(map) = &value {
            for key in map.keys().filter(|key| key.starts_with("geometry.")) {
                let (id, parent) = match key.split_once(':') {
                    Some((id, parent)) => (id.to_owned(), Some(parent.to_owned())),
                    None => (key.clone(), None),
                };
                identifiers.push((id, parent));
            }
        }
        if identifiers.is_empty() {
            continue;
        }
        let index = refs.geometry_files.len() as u32;
        refs.geometry_files.push(VanillaGeometryFile {
            path: relative.into_string(),
            text,
        });
        for (id, parent) in identifiers {
            if let Some(parent) = parent {
                refs.geometry_parent.insert(id.clone(), parent);
            }
            refs.geometry_index.insert(id, index);
        }
    }
    Ok(refs)
}
