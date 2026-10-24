use assets::{BlockEntityRouteKind, facts::*};

/// Decodes independent identity records from the pinned block carrier.
fn records() -> Box<[assets::RegistryRecord]> {
    assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap()
}

#[test]
fn block_entity_facts_cover_every_declared_backing_state_and_no_unrelated_state() {
    let facts = BlockEntityFacts::pinned();
    let records = records();
    for entry in facts.entries() {
        assert!(!entry.backing_blocks.is_empty(), "{}", entry.nbt_id);
        for name in &entry.backing_blocks {
            let mut states = records.iter().filter(|r| r.name.as_ref() == name);
            assert!(states.clone().next().is_some(), "{name}");
            assert!(
                states.all(|r| {
                    let fact = facts.get(r.sequential_id).unwrap();
                    fact.entity.nbt_id == entry.nbt_id && fact.network_hash == r.network_hash
                }),
                "{name}"
            );
        }
    }
    for record in &records {
        if let Some(backing) = facts.get(record.sequential_id) {
            assert_eq!(
                facts.by_block_identifier(&record.name),
                Some(backing.entity)
            );
            assert!(
                backing
                    .entity
                    .backing_blocks
                    .iter()
                    .any(|name| name == record.name.as_ref())
            );
        }
    }
    assert!(facts.get(u32::MAX).is_none());
    assert!(facts.entry("UnknownBlockEntity").is_none());
    assert!(facts.by_block_identifier("example:unknown_block").is_none());
}

#[test]
fn block_entity_facts_separate_sign_mounts_and_spawner_handlers() {
    let facts = BlockEntityFacts::pinned();
    let records = records();
    for (name, id, renderer, mount) in [
        (
            "minecraft:standing_sign",
            "Sign",
            BlockEntityRenderer::Sign,
            BlockEntityMount::Standing,
        ),
        (
            "minecraft:wall_sign",
            "Sign",
            BlockEntityRenderer::Sign,
            BlockEntityMount::Wall,
        ),
        (
            "minecraft:oak_hanging_sign",
            "HangingSign",
            BlockEntityRenderer::Sign,
            BlockEntityMount::Hanging,
        ),
        (
            "minecraft:standing_banner",
            "Banner",
            BlockEntityRenderer::Banner,
            BlockEntityMount::Standing,
        ),
        (
            "minecraft:wall_banner",
            "Banner",
            BlockEntityRenderer::Banner,
            BlockEntityMount::Wall,
        ),
        (
            "minecraft:mob_spawner",
            "MobSpawner",
            BlockEntityRenderer::MobSpawner,
            BlockEntityMount::None,
        ),
        (
            "minecraft:trial_spawner",
            "TrialSpawner",
            BlockEntityRenderer::None,
            BlockEntityMount::None,
        ),
    ] {
        for record in records.iter().filter(|r| r.name.as_ref() == name) {
            let backing = facts.get(record.sequential_id).unwrap();
            assert_eq!(backing.entity.nbt_id, id);
            assert_eq!(backing.entity.renderer, renderer);
            assert_eq!(backing.mount, mount);
        }
    }
    for id in ["BrushableBlock", "TrialSpawner", "Vault"] {
        assert_eq!(
            facts.entry(id).unwrap().route,
            BlockEntityRouteKind::Unsupported
        );
        assert!(facts.entry(id).unwrap().provisional);
    }
}

#[test]
fn chalkboard_block_entity_route_owns_only_its_declared_backing_states() {
    let facts = BlockEntityFacts::pinned();
    let entry = facts
        .entry("ChalkboardBlock")
        .expect("chalkboard model route");
    assert_eq!(entry.renderer, BlockEntityRenderer::Chalkboard);
    for record in records()
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:chalkboard")
    {
        assert_eq!(facts.get(record.sequential_id).unwrap().entity, entry);
    }
    assert_ne!(facts.by_block_identifier("minecraft:stone"), Some(entry));
}

#[test]
fn block_entity_facts_reject_stale_registry_bindings_and_duplicate_states() {
    let bytes = include_bytes!("../../data/block-entities-v2193.json");
    let mut payload: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    payload["registry_sha256"] = "00".repeat(32).into();
    assert!(
        BlockEntityFacts::decode(
            &serde_json::to_vec(&payload).unwrap(),
            assets::pinned_block_registry_bytes()
        )
        .is_err()
    );
    let mut payload: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    let first = payload["states"][0].clone();
    payload["states"].as_array_mut().unwrap().push(first);
    assert!(
        BlockEntityFacts::decode(
            &serde_json::to_vec(&payload).unwrap(),
            assets::pinned_block_registry_bytes()
        )
        .is_err()
    );
    assert!(
        BlockEntityFacts::decode(bytes, include_bytes!("../../data/block-registry-v1001.bin"))
            .is_err()
    );
}

#[test]
fn block_entity_facts_preserve_the_pinned_source_inventory_backings() {
    let source: serde_json::Value =
        serde_json::from_str(include_str!("../../data/block-entities-v1001.json")).unwrap();
    let records = records();
    for entry in source["entries"].as_array().unwrap() {
        let id = entry["nbt_id"]
            .as_str()
            .unwrap_or_else(|| entry["source_key"].as_str().unwrap());
        for block in entry["backing_blocks"].as_array().unwrap() {
            let name = block["name"].as_str().unwrap();
            for record in records.iter().filter(|r| r.name.as_ref() == name) {
                assert_eq!(
                    BlockEntityFacts::pinned()
                        .get(record.sequential_id)
                        .unwrap()
                        .entity
                        .nbt_id,
                    id,
                    "{name}"
                );
            }
        }
    }
}

#[test]
fn block_entity_facts_resolve_both_id_modes_without_world_artwork() {
    use sha2::{Digest, Sha256};
    let hash: [u8; 32] = Sha256::digest(assets::pinned_block_registry_bytes()).into();
    let facts = BlockEntityFacts::pinned();
    for record in records().iter() {
        let Some(expected) = facts.get(record.sequential_id) else {
            continue;
        };
        for (mode, value) in [
            (assets::NetworkIdMode::Sequential, record.sequential_id),
            (assets::NetworkIdMode::Hashed, record.network_hash),
        ] {
            let resolved = facts.for_registry(value, mode, hash).unwrap();
            assert_eq!(resolved, expected);
            assert!(facts.for_registry(value, mode, [0; 32]).is_none());
        }
    }
}
