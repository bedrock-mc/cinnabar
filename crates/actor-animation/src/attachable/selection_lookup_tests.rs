//! Candidate rig lookup work stays bounded independently of predicate execution budgets.

use super::*;

/// Fills a catalog with valid selection-only rigs beyond the item candidates.
fn large_catalog() -> (Arc<RuntimeEntityAssets>, RuntimeEquipmentCatalog) {
    let (mut compiled, catalog) = candidate_parts(assets::MAX_EQUIPMENT_BINDINGS - 1, 4, true);
    let mut symbols = compiled.symbols.into_vec();
    let mut rigs = compiled.rig_bindings.into_vec();
    while rigs.len() < assets::MAX_ENTITY_RIG_BINDINGS {
        let mut symbol = symbols.last().unwrap().clone();
        symbol.identifier = format!("test:unused_{:04}", rigs.len()).into();
        let mut rig = *rigs.last().unwrap();
        rig.entity_symbol = symbols.len() as u32;
        symbols.push(symbol);
        rigs.push(rig);
    }
    compiled.symbols = symbols.into();
    compiled.rig_bindings = rigs.into();
    (
        Arc::new(RuntimeEntityAssets::from_compiled(compiled).unwrap()),
        catalog,
    )
}

#[test]
fn rejected_candidates_bound_rig_inspections_with_large_catalog_and_exhausted_budget() {
    let owner = crate::tests::actor_with_metadata(HashMap::new());
    let mut rig = tests::owner_rig();
    rig.actor.runtime_id = owner.runtime_id;
    rig.actor.spawn_revision = owner.spawn_revision;
    RuntimeEntityAssets::take_attachable_rig_work();
    let (assets, catalog) = large_catalog();
    assert!(RuntimeEntityAssets::take_attachable_rig_work() >= assets::MAX_ENTITY_RIG_BINDINGS);
    let mut runtime = AttachablesRuntime::new(assets);
    // Partition search plus the matching and duplicate checks may inspect logarithmically many rigs.
    let per_lookup = assets::MAX_ENTITY_RIG_BINDINGS.ilog2() as usize + 4;
    for limit in [
        MAX_MOLANG_OPS_PER_ACTOR_TICK,
        MAX_MOLANG_OPS_PER_ACTOR_TICK,
        1,
        0,
        0,
    ] {
        RuntimeEntityAssets::take_attachable_rig_work();
        let (selected, queries, spent) = select(&mut runtime, &catalog, &owner, &rig, limit);
        assert_eq!(selected, None, "all mapped predicates reject this owner");
        let inspected = RuntimeEntityAssets::take_attachable_rig_work();
        assert!(inspected >= catalog.bindings().len());
        assert!(
            inspected <= catalog.bindings().len() * per_lookup,
            "{inspected} rig inspections exceeded the candidate lookup bound"
        );
        assert!(spent <= limit);
        if limit == MAX_MOLANG_OPS_PER_ACTOR_TICK {
            assert!(
                queries > 0,
                "indexed candidates must reach owner evaluation"
            );
        }
        if limit == 0 {
            assert_eq!((queries, spent), (0, 0));
        }
    }
}

#[test]
fn lookup_preserves_missing_duplicate_and_selection_only_rigs_across_catalogs() {
    let (compiled, _) = candidate_parts(2, 4, false);
    let first = RuntimeEntityAssets::from_compiled(compiled.clone()).unwrap();
    let retained = first.clone();
    let names: Vec<_> = first
        .rig_bindings()
        .iter()
        .map(|rig| {
            first.symbols()[rig.entity_symbol as usize]
                .identifier
                .clone()
        })
        .collect();
    assert_eq!(first.attachable_rig_binding("test:missing"), None);
    for (index, name) in names.iter().enumerate() {
        assert_eq!(first.attachable_rig_binding(name), Some(index));
    }
    let decoded =
        RuntimeEntityAssets::decode(&assets::entity::encode_entity_blob(&compiled).unwrap())
            .unwrap();
    for (index, name) in names.iter().enumerate() {
        assert_eq!(decoded.attachable_rig_binding(name), Some(index));
    }
    assert_eq!(first.rig_bindings()[1].geometry_count, 0);

    let mut duplicate = compiled.clone();
    let mut rigs = duplicate.rig_bindings.into_vec();
    rigs.extend([rigs[1]; 2]);
    duplicate.rig_bindings = rigs.into();
    let duplicate = RuntimeEntityAssets::from_compiled(duplicate).unwrap();
    assert_eq!(duplicate.attachable_rig_binding(&names[1]), None);
    assert_eq!(duplicate.attachable_rig_binding(&names[2]), Some(2));

    let mut replacement = compiled;
    replacement.rig_bindings[1..].swap(0, 1);
    let replacement = RuntimeEntityAssets::from_compiled(replacement).unwrap();
    assert_eq!(replacement.attachable_rig_binding(&names[1]), Some(2));
    assert_eq!(replacement.attachable_rig_binding(&names[2]), Some(1));
    assert_eq!(retained.attachable_rig_binding(&names[1]), Some(1));
    assert_eq!(first.attachable_rig_binding(&names[2]), Some(2));

    let owner = crate::tests::actor_with_metadata(HashMap::new());
    let rig = tests::owner_rig();
    let mut runtime = AttachablesRuntime::new(Arc::new(duplicate));
    let mut left = 0;
    assert_eq!(
        runtime.matches_owner("test:missing", &owner, &rig, Default::default(), &mut left),
        None
    );
    assert_eq!(
        runtime.matches_owner(&names[1], &owner, &rig, Default::default(), &mut left),
        None,
        "an ambiguous mapping stays unresolved even at zero budget"
    );
    assert_eq!(
        runtime.matches_owner(&names[2], &owner, &rig, Default::default(), &mut left),
        Some(false),
        "an indexed conditional definition remains rejected at zero budget"
    );
    assert_eq!(
        runtime.matches_owner(&names[0], &owner, &rig, Default::default(), &mut left),
        Some(true),
        "the unrestricted ordinary definition remains available at zero budget"
    );
    assert_eq!(left, 0);
}
