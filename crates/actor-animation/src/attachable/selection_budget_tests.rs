//! Candidate selection budgets include actual cold and cached query evaluations.
use super::*;
use assets::{EquipmentBinding, EquipmentReference, EquipmentTransform, RuntimeEquipmentCatalog};

/// Builds retained item mappings with shared or distinct query-heavy owner predicates.
fn candidates(
    count: usize,
    reads: usize,
    shared: bool,
) -> (Arc<RuntimeEntityAssets>, RuntimeEquipmentCatalog) {
    let (compiled, catalog) = candidate_parts(count, reads, shared);
    (
        Arc::new(RuntimeEntityAssets::from_compiled(compiled).unwrap()),
        catalog,
    )
}

/// Builds sorted candidate symbols and selection-only rigs without overlapping geometry ranges.
fn candidate_parts(
    count: usize,
    reads: usize,
    shared: bool,
) -> (assets::CompiledEntityAssets, RuntimeEquipmentCatalog) {
    let mut compiled = owner_reference_tests::compiled_fixture(true);
    compiled.rig_bindings[0].item_condition = None;
    compiled.molang_symbols[1].identifier = "query.is_sneaking".into();
    let mut expressions = Vec::new();
    let mut ops = Vec::new();
    for _ in 0..if shared { 1 } else { count } {
        let first_op = ops.len() as u32;
        for _ in 1..reads {
            ops.extend([assets::MolangOp::LoadQuery(1), assets::MolangOp::Pop]);
        }
        ops.push(assets::MolangOp::LoadQuery(1));
        expressions.push(assets::CompiledMolangExpression {
            first_op,
            op_count: u16::try_from(ops.len() as u32 - first_op).unwrap(),
            max_stack: 1,
        });
    }
    compiled.molang_expressions = expressions.into();
    compiled.molang_ops = ops.into();
    let ordinary = EquipmentBinding {
        identifier: "minecraft:test_item".into(),
        attachable: None,
        category: assets::EquipmentCategory::Held,
        geometry: EquipmentReference {
            identifier: "geometry.item".into(),
            resolution: assets::EntityDependencyResolution::Catalog,
        },
        texture: EquipmentReference {
            identifier: "textures/items/item".into(),
            resolution: assets::EntityDependencyResolution::Catalog,
        },
        resources: Default::default(),
        material: "entity_alphatest".into(),
        color_mask: assets::EquipmentColorMask::NoMask,
        render_controller: "controller.render.item".into(),
        first_person: EquipmentTransform::NeedsMeasurement,
        third_person: EquipmentTransform::NeedsMeasurement,
        dropped: EquipmentTransform::NeedsMeasurement,
        poses: Box::default(),
    };
    let mut bindings = vec![ordinary.clone()];
    let mut symbols = compiled.symbols.into_vec();
    let mut rigs = compiled.rig_bindings.into_vec();
    for index in 0..count {
        let name: Box<str> = format!("test:candidate_{index:04}").into();
        let mut symbol = symbols[4].clone();
        symbol.identifier = name.clone();
        let mut rig = rigs[0];
        rig.entity_symbol = symbols.len() as u32;
        rig.first_geometry = compiled.rig_geometries.len() as u32;
        rig.geometry_count = 0;
        rig.render_controller = None;
        rig.item_condition = Some(if shared { 0 } else { index as u32 });
        symbols.push(symbol);
        rigs.push(rig);
        bindings.push(EquipmentBinding {
            attachable: Some(name),
            ..ordinary.clone()
        });
    }
    compiled.symbols = symbols.into();
    compiled.rig_bindings = rigs.into();
    let catalog = RuntimeEquipmentCatalog::from_parts([1; 32], bindings, Vec::new()).unwrap();
    (compiled, catalog)
}

#[path = "selection_lookup_tests.rs"]
mod lookup;

#[path = "selection_preparation_tests.rs"]
mod preparation;

/// Selects actual candidates and checks measured query work against the caller's spent budget.
fn select(
    runtime: &mut AttachablesRuntime,
    catalog: &RuntimeEquipmentCatalog,
    owner: &dyn AnimatedActor,
    rig: &ActorRigSnapshot<'_>,
    limit: usize,
) -> (Option<Box<str>>, usize, usize) {
    crate::evaluation::take_query_work();
    let mut left = limit;
    let selected = runtime
        .select_equipment_binding(
            catalog,
            "minecraft:test_item",
            owner,
            rig,
            AttachableAnimationInput::default(),
            &mut left,
            |_| true,
        )
        .unwrap();
    let queries = crate::evaluation::take_query_work();
    assert!(
        queries <= limit - left,
        "{queries} queries exceeded {} charged operations",
        limit - left
    );
    (
        catalog.bindings()[selected].attachable.clone(),
        queries,
        limit - left,
    )
}

#[test]
fn shared_and_unique_false_candidates_bound_cold_and_warm_query_work() {
    let owner = crate::tests::actor_with_metadata(HashMap::new());
    let mut rig = tests::owner_rig();
    rig.actor.runtime_id = owner.runtime_id;
    rig.actor.spawn_revision = owner.spawn_revision;
    for (shared, reads) in [(true, 500), (false, 4)] {
        let (assets, catalog) = candidates(assets::MAX_EQUIPMENT_BINDINGS - 1, reads, shared);
        let mut runtime = AttachablesRuntime::new(assets);
        for _ in 0..3 {
            let (selected, queries, spent) = select(
                &mut runtime,
                &catalog,
                &owner,
                &rig,
                MAX_MOLANG_OPS_PER_ACTOR_TICK,
            );
            assert_eq!(
                selected, None,
                "the unrestricted ordinary definition remains the fallback"
            );
            assert!(queries > 0);
            assert_eq!(spent, MAX_MOLANG_OPS_PER_ACTOR_TICK);
        }
        assert_eq!(
            select(&mut runtime, &catalog, &owner, &rig, 0),
            (None, 0, 0)
        );
    }
}

#[test]
fn mixed_warm_and_cold_candidates_charge_both_validation_and_execution() {
    let owner = crate::tests::actor_with_metadata(HashMap::new());
    let mut rig = tests::owner_rig();
    rig.actor.runtime_id = owner.runtime_id;
    rig.actor.spawn_revision = owner.spawn_revision;
    let (assets, catalog) = candidates(32, 64, false);
    let mut runtime = AttachablesRuntime::new(assets);
    let warm = RuntimeEquipmentCatalog::from_parts(
        [1; 32],
        catalog
            .bindings()
            .iter()
            .enumerate()
            .filter(|(index, binding)| binding.attachable.is_none() || index % 2 == 0)
            .map(|(_, binding)| binding.clone())
            .collect(),
        Vec::new(),
    )
    .unwrap();
    assert_eq!(
        select(
            &mut runtime,
            &warm,
            &owner,
            &rig,
            MAX_MOLANG_OPS_PER_ACTOR_TICK
        )
        .0,
        None
    );
    let (selected, queries, spent) = select(
        &mut runtime,
        &catalog,
        &owner,
        &rig,
        MAX_MOLANG_OPS_PER_ACTOR_TICK,
    );
    assert_eq!(selected, None);
    assert_eq!(queries, 32 * 64);
    assert!(spent > queries && spent < MAX_MOLANG_OPS_PER_ACTOR_TICK);
}

#[test]
fn exhausted_validation_does_not_reuse_or_poison_a_changed_owner_result() {
    let mut owner = crate::tests::actor_with_metadata(HashMap::new());
    let mut rig = tests::owner_rig();
    rig.actor.runtime_id = owner.runtime_id;
    rig.actor.spawn_revision = owner.spawn_revision;
    let (assets, catalog) = candidates(1, 50, true);
    let mut runtime = AttachablesRuntime::new(assets);
    let cold = select(
        &mut runtime,
        &catalog,
        &owner,
        &rig,
        MAX_MOLANG_OPS_PER_ACTOR_TICK,
    );
    let warm = select(
        &mut runtime,
        &catalog,
        &owner,
        &rig,
        MAX_MOLANG_OPS_PER_ACTOR_TICK,
    );
    assert_eq!(cold.0, None);
    assert_eq!(warm.0, None);
    assert_eq!(cold.1, warm.1);
    assert!(
        warm.2 < cold.2,
        "validation reuses the cached branch without executing its bytecode"
    );
    for limit in [0, 1, 25, 50] {
        assert_eq!(select(&mut runtime, &catalog, &owner, &rig, limit).0, None);
    }
    owner.metadata.insert(
        protocol_ids::ActorDataKey::Flags.wire_value(),
        ActorMetadataValue::Flags(1 << protocol_ids::ActorFlag::Sneaking.wire_value()),
    );
    assert_eq!(select(&mut runtime, &catalog, &owner, &rig, 1).0, None);
    let mapped = catalog
        .bindings()
        .iter()
        .rev()
        .find(|binding| binding.attachable.is_some())
        .unwrap()
        .attachable
        .clone();
    assert_eq!(
        select(
            &mut runtime,
            &catalog,
            &owner,
            &rig,
            MAX_MOLANG_OPS_PER_ACTOR_TICK
        )
        .0,
        mapped
    );
    owner.runtime_id += 1;
    owner.metadata.clear();
    rig.actor.runtime_id = owner.runtime_id;
    assert_eq!(
        select(
            &mut runtime,
            &catalog,
            &owner,
            &rig,
            MAX_MOLANG_OPS_PER_ACTOR_TICK
        )
        .0,
        None
    );
    runtime.clear();
    assert_eq!(
        select(&mut runtime, &catalog, &owner, &rig, 0),
        (None, 0, 0)
    );
}
