//! Literal terrain atlas multipliers, inherited until a higher catalog replaces the key.

use std::collections::HashMap;

use assets::MaterialKeys;
use resource_pack::LayeredPackView;
use serde_json::Value;

use super::{MAX_CATALOG_ENTRIES, parse_pack_json};

pub(super) fn catalog_tints(
    view: &LayeredPackView,
    base: Option<&MaterialKeys>,
) -> HashMap<String, [u8; 3]> {
    let mut tints = base
        .into_iter()
        .flat_map(MaterialKeys::fixed_tints)
        .map(|(key, tint)| (key.to_owned(), tint))
        .collect();
    for layer in view.read_layers("textures/terrain_texture.json") {
        merge(&mut tints, &layer);
    }
    tints
}

fn merge(tints: &mut HashMap<String, [u8; 3]>, bytes: &[u8]) {
    let Some(Value::Object(entries)) =
        parse_pack_json(bytes).map(|mut root| root["texture_data"].take())
    else {
        return;
    };
    for (key, entry) in entries.into_iter().take(MAX_CATALOG_ENTRIES) {
        match first_tint(&entry["textures"]) {
            Some(Some(tint)) => {
                tints.insert(key, tint);
            }
            Some(None) => {
                tints.remove(&key);
            }
            None => {}
        }
    }
}

fn first_tint(value: &Value) -> Option<Option<[u8; 3]>> {
    match value {
        Value::String(path) if !path.trim().is_empty() => Some(None),
        Value::Object(entry)
            if entry
                .get("path")?
                .as_str()
                .is_some_and(|path| !path.trim().is_empty()) =>
        {
            match entry.get("tint_color") {
                None => Some(None),
                Some(tint) => Some(Some(pack_compiler::parse_atlas_tint(tint.as_str()?)?)),
            }
        }
        Value::Array(entries) => first_tint(entries.first()?),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raster_only_layers_inherit_tint_and_catalog_entries_replace_it() {
        let mut tints = HashMap::from([("pad".to_owned(), [32, 128, 48])]);
        merge(
            &mut tints,
            br#"{"texture_data":{"stone":{"textures":"textures/stone"}}}"#,
        );
        assert_eq!(tints["pad"], [32, 128, 48]);
        merge(&mut tints, br##"{"texture_data":{"pad":{"textures":{"path":"textures/new","tint_color":"#123456"}}}}"##);
        assert_eq!(tints["pad"], [18, 52, 86]);
        merge(&mut tints, br#"{"texture_data":{"pad":{"textures":null}}}"#);
        assert_eq!(tints["pad"], [18, 52, 86]);
        merge(
            &mut tints,
            br#"{"texture_data":{"pad":{"textures":"textures/precoloured"}}}"#,
        );
        assert!(!tints.contains_key("pad"));
    }
}
