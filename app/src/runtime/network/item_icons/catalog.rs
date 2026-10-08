//! Layered item atlas variants and pack-local item icon declarations.

use std::collections::HashMap;
use std::sync::Arc;

use resource_pack::LayeredPackView;

use super::super::resource_packs::{MAX_CATALOG_ENTRIES, parse_pack_json};

/// Retains array positions so a missing image never renumbers later metadata variants.
pub(super) fn paths(view: &LayeredPackView) -> HashMap<String, Vec<String>> {
    let mut paths = HashMap::new();
    for layer in view.read_layers("textures/item_texture.json") {
        let Some(root) = parse_pack_json(&layer) else {
            continue;
        };
        let Some(data) = root["texture_data"].as_object() else {
            continue;
        };
        for (key, entry) in data {
            if paths.len() >= MAX_CATALOG_ENTRIES && !paths.contains_key(key) {
                continue;
            }
            let textures = &entry["textures"];
            let path = |value: &serde_json::Value| {
                value
                    .as_str()
                    .or_else(|| value["path"].as_str())
                    .map(str::to_owned)
            };
            let variants = if let Some(path) = path(textures) {
                Some(vec![path])
            } else {
                textures
                    .as_array()
                    .filter(|values| !values.is_empty() && values.len() <= MAX_CATALOG_ENTRIES)
                    .map(|values| {
                        values
                            .iter()
                            .map(|value| path(value).unwrap_or_default())
                            .collect()
                    })
            };
            if let Some(variants) = variants {
                paths.insert(key.clone(), variants);
            }
        }
    }
    paths
}

/// Pack item declarations override the registry's default icon key without changing identity.
pub(super) fn icon_keys(
    view: &LayeredPackView,
    registry: &[(Arc<str>, Arc<str>)],
) -> Vec<(Arc<str>, Arc<str>)> {
    let mut keys: std::collections::BTreeMap<Arc<str>, Arc<str>> =
        registry.iter().cloned().collect();
    let paths = view
        .list("items/")
        .into_iter()
        .filter(|path| path.starts_with("items/") && path.ends_with(".json"))
        .collect::<Vec<_>>();
    for layer in view.layers() {
        for path in &paths {
            let Ok(Some(bytes)) = layer.read_file(path) else {
                continue;
            };
            let Some(root) = parse_pack_json(&bytes) else {
                continue;
            };
            let item = &root["minecraft:item"];
            let Some(identifier) = item["description"]["identifier"].as_str() else {
                continue;
            };
            let icon = &item["components"]["minecraft:icon"];
            let key = icon
                .as_str()
                .or_else(|| icon["textures"]["default"].as_str())
                .or_else(|| icon["texture"].as_str());
            if let Some(key) = key.filter(|key| !key.is_empty() && key.len() <= 256)
                && (keys.len() < MAX_CATALOG_ENTRIES || keys.contains_key(identifier))
            {
                keys.insert(Arc::from(identifier), Arc::from(key));
            }
        }
    }
    keys.into_iter().collect()
}
