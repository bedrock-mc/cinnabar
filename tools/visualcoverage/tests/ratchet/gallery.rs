use super::{support::*, *};

fn full_gallery_fixture(
    diagnostic_ids: &std::collections::BTreeSet<u32>,
) -> (Vec<u8>, Vec<u8>, Baseline) {
    full_gallery_fixture_with_strict_invalid_route(diagnostic_ids, false)
}

fn full_gallery_fixture_with_strict_invalid_route(
    diagnostic_ids: &std::collections::BTreeSet<u32>,
    strict_invalid: bool,
) -> (Vec<u8>, Vec<u8>, Baseline) {
    let mut records = read_registry(include_bytes!(
        "../../../../crates/assets/data/block-registry-v1001.bin"
    ))
    .expect("read production registry");
    for record in &mut records {
        if !record.flags.contains(BlockFlags::AIR) {
            record.model_family = ModelFamily::Cube;
        }
    }
    let registry = registry_bytes(&records);
    let mut visuals = records
        .iter()
        .map(|record| {
            if record.flags.contains(BlockFlags::AIR) {
                strict_no_draw(BlockFlags::AIR, ContributorRole::Air)
            } else if diagnostic_ids.contains(&record.sequential_id) {
                strict_diagnostic(BlockFlags::empty(), ContributorRole::Primary)
            } else {
                strict_cube([1; 6])
            }
        })
        .collect::<Vec<_>>();
    if strict_invalid {
        visuals
            .iter_mut()
            .find(|visual| visual.kind == VisualKind::Cube)
            .expect("at least one visible gallery target")
            .faces[0] = DIAGNOSTIC_MATERIAL;
    }
    let assets = strict_blob(
        &records,
        visuals,
        strict_materials(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    let snapshot = analyze_bytes(&registry, &assets).expect("analyze full gallery fixture");
    let mut baseline = baseline(&snapshot);
    baseline.expected_vine_diagnostic_masks = snapshot.vine_diagnostic_masks.clone();
    (registry, assets, baseline)
}

#[test]
fn gallery_inventory_has_exact_67_page_shape() {
    let (registry, assets, baseline) = full_gallery_fixture(&Default::default());
    let baseline = deterministic_json(&baseline).unwrap();
    let inventory = gallery_inventory_bytes(&registry, &assets, &baseline).unwrap();

    assert_eq!(inventory.schema, GALLERY_INVENTORY_SCHEMA);
    assert_eq!(inventory.pages.len(), 67);
    assert!(
        inventory.pages[..66]
            .iter()
            .all(|page| page.targets.len() == GALLERY_PAGE_CAPACITY)
    );
    assert_eq!(inventory.pages[66].targets.len(), 17);
    assert_eq!(inventory.pages[0].first_sequential_id, 0);
    assert_eq!(inventory.pages[65].last_sequential_id, 16_895);
    assert_eq!(inventory.pages[66].first_sequential_id, 16_896);
    assert_eq!(inventory.pages[66].last_sequential_id, 16_912);
}

#[test]
fn gallery_inventory_contains_every_sequential_id_once_and_in_order() {
    let (registry, assets, baseline) = full_gallery_fixture(&Default::default());
    let baseline = deterministic_json(&baseline).unwrap();
    let inventory = gallery_inventory_bytes(&registry, &assets, &baseline).unwrap();
    let ids = inventory
        .pages
        .iter()
        .flat_map(|page| page.targets.iter().map(|target| target.sequential_id))
        .collect::<Vec<_>>();

    assert_eq!(ids, (0..16_913).collect::<Vec<_>>());
    for (index, page) in inventory.pages.iter().enumerate() {
        assert_eq!(page.index, index as u32);
    }
}

#[test]
fn gallery_inventory_is_hash_bound_and_byte_identical() {
    let (registry, assets, baseline) = full_gallery_fixture(&Default::default());
    let baseline_bytes = deterministic_json(&baseline).unwrap();
    let first = gallery_inventory_bytes(&registry, &assets, &baseline_bytes).unwrap();
    let second = gallery_inventory_bytes(&registry, &assets, &baseline_bytes).unwrap();

    assert_eq!(
        deterministic_json(&first).unwrap(),
        deterministic_json(&second).unwrap()
    );
    assert_eq!(
        first.registry_sha256,
        format!("{:x}", Sha256::digest(&registry))
    );
    assert_eq!(
        first.assets_sha256,
        format!("{:x}", Sha256::digest(&assets))
    );
    assert_eq!(
        first.baseline_sha256,
        format!("{:x}", Sha256::digest(&baseline_bytes))
    );
    assert!(first.accepting);
    assert_eq!(first.diagnostic_targets, 0);
}

#[test]
fn gallery_inventory_is_non_accepting_when_zero_diagnostics_hide_a_strict_invalid_route() {
    let (registry, assets, baseline) =
        full_gallery_fixture_with_strict_invalid_route(&Default::default(), true);
    let baseline_bytes = deterministic_json(&baseline).unwrap();
    let inventory = gallery_inventory_bytes(&registry, &assets, &baseline_bytes).unwrap();

    assert_eq!(inventory.diagnostic_targets, 0);
    assert!(!inventory.accepting);
}

#[test]
#[ignore = "requires CINNABAR_REAL_PACK pointing at the ignored pinned vanilla-v1001.mcbea"]
fn current_gallery_inventory_is_non_accepting_with_2400_diagnostics() {
    let assets_path = std::env::var_os("CINNABAR_REAL_PACK")
        .map(std::path::PathBuf::from)
        .expect("set CINNABAR_REAL_PACK to the ignored pinned vanilla-v1001.mcbea");
    let registry = include_bytes!("../../../../crates/assets/data/block-registry-v1001.bin");
    let assets = std::fs::read(assets_path).unwrap();
    let baseline = include_bytes!("../../../../crates/assets/data/visual-coverage-v1001.json");
    let inventory = gallery_inventory_bytes(registry, &assets, baseline).unwrap();

    assert!(!inventory.accepting);
    assert_eq!(inventory.diagnostic_targets, 2_400);
    assert_eq!(
        inventory
            .pages
            .iter()
            .flat_map(|page| &page.targets)
            .filter(|target| target.status == visualcoverage::GalleryTargetStatus::Diagnostic)
            .count(),
        2_400
    );
}

#[test]
fn gallery_inventory_cli_preserves_output_on_failure() {
    let directory = tempfile::tempdir().unwrap();
    let registry_path = directory.path().join("registry.bin");
    let assets_path = directory.path().join("assets.mcbea");
    let baseline_path = directory.path().join("baseline.json");
    let output_path = directory.path().join("inventory.json");
    let (registry, assets, mut baseline) = full_gallery_fixture(&Default::default());
    baseline.registry_sha256 = "0".repeat(64);
    std::fs::write(&registry_path, registry).unwrap();
    std::fs::write(&assets_path, assets).unwrap();
    std::fs::write(&baseline_path, deterministic_json(&baseline).unwrap()).unwrap();
    std::fs::write(&output_path, b"reviewed-inventory\n").unwrap();

    let run = std::process::Command::new(env!("CARGO_BIN_EXE_visualcoverage"))
        .args([
            "gallery-inventory",
            "--registry",
            registry_path.to_str().unwrap(),
            "--assets",
            assets_path.to_str().unwrap(),
            "--baseline",
            baseline_path.to_str().unwrap(),
            "--out",
            output_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(!run.status.success());
    assert_eq!(
        std::fs::read(&output_path).unwrap(),
        b"reviewed-inventory\n"
    );
    assert_no_atomic_temp_artifacts(directory.path());
}

#[test]
fn committed_protocol_baseline_binds_the_complete_corpus_and_all_vines() {
    let baseline = parse_baseline(include_bytes!(
        "../../../../crates/assets/data/visual-coverage-v1001.json"
    ))
    .expect("parse committed baseline");
    let records = read_registry(include_bytes!(
        "../../../../crates/assets/data/block-registry-v1001.bin"
    ))
    .expect("read committed registry");

    assert_eq!(
        baseline.counts,
        Counts {
            names: 1_356,
            states: 16_913,
            air: 1,
        }
    );
    assert_eq!(
        baseline.states,
        records
            .iter()
            .map(StateIdentity::from_record)
            .collect::<Vec<_>>()
    );
    assert!(baseline.expected_vine_diagnostic_masks.is_empty());
    assert!(
        records
            .iter()
            .filter(|record| record.name.as_ref() == "minecraft:vine")
            .all(|record| !baseline
                .diagnostic_sequential_ids
                .contains(&record.sequential_id))
    );
    let carpets = records
        .iter()
        .filter(|record| record.model_family == ModelFamily::Carpet)
        .collect::<Vec<_>>();
    assert_eq!(carpets.len(), 179);
    assert!(carpets.iter().all(|record| {
        baseline
            .diagnostic_sequential_ids
            .binary_search(&record.sequential_id)
            .is_err()
    }));
    let buttons = records
        .iter()
        .filter(|record| record.model_family == ModelFamily::Button)
        .collect::<Vec<_>>();
    assert_eq!(buttons.len(), 168);
    assert!(buttons.iter().all(|record| {
        baseline
            .diagnostic_sequential_ids
            .binary_search(&record.sequential_id)
            .is_err()
    }));
    let huge_mushrooms = records
        .iter()
        .filter(|record| {
            matches!(
                record.name.as_ref(),
                "minecraft:brown_mushroom_block"
                    | "minecraft:mushroom_stem"
                    | "minecraft:red_mushroom_block"
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(huge_mushrooms.len(), 48);
    assert!(huge_mushrooms.iter().all(|record| {
        baseline
            .diagnostic_sequential_ids
            .binary_search(&record.sequential_id)
            .is_err()
    }));
    for &(sequential_id, name) in &STAINED_GLASS_REMOVALS {
        let record = &records[sequential_id as usize];
        assert_eq!(record.sequential_id, sequential_id);
        assert_eq!(record.name.as_ref(), name);
        assert_eq!(record.canonical_state.as_ref(), "{}");
        assert_eq!(record.model_family, ModelFamily::Cube);
        assert!(
            baseline
                .diagnostic_sequential_ids
                .binary_search(&sequential_id)
                .is_err()
        );
    }
    for &(sequential_id, name) in &COPPER_GRATE_REMOVALS {
        let record = &records[sequential_id as usize];
        assert_eq!(record.sequential_id, sequential_id);
        assert_eq!(record.name.as_ref(), name);
        assert_eq!(record.canonical_state.as_ref(), "{}");
        assert_eq!(record.model_family, ModelFamily::Cube);
        assert!(
            baseline
                .diagnostic_sequential_ids
                .binary_search(&sequential_id)
                .is_err()
        );
    }
    let chiseled_bookshelves = records
        .iter()
        .filter(|record| record.model_family == ModelFamily::ChiseledBookshelf)
        .collect::<Vec<_>>();
    assert_eq!(chiseled_bookshelves.len(), 256);
    assert!(chiseled_bookshelves.iter().all(|record| {
        baseline
            .diagnostic_sequential_ids
            .binary_search(&record.sequential_id)
            .is_err()
    }));
    let resin_clumps = records
        .iter()
        .filter(|record| record.model_family == ModelFamily::ResinClump)
        .collect::<Vec<_>>();
    assert_eq!(resin_clumps.len(), 64);
    assert!(resin_clumps.iter().all(|record| {
        baseline
            .diagnostic_sequential_ids
            .binary_search(&record.sequential_id)
            .is_err()
    }));
    for &sequential_id in &SELECTOR_ALIAS_CUBE_REMOVALS {
        let record = &records[sequential_id as usize];
        assert_eq!(record.sequential_id, sequential_id);
        assert_eq!(record.model_family, ModelFamily::Cube);
        assert_eq!(
            record.flags,
            BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE
        );
        assert!(
            baseline
                .diagnostic_sequential_ids
                .binary_search(&sequential_id)
                .is_err()
        );
    }
    for sequential_id in 13_606..=13_621 {
        let record = &records[sequential_id as usize];
        assert_eq!(record.sequential_id, sequential_id);
        assert_eq!(record.name.as_ref(), "minecraft:cactus");
        assert_eq!(record.model_family, ModelFamily::Cuboid);
        assert_eq!(record.contributor_role, ContributorRole::Primary);
        assert!(
            baseline
                .diagnostic_sequential_ids
                .binary_search(&sequential_id)
                .is_err()
        );
    }
    for sequential_id in 14_055..=14_061 {
        let record = &records[sequential_id as usize];
        assert_eq!(record.sequential_id, sequential_id);
        assert_eq!(record.name.as_ref(), "minecraft:cake");
        assert_eq!(record.model_family, ModelFamily::Cuboid);
        assert_eq!(record.contributor_role, ContributorRole::Primary);
        assert!(
            baseline
                .diagnostic_sequential_ids
                .binary_search(&sequential_id)
                .is_err()
        );
    }
    for sequential_id in 6_122..=6_129 {
        let record = &records[sequential_id as usize];
        assert_eq!(record.sequential_id, sequential_id);
        assert_eq!(record.name.as_ref(), "minecraft:farmland");
        assert_eq!(record.model_family, ModelFamily::Cuboid);
        assert_eq!(record.contributor_role, ContributorRole::Primary);
        assert!(
            baseline
                .diagnostic_sequential_ids
                .binary_search(&sequential_id)
                .is_err()
        );
    }
    for sequential_id in (10_395..=10_418).chain(12_495..=12_518) {
        let record = &records[sequential_id as usize];
        assert_eq!(record.sequential_id, sequential_id);
        assert!(matches!(
            record.name.as_ref(),
            "minecraft:bee_nest" | "minecraft:beehive"
        ));
        assert_eq!(record.model_family, ModelFamily::Cube);
        assert_eq!(record.contributor_role, ContributorRole::Primary);
        assert!(
            baseline
                .diagnostic_sequential_ids
                .binary_search(&sequential_id)
                .is_err()
        );
    }
    let shelf_names = [
        "minecraft:acacia_shelf",
        "minecraft:bamboo_shelf",
        "minecraft:birch_shelf",
        "minecraft:cherry_shelf",
        "minecraft:crimson_shelf",
        "minecraft:dark_oak_shelf",
        "minecraft:jungle_shelf",
        "minecraft:mangrove_shelf",
        "minecraft:oak_shelf",
        "minecraft:pale_oak_shelf",
        "minecraft:spruce_shelf",
        "minecraft:warped_shelf",
    ];
    let shelves = records
        .iter()
        .filter(|record| shelf_names.contains(&record.name.as_ref()))
        .collect::<Vec<_>>();
    assert_eq!(shelves.len(), 384);
    assert!(shelves.iter().all(|record| {
        record.model_family == ModelFamily::Cuboid
            && baseline
                .diagnostic_sequential_ids
                .binary_search(&record.sequential_id)
                .is_ok()
    }));
    assert_eq!(baseline.diagnostic_sequential_ids.len(), 2_400);
}
