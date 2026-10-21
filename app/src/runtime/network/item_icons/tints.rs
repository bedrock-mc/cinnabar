//! Inventory colours derived from carried texture metadata rather than a world biome.

use assets::material_keys::MaterialKeys;
use resource_pack::LayeredPackView;
use serde_json::Value;

const CARRIED_GRASS_TEXTURE: &str = "grass_carried";

/// Resolves carried grass colour with whole-entry server overrides and tracked catalog reads.
pub(in crate::runtime::network) fn carried_grass_tint(
    view: &LayeredPackView,
    base: Option<&MaterialKeys>,
) -> Option<[u8; 3]> {
    let mut colour = base
        .and_then(|keys| {
            keys.terrain_entries()
                .find(|(key, _)| *key == CARRIED_GRASS_TEXTURE)
        })
        .and_then(|(_, entry)| overlay_colour(entry));
    for layer in view.read_layers("textures/terrain_texture.json") {
        let Some(root) = pack_runtime::pack_textures::parse_pack_json(&layer) else {
            continue;
        };
        let entry = &root["texture_data"][CARRIED_GRASS_TEXTURE];
        if pack_runtime::pack_textures::terrain::parse_entry(entry).is_some() {
            colour = overlay_colour(entry);
        }
    }
    colour
}

/// Reads the default carried variant's literal overlay colour without guessing a replacement.
fn overlay_colour(entry: &Value) -> Option<[u8; 3]> {
    let textures = entry.get("textures")?;
    let texture = textures
        .as_array()
        .map_or(Some(textures), |values| values.first())?;
    let colour = texture
        .get("overlay_color")
        .or_else(|| entry.get("overlay_color"))?;
    pack_compile_input::parse_atlas_tint(colour.as_str()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn carried_colour_uses_authored_default_variant_and_rejects_unknowns() {
        assert_eq!(
            overlay_colour(&json!({"textures": [
                {"path":"textures/grass", "overlay_color":"#123456"},
                {"path":"textures/grass", "overlay_color":"#abcdef"}
            ]})),
            Some([0x12, 0x34, 0x56])
        );
        for entry in [
            json!({"textures":"textures/grass"}),
            json!({"textures":[]}),
            json!({"textures":{"path":"textures/grass","overlay_color":"invalid"}}),
        ] {
            assert_eq!(overlay_colour(&entry), None);
        }
    }
}
