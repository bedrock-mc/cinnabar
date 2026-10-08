use serde_json::{Value, json};

use crate::{mesh_arena, model::Arena, terrain::FLOATS_PER_VERTEX};

#[path = "browser_actor/animation/pose_cache.rs"]
mod pose_cache;

fn arena(blocks: Value) -> Value {
    json!({
        "id": "proof", "name": "Proof arena",
        "palette": [{"name": "minecraft:air", "states": {}},
                    {"name": "minecraft:stone", "states": {}}],
        "blocks": blocks,
        "bounds": [-32, -32, -32, 32, 32, 32]
    })
}

fn vertices(scene: Value) -> Vec<f32> {
    mesh_arena(&scene.to_string()).expect("valid arena")
}

#[test]
fn cube_geometry_retains_world_coordinates_and_outward_normals() {
    let mesh = vertices(arena(json!([[-17, -2, 20, 1]])));
    assert_eq!(mesh.len(), 6 * 6 * FLOATS_PER_VERTEX);
    for vertex in mesh.chunks_exact(FLOATS_PER_VERTEX) {
        assert!((-17.0..=-16.0).contains(&vertex[0]));
        assert!((-2.0..=-1.0).contains(&vertex[1]));
        assert!((20.0..=21.0).contains(&vertex[2]));
        assert_eq!(
            vertex[3..6].iter().map(|value| value.abs()).sum::<f32>(),
            1.0
        );
        assert!(vertex[6..9].iter().all(|color| (0.0..=1.0).contains(color)));
    }
    for triangle in mesh.chunks_exact(3 * FLOATS_PER_VERTEX) {
        let a = &triangle[..FLOATS_PER_VERTEX];
        let b = &triangle[FLOATS_PER_VERTEX..2 * FLOATS_PER_VERTEX];
        let c = &triangle[2 * FLOATS_PER_VERTEX..];
        let ab: [f32; 3] = std::array::from_fn(|axis| b[axis] - a[axis]);
        let ac: [f32; 3] = std::array::from_fn(|axis| c[axis] - a[axis]);
        let cross = [
            ab[1] * ac[2] - ab[2] * ac[1],
            ab[2] * ac[0] - ab[0] * ac[2],
            ab[0] * ac[1] - ab[1] * ac[0],
        ];
        assert!((0..3).map(|axis| cross[axis] * a[axis + 3]).sum::<f32>() > 0.0);
    }
}

#[test]
fn adjacent_blocks_are_greedily_merged_and_hidden_faces_culled() {
    let mesh = vertices(arena(json!([[1, 1, 1, 1], [2, 1, 1, 1]])));
    assert_eq!(mesh.len(), 6 * 6 * FLOATS_PER_VERTEX);
    assert!(
        mesh.chunks_exact(FLOATS_PER_VERTEX)
            .any(|vertex| vertex[0] == 3.0)
    );
}

#[test]
fn face_culling_crosses_positive_and_negative_subchunk_boundaries() {
    for edge in [0, 16] {
        let mesh = vertices(arena(json!([[edge - 1, 1, 1, 1], [edge, 1, 1, 1]])));
        assert!(
            !mesh
                .chunks_exact(FLOATS_PER_VERTEX)
                .any(|vertex| { vertex[0] == edge as f32 && vertex[3].abs() == 1.0 })
        );
    }
}

#[test]
fn final_duplicate_block_update_can_remove_a_block() {
    assert!(vertices(arena(json!([[1, 1, 1, 1], [1, 1, 1, 0]]))).is_empty());
    assert!(vertices(arena(json!([]))).is_empty());
}

#[test]
fn barrier_ceiling_neither_draws_nor_culls_the_stone_below() {
    let stone = arena(json!([[1, 1, 1, 1]]));
    let expected = vertices(stone.clone());
    let mut scene = stone;
    scene["palette"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name": "minecraft:barrier"}));
    for x in 0..3 {
        for z in 0..3 {
            scene["blocks"]
                .as_array_mut()
                .unwrap()
                .push(json!([x, 2, z, 2]));
        }
    }
    assert_eq!(vertices(scene), expected);
    assert!(
        expected
            .chunks_exact(FLOATS_PER_VERTEX)
            .any(|vertex| vertex[1] == 2.0 && vertex[4] == 1.0)
    );
}

#[test]
fn invisible_block_families_produce_no_geometry() {
    for name in [
        "minecraft:barrier",
        "minecraft:structure_void",
        "minecraft:invisible_bedrock",
        "minecraft:moving_block",
        "minecraft:light_block_0",
        "minecraft:light_block_15",
    ] {
        let mut scene = arena(json!([[1, 1, 1, 1]]));
        scene["palette"][1]["name"] = json!(name);
        assert!(vertices(scene).is_empty(), "{name}");
    }
}

#[test]
fn invisible_duplicate_updates_preserve_the_last_palette_value() {
    let mut scene = arena(json!([[1, 1, 1, 1], [1, 1, 1, 2]]));
    scene["palette"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name": "minecraft:barrier"}));
    assert!(vertices(scene.clone()).is_empty());
    scene["blocks"] = json!([[1, 1, 1, 2], [1, 1, 1, 1]]);
    assert_eq!(vertices(scene), vertices(arena(json!([[1, 1, 1, 1]]))));
}

#[test]
fn empty_go_palette_states_may_be_absent_or_null() {
    let mut scene = arena(json!([[1, 1, 1, 1]]));
    scene["palette"][0]["states"] = Value::Null;
    scene["palette"][1]
        .as_object_mut()
        .unwrap()
        .remove("states");
    assert!(!vertices(scene).is_empty());
}

#[test]
fn invalid_snapshot_is_rejected_before_world_allocation() {
    let mut scene = arena(json!([[1, 1, 1, 2]]));
    assert!(
        mesh_arena(&scene.to_string())
            .unwrap_err()
            .contains("palette")
    );
    scene["blocks"] = json!([[33, 1, 1, 1]]);
    assert!(
        mesh_arena(&scene.to_string())
            .unwrap_err()
            .contains("coordinates")
    );
    scene["blocks"] = json!([]);
    scene["bounds"] = json!([-1_000_001, 0, 0, 0, 0, 0]);
    assert!(
        mesh_arena(&scene.to_string())
            .unwrap_err()
            .contains("bounds")
    );
    scene["bounds"] = json!([0, 0, 0, 1024, 1, 1]);
    assert!(
        mesh_arena(&scene.to_string())
            .unwrap_err()
            .contains("extent")
    );
    scene["bounds"] = json!([0, 0, 0, 1, 1, 1]);
    scene["palette"][0]["name"] = json!("minecraft:stone");
    assert!(mesh_arena(&scene.to_string()).unwrap_err().contains("zero"));
    assert!(mesh_arena("{").unwrap_err().contains("JSON"));
}

#[test]
fn admitted_blocks_and_sparse_subchunks_are_bounded() {
    let mut parsed = Arena::parse(&arena(json!([])).to_string()).unwrap();
    parsed.blocks = vec![[0, 0, 0, 1]; crate::model::MAX_BLOCKS + 1];
    assert!(parsed.validate().unwrap_err().contains("million"));
    let blocks = (0..4097)
        .map(|index| {
            [
                16 * (index % 64),
                16 * ((index / 64) % 64),
                16 * (index / 4096),
                1,
            ]
        })
        .collect::<Vec<_>>();
    let mut scene = arena(json!(blocks));
    scene["bounds"] = json!([0, 0, 0, 1023, 1023, 1023]);
    assert!(
        mesh_arena(&scene.to_string())
            .unwrap_err()
            .contains("subchunk")
    );
}

#[test]
fn dyed_palette_states_produce_distinct_linear_colors() {
    let mut scene = arena(json!([[1, 1, 1, 1]]));
    scene["palette"][1] = json!({"name": "minecraft:wool", "states": {"color": "red"}});
    let red = vertices(scene.clone());
    scene["palette"][1]["states"]["color"] = json!("blue");
    let blue = vertices(scene);
    assert!(red[6] > red[8]);
    assert!(blue[8] > blue[6]);
    assert_eq!(red[..6], blue[..6]);
}
