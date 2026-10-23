use super::*;
use assets::NetworkIdMode;
use std::sync::{
    OnceLock,
    atomic::{AtomicUsize, Ordering},
};

/// Shares real coherent registry tables between fixtures without retaining custom session maps.
pub(crate) fn registry_base() -> RegistryBase {
    static BASE: OnceLock<RegistryBase> = OnceLock::new();
    BASE.get_or_init(|| {
        let collisions = collision_resource();
        RegistryBase::new(&Arc::new(assets::RuntimeAssets::diagnostic()), &collisions)
    })
    .clone()
}

/// Supplies the collision owner now captured by real session startup and teardown systems.
pub(crate) fn collision_resource() -> client_interaction::movement::PhysicsCollisionRegistries {
    let breg = assets::pinned_block_registry_bytes();
    let preg = include_bytes!("../../../../../crates/assets/data/block-physics-v2193.bin");
    let version = assets::active_content_registry_protocol();
    let records = assets::read_registry_for_protocol(breg, version).unwrap();
    client_interaction::movement::PhysicsCollisionRegistries::from_assets(
        breg, &records, preg, version,
    )
    .unwrap()
}

/// Supplies the prepared bundle required by tests that exercise the real bootstrap boundary.
pub(crate) fn prepared_packs(
    base: RegistryBase,
    generation: SessionGeneration,
    custom: &protocol::CustomBlocks,
    hashed: bool,
) -> PackApplication {
    let mut packs = PackApplication::default();
    packs.registries = base.prepare(generation, hashed, custom, None, &packs, &|| false);
    packs
}

/// Admits one original full-cube definition through the production shared-state cache.
fn custom(name: &str) -> protocol::CustomBlocks {
    protocol::CustomBlocks::from_definitions([(name, b"\x0a\x00\x00".as_slice())])
}

/// Prepares without cancellation for behavior comparisons.
fn prepare(
    base: RegistryBase,
    hashed: bool,
    custom: &protocol::CustomBlocks,
) -> PreparedSessionRegistries {
    base.prepare(
        SessionGeneration::new(7),
        hashed,
        custom,
        None,
        &PackApplication::default(),
        &|| false,
    )
    .unwrap()
}

#[test]
fn digger_queries_are_ready_in_the_worker_prepared_session_bundle() {
    let digger = protocol::ItemDigger {
        use_efficiency: false,
        destroy_speeds: Arc::new([protocol::DiggerSpeed {
            block: protocol::DiggerBlock::Tags("q.any_tag('test:stone')".into()),
            speed: 8,
        }]),
    };
    let registry = protocol::ItemRegistryEvent {
        entries: Arc::new([protocol::ItemRegistryEntry {
            components: Some(Arc::new(protocol::ItemComponents {
                digger: Some(Arc::new(digger.clone())),
                ..Default::default()
            })),
            ..protocol::vanilla_item_registry()[0].clone()
        }]),
    };
    let prepared = registry_base()
        .prepare(
            SessionGeneration::new(7),
            false,
            &protocol::CustomBlocks::default(),
            Some(&registry),
            &PackApplication::default(),
            &|| false,
        )
        .unwrap();
    assert_eq!(
        pack_runtime::digger_match(&digger, &prepared.digger_filters, "test:block", &|tag| tag
            == "test:stone"),
        pack_runtime::DiggerMatch::Speed(8)
    );
}

#[test]
fn custom_mining_filters_are_prepared_with_the_session_registries() {
    let mut blocks = custom("test:prepared_mining");
    Arc::make_mut(&mut blocks.blocks)[0].mining = Arc::new(protocol::CustomBlockMining {
        seconds_to_destroy: 4.0,
        item_specific_speeds: [protocol::CustomMiningSpeed {
            item: protocol::CustomMiningItem::Tags("q.any_tag('test:tools')".into()),
            destroy_speed: 0.5,
        }]
        .into(),
    });
    for hashed in [false, true] {
        let prepared = prepare(registry_base(), hashed, &blocks);
        assert_eq!(
            pack_runtime::custom_mining_seconds(
                &blocks.blocks[0].mining,
                &prepared.mining_filters,
                Some("test:tool"),
                0,
                &|tag| tag == "test:tools",
            ),
            0.5
        );
    }
    let replacement = prepare(registry_base(), false, &protocol::CustomBlocks::default());
    assert_eq!(
        pack_runtime::custom_mining_seconds(
            &blocks.blocks[0].mining,
            &replacement.mining_filters,
            Some("test:tool"),
            0,
            &|_| true,
        ),
        4.0,
        "a filter another session prepared matches no item"
    );
}

#[test]
fn preparation_preserves_base_and_matches_direct_collision_registration() {
    let base = registry_base();
    let blocks = custom("example:prepared_session");
    let hash = blocks.blocks[0].hashed_states()[0].hash;
    for hashed in [false, true] {
        let mode = if hashed {
            NetworkIdMode::Hashed
        } else {
            NetworkIdMode::Sequential
        };
        let mut direct = base.collisions.clone();
        let expected = direct.begin_session_blocks(&blocks, hashed);
        let prepared = prepare(base.clone(), hashed, &blocks);
        assert_eq!(prepared.registration.custom_ids, expected.custom_ids);
        assert_eq!(prepared.registration.remap, expected.remap);
        assert_eq!(prepared.registration.supported, expected.supported);
        let id = if hashed {
            hash
        } else {
            expected.custom_ids.unwrap().start
        };
        assert_eq!(
            prepared.collisions.block_identifier(mode, id),
            Some("example:prepared_session")
        );
        assert_eq!(
            prepared.collisions.block_facts(mode, id),
            direct.block_facts(mode, id)
        );
        assert_eq!(
            prepared.collisions.registry(mode).identity(),
            direct.registry(mode).identity()
        );
        assert!(!base.collisions.registry(mode).contains_runtime_id(id));
        assert!(Arc::ptr_eq(&prepared.assets, &base.assets));
        assert!(prepared.matches(
            SessionGeneration::new(7),
            hashed,
            &base.assets,
            &base.collisions,
            &blocks
        ));
    }
}

#[test]
fn prepared_inputs_reject_other_generations_modes_carriers_and_definitions() {
    let base = registry_base();
    let blocks = custom("example:matching_inputs");
    let prepared = prepare(base.clone(), true, &blocks);
    let matches = |generation,
                   hashed,
                   assets: &Arc<assets::RuntimeAssets>,
                   blocks: &protocol::CustomBlocks| {
        prepared.matches(
            SessionGeneration::new(generation),
            hashed,
            assets,
            &base.collisions,
            blocks,
        )
    };
    assert!(matches(7, true, &base.assets, &blocks.clone()));
    assert!(!matches(6, true, &base.assets, &blocks));
    assert!(!matches(8, true, &base.assets, &blocks));
    assert!(!matches(7, false, &base.assets, &blocks));
    assert!(!matches(
        7,
        true,
        &Arc::new(assets::RuntimeAssets::diagnostic()),
        &blocks
    ));
    assert!(!matches(
        7,
        true,
        &base.assets,
        &custom("example:matching_inputs")
    ));
    let mut changed = blocks.clone();
    changed.skipped += 1;
    assert!(!matches(7, true, &base.assets, &changed));
    changed = blocks.clone();
    changed.vanilla_blocks = Arc::from([Arc::from("example:other_admission")]);
    assert!(!matches(7, true, &base.assets, &changed));
}

#[test]
fn cancellation_never_publishes_partially_prepared_registries() {
    let base = registry_base();
    let blocks = custom("example:cancelled_session");
    let hash = blocks.blocks[0].hashed_states()[0].hash;
    for after in 0..4 {
        let polls = AtomicUsize::new(0);
        let cancelled = || polls.fetch_add(1, Ordering::Relaxed) >= after;
        assert!(
            base.clone()
                .prepare(
                    SessionGeneration::new(7),
                    true,
                    &blocks,
                    None,
                    &PackApplication::default(),
                    &cancelled
                )
                .is_none()
        );
        assert!(
            !base
                .collisions
                .registry(NetworkIdMode::Hashed)
                .contains_runtime_id(hash)
        );
    }
}

#[test]
fn later_sessions_remove_previous_customs_without_mutating_retained_snapshots() {
    let base = registry_base();
    let first_blocks = custom("example:first_session");
    let next_blocks = custom("example:next_session");
    let first_hash = first_blocks.blocks[0].hashed_states()[0].hash;
    let next_hash = next_blocks.blocks[0].hashed_states()[0].hash;
    let first = prepare(base.clone(), true, &first_blocks);
    let next_base = RegistryBase::new(&base.assets, &first.collisions);
    let next = prepare(next_base.clone(), true, &next_blocks);
    assert!(
        first
            .collisions
            .registry(NetworkIdMode::Hashed)
            .contains_runtime_id(first_hash)
    );
    assert!(
        !first
            .collisions
            .registry(NetworkIdMode::Hashed)
            .contains_runtime_id(next_hash)
    );
    assert!(
        !next
            .collisions
            .registry(NetworkIdMode::Hashed)
            .contains_runtime_id(first_hash)
    );
    assert!(
        next.collisions
            .registry(NetworkIdMode::Hashed)
            .contains_runtime_id(next_hash)
    );
    let empty = prepare(next_base, true, &protocol::CustomBlocks::default());
    assert!(
        !empty
            .collisions
            .registry(NetworkIdMode::Hashed)
            .contains_runtime_id(first_hash)
    );
    let sequential = prepare(
        RegistryBase::new(&base.assets, &next.collisions),
        false,
        &first_blocks,
    );
    let id = sequential.registration.custom_ids.unwrap().start;
    assert_eq!(
        sequential
            .collisions
            .block_identifier(NetworkIdMode::Sequential, id),
        Some("example:first_session")
    );
}

#[test]
fn overlay_assets_are_materialized_before_commit_and_keep_the_exact_base() {
    let base = registry_base();
    let blocks = protocol::CustomBlocks::default();
    let packs = PackApplication {
        block_overlay: Some(Arc::new(
            super::super::block_overlay::CompiledBlockOverlay {
                overlay: Default::default(),
                gaps: Default::default(),
                vanilla_carried: Vec::new(),
            },
        )),
        ..Default::default()
    };
    let prepared = base
        .clone()
        .prepare(
            SessionGeneration::new(7),
            true,
            &blocks,
            None,
            &packs,
            &|| false,
        )
        .unwrap();
    assert!(!Arc::ptr_eq(&prepared.assets, &base.assets));
    assert!(Arc::ptr_eq(&prepared.source_assets, &base.assets));
    assert_eq!(prepared.assets.visual_count(), base.assets.visual_count());
    assert!(prepared.matches(
        SessionGeneration::new(7),
        true,
        &base.assets,
        &base.collisions,
        &blocks
    ));
    assert!(!prepared.matches(
        SessionGeneration::new(7),
        true,
        &prepared.assets,
        &base.collisions,
        &blocks
    ));
}
