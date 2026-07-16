use super::*;

#[test]
#[ignore = "requires CINNABAR_REAL_PACK pointing at the ignored pinned MCBEAS05 blob"]
fn production_ratchet_reports_exact_model_removals_for_the_full_real_pack() {
    let assets_path = std::env::var_os("CINNABAR_REAL_PACK")
        .map(std::path::PathBuf::from)
        .expect("set CINNABAR_REAL_PACK to the ignored pinned vanilla-v1001.mcbea");
    assert!(assets_path.is_file(), "missing real pack: {assets_path:?}");
    let registry_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/assets/data/block-registry-v1001.bin");
    let registry_bytes = std::fs::read(&registry_path).unwrap();
    let assets_bytes = std::fs::read(&assets_path).unwrap();
    let records = read_registry(&registry_bytes).expect("read full production registry");
    let baseline = parse_baseline(include_bytes!(
        "../../../../crates/assets/data/visual-coverage-v1001.json"
    ))
    .expect("parse committed production baseline");
    let current = analyze_bytes(&registry_bytes, &assets_bytes).unwrap();
    assert_eq!(current.states.len(), 16_913);
    assert_eq!(baseline.diagnostic_sequential_ids.len(), 2_398);
    assert_eq!(current.diagnostic_states.len(), 2_398);

    let expected_mineral_cube_ids = [12_638, 14_658];
    for &(sequential_id, name) in &[(12_638, "minecraft:cinnabar"), (14_658, "minecraft:sulfur")] {
        let record = &records[sequential_id as usize];
        assert_eq!(record.sequential_id, sequential_id);
        assert_eq!(record.name.as_ref(), name);
        assert_eq!(record.canonical_state.as_ref(), "{}");
        assert_eq!(record.model_family, ModelFamily::Unknown);
        assert_eq!(record.flags, BlockFlags::empty());
        assert_eq!(record.contributor_role, ContributorRole::Primary);
        assert!(
            baseline
                .diagnostic_sequential_ids
                .binary_search(&sequential_id)
                .is_err()
        );
    }
    let mut pre_mineral_cube_baseline = baseline.clone();
    pre_mineral_cube_baseline
        .diagnostic_sequential_ids
        .extend(expected_mineral_cube_ids);
    pre_mineral_cube_baseline
        .diagnostic_sequential_ids
        .sort_unstable();
    assert_eq!(
        pre_mineral_cube_baseline.diagnostic_sequential_ids.len(),
        2_400
    );
    let report = ratchet_protocol_1001(current.clone(), &pre_mineral_cube_baseline)
        .expect("run exact pre-mineral-cube production ratchet");
    assert!(report.added_diagnostics.is_empty());
    assert_eq!(report.removed_diagnostics.len(), 2);
    assert_eq!(
        report
            .removed_diagnostics
            .iter()
            .map(|state| state.sequential_id)
            .collect::<Vec<_>>(),
        expected_mineral_cube_ids
    );
    assert!(report.removed_diagnostics.iter().all(|state| {
        state.canonical_state == "{}"
            && state.model_family == "unknown"
            && !state.is_air
            && matches!(
                state.name.as_str(),
                "minecraft:cinnabar" | "minecraft:sulfur"
            )
    }));

    let expected_bee_housing_ids = (10_395..=10_418).chain(12_495..=12_518).collect::<Vec<_>>();
    for &sequential_id in &expected_bee_housing_ids {
        let record = &records[sequential_id as usize];
        assert_eq!(record.sequential_id, sequential_id);
        assert!(matches!(
            record.name.as_ref(),
            "minecraft:bee_nest" | "minecraft:beehive"
        ));
        assert_eq!(record.model_family, ModelFamily::Cube);
        assert_eq!(record.contributor_role, ContributorRole::Primary);
        let family_base = if record.name.as_ref() == "minecraft:bee_nest" {
            10_395
        } else {
            12_495
        };
        let state = sequential_id - family_base;
        assert_eq!(
            record.model_state.get(assets::ModelStateField::Orientation),
            Some(state % 4)
        );
        assert_eq!(
            record.model_state.get(assets::ModelStateField::Growth),
            Some(state / 4)
        );
        assert!(
            baseline
                .diagnostic_sequential_ids
                .binary_search(&sequential_id)
                .is_err()
        );
    }
    let mut pre_bee_housing_baseline = baseline.clone();
    pre_bee_housing_baseline
        .diagnostic_sequential_ids
        .extend(expected_bee_housing_ids.iter().copied());
    pre_bee_housing_baseline
        .diagnostic_sequential_ids
        .sort_unstable();
    assert_eq!(
        pre_bee_housing_baseline.diagnostic_sequential_ids.len(),
        2_446
    );
    let report = ratchet_protocol_1001(current.clone(), &pre_bee_housing_baseline)
        .expect("run exact pre-bee-housing production ratchet");
    assert!(report.added_diagnostics.is_empty());
    assert_eq!(report.removed_diagnostics.len(), 48);
    assert_eq!(
        report
            .removed_diagnostics
            .iter()
            .map(|state| state.sequential_id)
            .collect::<Vec<_>>(),
        expected_bee_housing_ids
    );
    assert!(report.removed_diagnostics.iter().all(|state| {
        matches!(
            state.name.as_str(),
            "minecraft:bee_nest" | "minecraft:beehive"
        ) && state.model_family == "cube"
            && !state.is_air
    }));

    let expected_farmland_ids = (6_122..=6_129).collect::<Vec<_>>();
    for &sequential_id in &expected_farmland_ids {
        let record = &records[sequential_id as usize];
        assert_eq!(record.sequential_id, sequential_id);
        assert_eq!(record.name.as_ref(), "minecraft:farmland");
        assert_eq!(record.model_family, ModelFamily::Cuboid);
        assert_eq!(record.contributor_role, ContributorRole::Primary);
        assert_eq!(
            record.model_state.get(assets::ModelStateField::Growth),
            Some(sequential_id - 6_122)
        );
        assert!(
            baseline
                .diagnostic_sequential_ids
                .binary_search(&sequential_id)
                .is_err()
        );
    }
    let mut pre_farmland_baseline = baseline.clone();
    pre_farmland_baseline
        .diagnostic_sequential_ids
        .extend(expected_farmland_ids.iter().copied());
    pre_farmland_baseline
        .diagnostic_sequential_ids
        .sort_unstable();
    assert_eq!(pre_farmland_baseline.diagnostic_sequential_ids.len(), 2_406);
    let report = ratchet_protocol_1001(current.clone(), &pre_farmland_baseline)
        .expect("run exact pre-farmland production ratchet");
    assert!(report.added_diagnostics.is_empty());
    assert_eq!(report.removed_diagnostics.len(), 8);
    assert_eq!(
        report
            .removed_diagnostics
            .iter()
            .map(|state| state.sequential_id)
            .collect::<Vec<_>>(),
        expected_farmland_ids
    );
    assert!(report.removed_diagnostics.iter().all(|state| {
        state.name == "minecraft:farmland" && state.model_family == "cuboid" && !state.is_air
    }));

    let expected_cake_ids = (14_055..=14_061).collect::<Vec<_>>();
    for &sequential_id in &expected_cake_ids {
        let record = &records[sequential_id as usize];
        assert_eq!(record.sequential_id, sequential_id);
        assert_eq!(record.name.as_ref(), "minecraft:cake");
        assert_eq!(record.model_family, ModelFamily::Cuboid);
        assert_eq!(record.contributor_role, ContributorRole::Primary);
        assert_eq!(
            record.model_state.get(assets::ModelStateField::Growth),
            Some(sequential_id - 14_055)
        );
        assert!(
            baseline
                .diagnostic_sequential_ids
                .binary_search(&sequential_id)
                .is_err()
        );
    }
    let mut pre_cake_baseline = baseline.clone();
    pre_cake_baseline
        .diagnostic_sequential_ids
        .extend(expected_cake_ids.iter().copied());
    pre_cake_baseline.diagnostic_sequential_ids.sort_unstable();
    assert_eq!(pre_cake_baseline.diagnostic_sequential_ids.len(), 2_405);
    let report = ratchet_protocol_1001(current.clone(), &pre_cake_baseline)
        .expect("run exact pre-cake production ratchet");
    assert!(report.added_diagnostics.is_empty());
    assert_eq!(report.removed_diagnostics.len(), 7);
    assert_eq!(
        report
            .removed_diagnostics
            .iter()
            .map(|state| state.sequential_id)
            .collect::<Vec<_>>(),
        expected_cake_ids
    );
    assert!(report.removed_diagnostics.iter().all(|state| {
        state.name == "minecraft:cake" && state.model_family == "cuboid" && !state.is_air
    }));

    let expected_cactus_ids = (13_606..=13_621).collect::<Vec<_>>();
    for &sequential_id in &expected_cactus_ids {
        let record = &records[sequential_id as usize];
        assert_eq!(record.sequential_id, sequential_id);
        assert_eq!(record.name.as_ref(), "minecraft:cactus");
        assert_eq!(record.model_family, ModelFamily::Cuboid);
        assert_eq!(record.contributor_role, ContributorRole::Primary);
        assert_eq!(
            record.model_state.get(assets::ModelStateField::Growth),
            Some(sequential_id - 13_606)
        );
        assert!(
            baseline
                .diagnostic_sequential_ids
                .binary_search(&sequential_id)
                .is_err()
        );
    }
    let mut pre_cactus_baseline = baseline.clone();
    pre_cactus_baseline
        .diagnostic_sequential_ids
        .extend(expected_cactus_ids.iter().copied());
    pre_cactus_baseline
        .diagnostic_sequential_ids
        .sort_unstable();
    assert_eq!(pre_cactus_baseline.diagnostic_sequential_ids.len(), 2_414);
    let report = ratchet_protocol_1001(current.clone(), &pre_cactus_baseline)
        .expect("run exact pre-cactus production ratchet");
    assert!(report.added_diagnostics.is_empty());
    assert_eq!(report.removed_diagnostics.len(), 16);
    assert_eq!(
        report
            .removed_diagnostics
            .iter()
            .map(|state| state.sequential_id)
            .collect::<Vec<_>>(),
        expected_cactus_ids
    );
    assert!(report.removed_diagnostics.iter().all(|state| {
        state.name == "minecraft:cactus" && state.model_family == "cuboid" && !state.is_air
    }));

    for &sequential_id in &SELECTOR_ALIAS_CUBE_REMOVALS {
        let record = &records[sequential_id as usize];
        assert_eq!(record.sequential_id, sequential_id);
        assert!(matches!(
            record.name.as_ref(),
            "minecraft:bone_block"
                | "minecraft:chiseled_quartz_block"
                | "minecraft:hay_block"
                | "minecraft:purpur_block"
                | "minecraft:quartz_block"
                | "minecraft:smooth_quartz"
                | "minecraft:tnt"
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
    let mut pre_selector_alias_baseline = baseline.clone();
    pre_selector_alias_baseline
        .diagnostic_sequential_ids
        .extend(SELECTOR_ALIAS_CUBE_REMOVALS);
    pre_selector_alias_baseline
        .diagnostic_sequential_ids
        .sort_unstable();
    assert_eq!(
        pre_selector_alias_baseline.diagnostic_sequential_ids.len(),
        2_425
    );
    let report = ratchet_protocol_1001(current.clone(), &pre_selector_alias_baseline)
        .expect("run exact pre-selector-alias production ratchet");
    assert!(report.added_diagnostics.is_empty());
    assert_eq!(report.removed_diagnostics.len(), 27);
    assert_eq!(
        report
            .removed_diagnostics
            .iter()
            .map(|state| state.sequential_id)
            .collect::<Vec<_>>(),
        SELECTOR_ALIAS_CUBE_REMOVALS
    );
    assert!(
        report
            .removed_diagnostics
            .iter()
            .all(|state| { state.model_family == "cube" && !state.is_air })
    );

    let expected_resin_clump_ids = (2_930..=2_993).collect::<Vec<_>>();
    for &sequential_id in &expected_resin_clump_ids {
        let record = &records[sequential_id as usize];
        assert_eq!(record.sequential_id, sequential_id);
        assert_eq!(record.name.as_ref(), "minecraft:resin_clump");
        assert_eq!(record.model_family, ModelFamily::ResinClump);
        assert_eq!(record.contributor_role, ContributorRole::Primary);
        assert_eq!(
            record.model_state.get(assets::ModelStateField::Connections),
            Some(sequential_id - 2_930)
        );
        assert!(
            baseline
                .diagnostic_sequential_ids
                .binary_search(&sequential_id)
                .is_err()
        );
    }
    let mut pre_resin_clump_baseline = baseline.clone();
    pre_resin_clump_baseline
        .diagnostic_sequential_ids
        .extend(expected_resin_clump_ids.iter().copied());
    pre_resin_clump_baseline
        .diagnostic_sequential_ids
        .sort_unstable();
    assert_eq!(
        pre_resin_clump_baseline.diagnostic_sequential_ids.len(),
        2_462
    );
    let report = ratchet_protocol_1001(current.clone(), &pre_resin_clump_baseline)
        .expect("run exact pre-resin-clump production ratchet");
    assert!(report.added_diagnostics.is_empty());
    assert_eq!(report.removed_diagnostics.len(), 64);
    assert_eq!(
        report
            .removed_diagnostics
            .iter()
            .map(|state| state.sequential_id)
            .collect::<Vec<_>>(),
        expected_resin_clump_ids
    );
    assert!(report.removed_diagnostics.iter().all(|state| {
        state.name == "minecraft:resin_clump"
            && state.model_family == "resin_clump"
            && !state.is_air
    }));

    let expected_chiseled_bookshelf_ids = (1_605..=1_860).collect::<Vec<_>>();
    for &sequential_id in &expected_chiseled_bookshelf_ids {
        let record = &records[sequential_id as usize];
        assert_eq!(record.sequential_id, sequential_id);
        assert_eq!(record.name.as_ref(), "minecraft:chiseled_bookshelf");
        assert_eq!(record.model_family, ModelFamily::ChiseledBookshelf);
        assert_eq!(record.contributor_role, ContributorRole::Primary);
        assert!(
            baseline
                .diagnostic_sequential_ids
                .binary_search(&sequential_id)
                .is_err()
        );
    }
    let mut pre_chiseled_bookshelf_baseline = baseline.clone();
    pre_chiseled_bookshelf_baseline
        .diagnostic_sequential_ids
        .extend(expected_chiseled_bookshelf_ids.iter().copied());
    pre_chiseled_bookshelf_baseline
        .diagnostic_sequential_ids
        .sort_unstable();
    assert_eq!(
        pre_chiseled_bookshelf_baseline
            .diagnostic_sequential_ids
            .len(),
        2_654
    );
    let report = ratchet_protocol_1001(current.clone(), &pre_chiseled_bookshelf_baseline)
        .expect("run exact pre-chiseled-bookshelf production ratchet");
    assert!(report.added_diagnostics.is_empty());
    assert_eq!(report.removed_diagnostics.len(), 256);
    assert_eq!(
        report
            .removed_diagnostics
            .iter()
            .map(|state| state.sequential_id)
            .collect::<Vec<_>>(),
        expected_chiseled_bookshelf_ids
    );
    assert!(report.removed_diagnostics.iter().all(|state| {
        state.name == "minecraft:chiseled_bookshelf"
            && state.model_family == "chiseled_bookshelf"
            && !state.is_air
    }));

    let expected_copper_grate_ids = COPPER_GRATE_REMOVALS.map(|(id, _)| id);
    for &(sequential_id, name) in &COPPER_GRATE_REMOVALS {
        let record = &records[sequential_id as usize];
        assert_eq!(record.sequential_id, sequential_id);
        assert_eq!(record.name.as_ref(), name);
        assert_eq!(record.canonical_state.as_ref(), "{}");
        assert_eq!(record.model_family, ModelFamily::Cube);
        assert_eq!(record.contributor_role, ContributorRole::Primary);
        assert!(
            baseline
                .diagnostic_sequential_ids
                .binary_search(&sequential_id)
                .is_err()
        );
    }
    let mut pre_copper_grate_baseline = baseline.clone();
    pre_copper_grate_baseline
        .diagnostic_sequential_ids
        .extend(expected_copper_grate_ids);
    pre_copper_grate_baseline
        .diagnostic_sequential_ids
        .sort_unstable();
    assert_eq!(
        pre_copper_grate_baseline.diagnostic_sequential_ids.len(),
        2_406
    );
    let report = ratchet_protocol_1001(current.clone(), &pre_copper_grate_baseline)
        .expect("run exact pre-copper-grate production ratchet");
    assert!(report.added_diagnostics.is_empty());
    assert_eq!(report.removed_diagnostics.len(), 8);
    assert_eq!(
        report
            .removed_diagnostics
            .iter()
            .map(|state| (state.sequential_id, state.name.as_str()))
            .collect::<Vec<_>>(),
        COPPER_GRATE_REMOVALS
    );
    assert!(report.removed_diagnostics.iter().all(|state| {
        state.canonical_state == "{}" && state.model_family == "cube" && !state.is_air
    }));

    let expected_stained_glass_ids = STAINED_GLASS_REMOVALS.map(|(id, _)| id);
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
    let mut pre_stained_glass_baseline = baseline.clone();
    pre_stained_glass_baseline
        .diagnostic_sequential_ids
        .extend(expected_stained_glass_ids);
    pre_stained_glass_baseline
        .diagnostic_sequential_ids
        .sort_unstable();
    assert_eq!(
        pre_stained_glass_baseline.diagnostic_sequential_ids.len(),
        2_414
    );
    assert_eq!(2_414 + COPPER_GRATE_REMOVALS.len(), 2_422);
    let report = ratchet_protocol_1001(current.clone(), &pre_stained_glass_baseline)
        .expect("run exact pre-stained-glass production ratchet");
    assert!(report.added_diagnostics.is_empty());
    assert_eq!(report.removed_diagnostics.len(), 16);
    assert_eq!(
        report
            .removed_diagnostics
            .iter()
            .map(|state| (state.sequential_id, state.name.as_str()))
            .collect::<Vec<_>>(),
        STAINED_GLASS_REMOVALS
    );
    assert!(report.removed_diagnostics.iter().all(|state| {
        state.canonical_state == "{}" && state.model_family == "cube" && !state.is_air
    }));

    let expected_sign_ids = records
        .iter()
        .filter(|record| record.model_family == ModelFamily::Sign)
        .map(|record| record.sequential_id)
        .collect::<Vec<_>>();
    assert_eq!(expected_sign_ids.len(), 4_872);
    assert!(expected_sign_ids.iter().all(|id| {
        baseline
            .diagnostic_sequential_ids
            .binary_search(id)
            .is_err()
    }));
    let mut pre_sign_baseline = baseline.clone();
    pre_sign_baseline
        .diagnostic_sequential_ids
        .extend(expected_sign_ids.iter().copied());
    pre_sign_baseline.diagnostic_sequential_ids.sort_unstable();
    assert_eq!(pre_sign_baseline.diagnostic_sequential_ids.len(), 7_270);
    let report = ratchet_protocol_1001(current.clone(), &pre_sign_baseline)
        .expect("run exact pre-sign production ratchet");
    assert!(report.added_diagnostics.is_empty());
    assert_eq!(report.removed_diagnostics.len(), 4_872);
    assert!(
        report
            .removed_diagnostics
            .iter()
            .all(|state| state.model_family == "sign")
    );
    assert_eq!(
        report
            .removed_diagnostics
            .iter()
            .map(|state| state.sequential_id)
            .collect::<Vec<_>>(),
        expected_sign_ids
    );

    let expected_multiface_ids = records
        .iter()
        .filter(|record| {
            matches!(
                record.model_family,
                ModelFamily::GlowLichen | ModelFamily::SculkVein
            )
        })
        .map(|record| record.sequential_id)
        .collect::<Vec<_>>();
    assert_eq!(expected_multiface_ids.len(), 128);
    assert!(expected_multiface_ids.iter().all(|id| {
        baseline
            .diagnostic_sequential_ids
            .binary_search(id)
            .is_err()
    }));
    let mut pre_multiface_baseline = baseline.clone();
    pre_multiface_baseline
        .diagnostic_sequential_ids
        .extend(expected_multiface_ids.iter().copied());
    pre_multiface_baseline
        .diagnostic_sequential_ids
        .sort_unstable();
    assert_eq!(
        pre_multiface_baseline.diagnostic_sequential_ids.len(),
        2_526
    );
    assert_eq!(2_526 + COPPER_GRATE_REMOVALS.len(), 2_534);
    assert_eq!(2_534 + STAINED_GLASS_REMOVALS.len(), 2_550);
    let report = ratchet_protocol_1001(current.clone(), &pre_multiface_baseline)
        .expect("run exact pre-multiface production ratchet");
    assert!(report.added_diagnostics.is_empty());
    assert_eq!(report.removed_diagnostics.len(), 128);
    assert!(
        report
            .removed_diagnostics
            .iter()
            .all(|state| { matches!(state.model_family.as_str(), "glow_lichen" | "sculk_vein") })
    );
    assert_eq!(
        report
            .removed_diagnostics
            .iter()
            .map(|state| state.sequential_id)
            .collect::<Vec<_>>(),
        expected_multiface_ids
    );
    let expected_gate_ids = records
        .iter()
        .filter(|record| record.model_family == ModelFamily::Gate)
        .map(|record| record.sequential_id)
        .collect::<Vec<_>>();
    assert_eq!(expected_gate_ids.len(), 192);
    assert!(expected_gate_ids.iter().all(|id| {
        baseline
            .diagnostic_sequential_ids
            .binary_search(id)
            .is_err()
    }));

    let mut pre_gate_baseline = baseline.clone();
    pre_gate_baseline
        .diagnostic_sequential_ids
        .extend(expected_gate_ids.iter().copied());
    pre_gate_baseline.diagnostic_sequential_ids.sort_unstable();
    assert_eq!(pre_gate_baseline.diagnostic_sequential_ids.len(), 2_590);
    assert_eq!(2_590 + COPPER_GRATE_REMOVALS.len(), 2_598);
    assert_eq!(2_598 + STAINED_GLASS_REMOVALS.len(), 2_614);
    let report = ratchet_protocol_1001(current.clone(), &pre_gate_baseline)
        .expect("run exact pre-Gate production ratchet");
    assert!(report.added_diagnostics.is_empty());
    assert_eq!(report.removed_diagnostics.len(), 192);
    assert!(
        report
            .removed_diagnostics
            .iter()
            .all(|state| state.model_family == "gate")
    );
    let removed_ids = report
        .removed_diagnostics
        .iter()
        .map(|state| state.sequential_id)
        .collect::<Vec<_>>();
    assert_eq!(removed_ids, expected_gate_ids);

    let expected_carpet_ids = records
        .iter()
        .filter(|record| record.model_family == ModelFamily::Carpet)
        .map(|record| record.sequential_id)
        .collect::<Vec<_>>();
    assert_eq!(expected_carpet_ids.len(), 179);
    assert!(expected_carpet_ids.iter().all(|id| {
        baseline
            .diagnostic_sequential_ids
            .binary_search(id)
            .is_err()
    }));
    let mut pre_carpet_baseline = baseline.clone();
    pre_carpet_baseline
        .diagnostic_sequential_ids
        .extend(expected_carpet_ids.iter().copied());
    pre_carpet_baseline
        .diagnostic_sequential_ids
        .sort_unstable();
    assert_eq!(pre_carpet_baseline.diagnostic_sequential_ids.len(), 2_577);
    assert_eq!(2_577 + COPPER_GRATE_REMOVALS.len(), 2_585);
    assert_eq!(2_585 + STAINED_GLASS_REMOVALS.len(), 2_601);
    let report = ratchet_protocol_1001(current.clone(), &pre_carpet_baseline)
        .expect("run exact pre-Carpet production ratchet");
    assert!(report.added_diagnostics.is_empty());
    assert_eq!(report.removed_diagnostics.len(), 179);
    assert!(
        report
            .removed_diagnostics
            .iter()
            .all(|state| state.model_family == "carpet")
    );
    let removed_ids = report
        .removed_diagnostics
        .iter()
        .map(|state| state.sequential_id)
        .collect::<Vec<_>>();
    assert_eq!(removed_ids, expected_carpet_ids);

    let expected_button_ids = records
        .iter()
        .filter(|record| record.model_family == ModelFamily::Button)
        .map(|record| record.sequential_id)
        .collect::<Vec<_>>();
    assert_eq!(expected_button_ids.len(), 168);
    assert!(expected_button_ids.iter().all(|id| {
        baseline
            .diagnostic_sequential_ids
            .binary_search(id)
            .is_err()
    }));
    let mut pre_button_baseline = baseline.clone();
    pre_button_baseline
        .diagnostic_sequential_ids
        .extend(expected_button_ids.iter().copied());
    pre_button_baseline
        .diagnostic_sequential_ids
        .sort_unstable();
    assert_eq!(pre_button_baseline.diagnostic_sequential_ids.len(), 2_566);
    assert_eq!(2_566 + COPPER_GRATE_REMOVALS.len(), 2_574);
    assert_eq!(2_574 + STAINED_GLASS_REMOVALS.len(), 2_590);
    let report = ratchet_protocol_1001(current.clone(), &pre_button_baseline)
        .expect("run exact pre-Button production ratchet");
    assert!(report.added_diagnostics.is_empty());
    assert_eq!(report.removed_diagnostics.len(), 168);
    assert!(
        report
            .removed_diagnostics
            .iter()
            .all(|state| state.model_family == "button")
    );
    let removed_ids = report
        .removed_diagnostics
        .iter()
        .map(|state| state.sequential_id)
        .collect::<Vec<_>>();
    assert_eq!(removed_ids, expected_button_ids);

    let expected_huge_mushroom_ids = records
        .iter()
        .filter(|record| {
            matches!(
                record.name.as_ref(),
                "minecraft:brown_mushroom_block"
                    | "minecraft:mushroom_stem"
                    | "minecraft:red_mushroom_block"
            )
        })
        .map(|record| record.sequential_id)
        .collect::<Vec<_>>();
    assert_eq!(expected_huge_mushroom_ids.len(), 48);
    assert!(expected_huge_mushroom_ids.iter().all(|id| {
        baseline
            .diagnostic_sequential_ids
            .binary_search(id)
            .is_err()
    }));
    let mut pre_huge_mushroom_baseline = baseline.clone();
    pre_huge_mushroom_baseline
        .diagnostic_sequential_ids
        .extend(expected_huge_mushroom_ids.iter().copied());
    pre_huge_mushroom_baseline
        .diagnostic_sequential_ids
        .sort_unstable();
    assert_eq!(
        pre_huge_mushroom_baseline.diagnostic_sequential_ids.len(),
        2_446
    );
    assert_eq!(2_446 + COPPER_GRATE_REMOVALS.len(), 2_454);
    assert_eq!(2_454 + STAINED_GLASS_REMOVALS.len(), 2_470);
    let report = ratchet_protocol_1001(current.clone(), &pre_huge_mushroom_baseline)
        .expect("run exact pre-huge-mushroom production ratchet");
    assert!(report.added_diagnostics.is_empty());
    assert_eq!(report.removed_diagnostics.len(), 48);
    assert!(report.removed_diagnostics.iter().all(|state| {
        state.model_family == "cube"
            && matches!(
                state.name.as_str(),
                "minecraft:brown_mushroom_block"
                    | "minecraft:mushroom_stem"
                    | "minecraft:red_mushroom_block"
            )
    }));
    let removed_ids = report
        .removed_diagnostics
        .iter()
        .map(|state| state.sequential_id)
        .collect::<Vec<_>>();
    assert_eq!(removed_ids, expected_huge_mushroom_ids);

    let refreshed = ratchet_protocol_1001(current, &baseline)
        .expect("run refreshed production coverage ratchet");
    assert!(refreshed.added_diagnostics.is_empty());
    assert!(refreshed.removed_diagnostics.is_empty());
}
