use std::{collections::BTreeMap, path::Path};

use serde_json::Value;

use super::{read_json, safe_relative};

fn first_path(value: &Value) -> Option<&str> {
    value
        .as_str()
        .or_else(|| value.get("path").and_then(Value::as_str))
        .or_else(|| value.as_array()?.first().and_then(first_path))
}

pub(in crate::pbr) fn read(root: &Path) -> Result<BTreeMap<String, String>, String> {
    let path = root.join("textures/terrain_texture.json");
    if !path.is_file() {
        return Ok(BTreeMap::new());
    }
    let json = read_json(&path)?;
    let Some(data) = json.get("texture_data") else {
        return Ok(BTreeMap::new());
    };
    let data = data
        .as_object()
        .ok_or_else(|| format!("{} has invalid texture_data", path.display()))?;
    if data.len() > 8192 {
        return Err("authored terrain index exceeds texture key budget".to_owned());
    }
    let mut result = BTreeMap::new();
    for (key, entry) in data {
        let Some(value) = entry.get("textures").and_then(first_path) else {
            continue;
        };
        let relative = safe_relative(value)
            .ok_or_else(|| format!("terrain texture {key} escapes its defining pack"))?;
        result.insert(key.clone(), relative.to_string_lossy().replace('\\', "/"));
    }
    Ok(result)
}
