use std::sync::Arc;

use serde_json::json;

use crate::{TerrainAssets, canonical, materials, model::Arena, terrain_runtime};

fn registry() -> Box<[assets::RegistryRecord]> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let target: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("assets/bedrock-target.json")).unwrap())
            .unwrap();
    let bytes =
        std::fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap();
    let protocol = u32::try_from(target["wire_protocol"].as_u64().unwrap()).unwrap();
    assets::read_registry_for_protocol(&bytes, protocol).expect("pinned registry")
}

#[test]
fn registry_states_accept_dragonfly_boolean_bits_without_guessing_other_states() {
    let records = registry();
    let index = canonical::registry_index(&records).expect("unambiguous canonical states");
    let record = records
        .iter()
        .find(|record| {
            let states: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
            states.as_object().unwrap().iter().any(|(key, field)| {
                key.ends_with("_bit") && matches!(field["value"].as_i64(), Some(0 | 1))
            })
        })
        .expect("registry contains bit-valued states");
    let typed: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&record.canonical_state).unwrap();
    let mut states = typed
        .into_iter()
        .map(|(key, field)| {
            let value = if key.ends_with("_bit") {
                json!(field["value"].as_i64().unwrap() != 0)
            } else {
                field["value"].clone()
            };
            (key, value)
        })
        .collect::<serde_json::Map<_, _>>();
    let entry = crate::model::PaletteEntry {
        name: record.name.to_string(),
        states: states.clone(),
    };
    assert_eq!(
        canonical::palette_ids(&index, &[entry]).unwrap(),
        [record.sequential_id]
    );
    states.insert("absent_state".into(), json!(123));
    let entry = crate::model::PaletteEntry {
        name: record.name.to_string(),
        states,
    };
    assert!(canonical::palette_ids(&index, &[entry]).is_err());
}

#[test]
fn registry_resolver_does_not_assume_air_has_runtime_id_zero() {
    let records = registry();
    let index = canonical::registry_index(&records).unwrap();
    let air = records
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:air")
        .unwrap();
    let entry = crate::model::PaletteEntry {
        name: "air".into(),
        states: Default::default(),
    };
    assert_eq!(
        canonical::palette_ids(&index, &[entry]).unwrap(),
        [air.sequential_id]
    );
    assert_ne!(air.sequential_id, 0);
}

#[test]
fn native_mesh_keeps_packed_streams_and_cross_boundary_final_updates() {
    let input = json!({"palette":[{"name":"minecraft:air"},{"name":"minecraft:stone"}],
        "blocks":[[-1,0,0,1],[0,0,0,1],[0,0,0,0]],"bounds":[-1,0,0,0,0,0]});
    let arena = Arena::parse(&input.to_string()).unwrap();
    let assets = diagnostic_assets(&arena);
    let meshes = terrain_runtime::prepare(&arena, &assets).unwrap();
    assert_eq!(meshes.len(), 1);
    let (key, mesh) = &meshes[0];
    assert_eq!(key.x, -1);
    assert_eq!(mesh.quad_count(), 6);
}

fn diagnostic_assets(arena: &Arena) -> TerrainAssets {
    let (runtime, _) = materials::palette_assets(&arena.palette).unwrap();
    let canonical = std::collections::BTreeMap::from([
        (
            serde_json::to_string(&(
                "minecraft:air",
                std::collections::BTreeMap::<String, serde_json::Value>::new(),
            ))
            .unwrap(),
            0,
        ),
        (
            serde_json::to_string(&(
                "minecraft:stone",
                std::collections::BTreeMap::<String, serde_json::Value>::new(),
            ))
            .unwrap(),
            1,
        ),
    ]);
    TerrainAssets {
        runtime: Arc::new(runtime),
        canonical: Arc::new(canonical),
        air: 0,
        #[cfg(target_arch = "wasm32")]
        collision_records: Arc::from([]),
        #[cfg(target_arch = "wasm32")]
        collision_halo: [[0, 0]; 3],
    }
}

#[test]
fn replay_seek_restores_removed_blocks_and_cross_boundary_faces() {
    let arena = Arena::parse(&json!({"palette":[{"name":"minecraft:air"},{"name":"minecraft:stone"}],"blocks":[[-1,0,0,1],[0,0,0,1]],"bounds":[-1,0,0,0,0,0]}).to_string()).unwrap();
    let assets = diagnostic_assets(&arena);
    let mut scene = terrain_runtime::TerrainScene::new(&arena, &assets).unwrap();
    let original = scene
        .initial(&assets)
        .unwrap()
        .iter()
        .map(|(_, mesh)| mesh.quad_count())
        .sum::<usize>();
    let removed = [crate::browser_model::SceneBlock {
        position: [0, 0, 0],
        name: "minecraft:air".into(),
        states: Default::default(),
    }];
    let changed = scene.apply(&removed, &assets).unwrap();
    assert!(
        changed
            .iter()
            .any(|(key, mesh)| key.x == 0 && mesh.is_empty())
    );
    assert!(
        changed
            .iter()
            .any(|(key, mesh)| key.x == -1 && mesh.quad_count() == 6)
    );
    assert!(scene.apply(&removed, &assets).unwrap().is_empty());
    let restored = scene.apply(&[], &assets).unwrap();
    assert_eq!(
        restored
            .iter()
            .map(|(_, mesh)| mesh.quad_count())
            .sum::<usize>(),
        original
    );
    assert!(scene.apply(&[], &assets).unwrap().is_empty());
}
