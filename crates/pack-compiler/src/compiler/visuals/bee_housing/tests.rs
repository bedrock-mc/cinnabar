use super::*;
use serde_json::json;

#[test]
fn retained_honey_fronts_resolve_the_second_catalog_variant() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("textures")).unwrap();
    let mut terrain = serde_json::Map::new();
    let mut blocks = serde_json::Map::new();
    for family in ["bee_nest", "beehive"] {
        let front = format!("{family}_front");
        terrain.insert(front.clone(), json!({"textures":[format!("textures/blocks/{front}"), format!("textures/blocks/{front}_honey")]}));
        for suffix in ["side", "top", "bottom"] {
            let key = format!("{family}_{suffix}");
            terrain.insert(
                key.clone(),
                json!({"textures":[format!("textures/blocks/{key}")]}),
            );
        }
        blocks.insert(
            family.into(),
            json!({"textures":{
                "west":format!("{family}_side"), "east":format!("{family}_side"),
                "north":format!("{family}_side"), "up":format!("{family}_top"),
                "down":format!("{family}_{}", if family == "beehive" { "top" } else { "bottom" }),
                "south":front,
            }}),
        );
    }
    std::fs::write(root.path().join("blocks.json"), json!(blocks).to_string()).unwrap();
    std::fs::write(
        root.path().join("textures/terrain_texture.json"),
        json!({"texture_data":terrain}).to_string(),
    )
    .unwrap();
    std::fs::write(root.path().join("textures/flipbook_textures.json"), "[]").unwrap();
    let pack = pack_compile_input::read_pack(root.path()).unwrap();
    let descriptors = bee_housing_material_descriptors(&pack).unwrap();
    std::fs::create_dir_all(root.path().join("textures/blocks")).unwrap();
    for (descriptor, _) in &descriptors {
        image::RgbaImage::from_pixel(1, 1, image::Rgba([31, 47, 59, 255]))
            .save(
                root.path()
                    .join(descriptor.path.as_ref())
                    .with_extension("png"),
            )
            .unwrap();
    }
    let ids = descriptors
        .iter()
        .enumerate()
        .map(|(index, (descriptor, _))| (descriptor.clone(), index as u32 + 1))
        .collect::<BTreeMap<_, _>>();
    let materials = vec![Material::unvaried(); descriptors.len() + 1];
    let keys = crate::compiler::pack_bindings::retain(
        MaterialKeys::from_entries(
            descriptors
                .iter()
                .enumerate()
                .map(|(index, (_, key))| (index as u32 + 1, key)),
        ),
        (&pack, root.path()),
        &ids,
        &materials,
        (&[], &[]),
        &[],
        &[],
    )
    .unwrap();
    let keys =
        MaterialKeys::from_json(&keys.to_json(materials.len() as u32), materials.len()).unwrap();
    for (id, source) in keys.material_sources() {
        let honey = source.path.ends_with("_honey");
        assert_eq!(source.variant, u32::from(honey), "material {id}");
        assert_eq!(source.raster.as_ref(), format!("{}.png", source.path));
        let entry = keys
            .terrain_entries()
            .find(|(key, _)| *key == source.key.as_ref())
            .unwrap()
            .1;
        assert_eq!(
            entry["textures"][source.variant as usize].as_str().unwrap(),
            source.path.as_ref()
        );
    }
}
