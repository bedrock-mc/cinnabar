use super::*;
use serde_json::json;

#[test]
fn cake_inner_material_keeps_the_second_west_face_variant() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("textures")).unwrap();
    std::fs::write(
        root.path().join("blocks.json"),
        json!({"cake":{"textures":{
            "west":"cake_west", "east":"cake_side", "north":"cake_side",
            "south":"cake_side", "up":"cake_top", "down":"cake_bottom"
        }}})
        .to_string(),
    )
    .unwrap();
    let mut terrain = serde_json::Map::new();
    for suffix in ["side", "top", "bottom"] {
        let path = format!("textures/blocks/cake_{suffix}");
        terrain.insert(format!("cake_{suffix}"), json!({"textures":[path, path]}));
    }
    terrain.insert(
        "cake_west".into(),
        json!({"textures":["textures/blocks/cake_side", "textures/blocks/cake_inner"]}),
    );
    std::fs::write(
        root.path().join("textures/terrain_texture.json"),
        json!({"texture_data":terrain}).to_string(),
    )
    .unwrap();
    std::fs::write(root.path().join("textures/flipbook_textures.json"), "[]").unwrap();
    let pack = pack_compile_input::read_pack(root.path()).unwrap();
    for (descriptor, key) in cake_material_descriptors(&pack).unwrap() {
        let variants = pack.terrain.get_exact_pair_no_tint(&key).unwrap();
        assert_eq!(
            variants[descriptor.state_variant as usize],
            descriptor.path.as_ref()
        );
        if key.as_ref() == "cake_west" {
            assert_eq!(descriptor.state_variant, 1);
        }
    }
}
