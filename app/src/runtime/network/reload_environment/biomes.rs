use super::{decode_pack_texture, parse_pack_json, parse_rgb};
use assets::{BlockOverlay, RuntimeAssets, TintMapId, TintSource};
use resource_pack::LayeredPackView;
use serde_json::Value;

/// Applies pack colormaps and client-biome colour components to the worker's block overlay.
pub(in crate::runtime::network) fn apply_biome_overlay(
    view: &LayeredPackView,
    base: &RuntimeAssets,
    overlay: &mut BlockOverlay,
) {
    let mut biomes = base.biome_assets().clone();
    let mut changed = false;
    let map_pixels = (assets::TINT_MAP_SIZE * assets::TINT_MAP_SIZE) as usize;
    for map in TintMapId::ALL {
        let path = format!("textures/colormap/{}.png", map.source_name());
        let Some(texture) = decode_pack_texture(view, &path) else {
            continue;
        };
        if texture.width != assets::TINT_MAP_SIZE || texture.height != assets::TINT_MAP_SIZE {
            continue;
        }
        let target = &mut biomes.tint_maps_rgb8[map as usize * map_pixels * 3..][..map_pixels * 3];
        for (output, rgba) in target
            .chunks_exact_mut(3)
            .zip(texture.rgba8.chunks_exact(4))
        {
            output.copy_from_slice(&rgba[..3]);
        }
        changed = true;
    }
    for (_, bytes) in super::layered_json(view, "biomes/") {
        let Some(root) = parse_pack_json(&bytes) else {
            continue;
        };
        let biome = &root["minecraft:client_biome"];
        let Some(identifier) = biome["description"]["identifier"].as_str() else {
            continue;
        };
        let Some(rule) = biomes
            .rules
            .iter_mut()
            .find(|rule| rule.name.as_ref() == identifier)
        else {
            continue;
        };
        let components = &biome["components"];
        for (component, field, target) in [
            ("minecraft:grass_appearance", "color", &mut rule.grass),
            ("minecraft:foliage_appearance", "color", &mut rule.foliage),
            (
                "minecraft:dry_foliage_color",
                "color",
                &mut rule.dry_foliage,
            ),
            (
                "minecraft:water_appearance",
                "surface_color",
                &mut rule.water,
            ),
        ] {
            if let Some(source) = tint(&components[component][field]) {
                *target = source;
                changed = true;
            }
        }
        if let Some(shaded) = components["minecraft:grass_appearance"]["grass_is_shaded"].as_bool()
        {
            rule.flags = (rule.flags & !assets::BIOME_RULE_FLAG_GRASS_SHADED)
                | if shaded {
                    assets::BIOME_RULE_FLAG_GRASS_SHADED
                } else {
                    0
                };
            changed = true;
        }
    }
    if changed {
        overlay.biomes = Some(biomes);
    }
}

/// Resolves supported named colormaps and direct RGB colours.
fn tint(value: &Value) -> Option<TintSource> {
    value["color_map"]
        .as_str()
        .and_then(TintMapId::from_source_name)
        .map(TintSource::map)
        .or_else(|| parse_rgb(value).map(TintSource::direct))
}
