//! Account-free production terrain-shader captures for every hollow-container state.
use crate::{
    end_rod_snapshot::render_at,
    gpu_snapshot::{self, Gpu},
};
use {assets::NetworkIdMode, bevy::math::Vec3};

/// Loads an explicitly provided carrier without using installed game assets or user state.
fn fixture() -> Option<(assets::RuntimeAssets, Vec<assets::RegistryRecord>)> {
    let Some(path) = std::env::var_os("CINNABAR_HOLLOW_CARRIER") else {
        eprintln!("skipping hollow_container_snapshot: missing CINNABAR_HOLLOW_CARRIER fixture");
        return None;
    };
    let target: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../../../assets/bedrock-target.json")).unwrap();
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let registry = assets::read_registry_for_protocol(
        &std::fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap(),
        target["wire_protocol"].as_u64().unwrap() as u32,
    )
    .unwrap();
    Some((
        assets::RuntimeAssets::decode(&std::fs::read(path).unwrap()).unwrap(),
        registry.into_vec(),
    ))
}

/// Meshes the selected container on a bright support with optional solid neighbours.
fn mesh(
    assets: &assets::RuntimeAssets,
    air: u32,
    stone: u32,
    container: u32,
    neighbours: bool,
) -> meshing::ChunkMesh {
    let key = world::SubChunkKey::new(0, 0, 0, 0);
    let mut store = world::ChunkStore::new();
    store.mark_sub_chunk_loaded(key).unwrap();
    for (x, y, z, id) in [(1, 1, 1, container), (1, 0, 1, stone)] {
        store
            .update_block(key, world::BlockUpdate::new(x, y, z, 0, id), air)
            .unwrap();
    }
    if neighbours {
        for (x, z) in [(0, 1), (2, 1), (1, 0), (1, 2)] {
            store
                .update_block(key, world::BlockUpdate::new(x, 1, z, 0, stone), air)
                .unwrap();
        }
    }
    meshing::mesh_sub_chunk(
        &meshing::BlockClassifier::new(air),
        assets,
        NetworkIdMode::Sequential,
        &meshing::Neighbourhood::empty(),
        &store.sub_chunk(key).unwrap(),
    )
}

#[test]
fn hollow_container_states_render_open_rims_closed_interiors_and_contents() {
    let Some((assets, records)) = fixture() else {
        return;
    };
    let Some(gpu) = Gpu::for_fixture("hollow-container terrain snapshot") else {
        return;
    };
    eprintln!("hollow-container GPU backend: {:?}", gpu.backend);
    let id = |name: &str| {
        records
            .iter()
            .find(|r| r.name.as_ref() == name)
            .unwrap()
            .sequential_id
    };
    let mut count = 0;
    for record in records.iter().filter(|r| {
        matches!(
            r.name.as_ref(),
            "minecraft:composter" | "minecraft:cauldron"
        )
    }) {
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let label = if record.name.as_ref() == "minecraft:composter" {
            format!("composter-level-{}", state["composter_fill_level"]["value"])
        } else {
            format!(
                "cauldron-{}-level-{}",
                state["cauldron_liquid"]["value"].as_str().unwrap(),
                state["fill_level"]["value"]
            )
        };
        for neighbours in [false, true] {
            let mesh = mesh(
                &assets,
                id("minecraft:air"),
                id("minecraft:stone"),
                record.sequential_id,
                neighbours,
            );
            let total = mesh.model_draw_refs().len() + mesh.transparent_model_draw_refs().len();
            assert!(total > 0, "fixture must admit {label}");
            for (view, offset) in [
                ("south", Vec3::new(1.9, 2.7, 2.5)),
                ("north", Vec3::new(-1.9, 2.7, -2.5)),
                ("west", Vec3::new(-2.5, 2.7, 1.9)),
                ("east", Vec3::new(2.5, 2.7, -1.9)),
                ("above", Vec3::new(0.01, 4.0, 0.01)),
                ("below", Vec3::new(1.9, -2.7, 2.5)),
            ] {
                let pixels = render_at(
                    &gpu,
                    &assets,
                    &mesh,
                    Vec3::splat(1.5) + offset,
                    Vec3::splat(1.5),
                    true,
                );
                gpu_snapshot::save(
                    &format!(
                        "{label}-{view}{}",
                        if neighbours { "-neighbours" } else { "" }
                    ),
                    &pixels,
                );
                count += 1;
            }
            if neighbours {
                let parts = assets::model_template_parts(
                    assets.model_templates(),
                    assets
                        .resolve(NetworkIdMode::Sequential, record.sequential_id)
                        .model_template()
                        .unwrap(),
                )
                .unwrap();
                let expected: usize = parts.iter().map(|p| p.quad_count as usize).sum();
                assert_eq!(
                    total, expected,
                    "neighbours cannot cull inward walls or fill surfaces for {label}"
                );
            }
        }
    }
    assert_eq!(count, 360);
}
