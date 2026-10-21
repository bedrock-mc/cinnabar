//! One search prepares owner slots once and restores only actual predicate mutations.

use super::*;
use crate::MolangVariables;
use crate::evaluation::{ActorAnimationVariables, VariableLayout, selection_work};
use assets::{MolangFunction, MolangOp, MolangSymbol, MolangSymbolKind};

/// Builds many distinct predicates over large inherited and temporary layouts, with optional writes.
fn fixture(
    count: usize,
    slots: usize,
    mutating: bool,
) -> (Arc<RuntimeEntityAssets>, RuntimeEquipmentCatalog) {
    let (mut compiled, catalog) = candidate_parts(count, 1, false);
    let mut symbols = compiled.molang_symbols.into_vec();
    let first_variable = symbols.len() as u32;
    for (kind, prefix) in [
        (MolangSymbolKind::Variable, "variable.padding"),
        (MolangSymbolKind::Temporary, "temp.padding"),
    ] {
        for index in 0..slots {
            symbols.push(MolangSymbol {
                kind,
                identifier: format!("{prefix}_{index:05}").into(),
            });
        }
    }
    let last_variable = first_variable + slots as u32 - 1;
    let last_temp = last_variable + slots as u32;
    let mut expression = vec![MolangOp::LoadVariable(first_variable), MolangOp::Pop];
    if mutating {
        expression.extend([
            MolangOp::LoadVariable(last_variable),
            MolangOp::LoadVariable(last_temp),
            MolangOp::Add,
            MolangOp::StoreVariable(last_temp),
            MolangOp::Push(crate::test_support::scalar(1.0)),
            MolangOp::StoreVariable(last_variable),
            MolangOp::Push(crate::test_support::scalar(2.0)),
            MolangOp::StoreVariable(last_variable),
            MolangOp::Push(crate::test_support::scalar(0.0)),
            MolangOp::Push(crate::test_support::scalar(1.0)),
            MolangOp::Call(MolangFunction::Random),
            MolangOp::Pop,
            MolangOp::LoadVariable(last_temp),
            MolangOp::LoadQuery(1),
            MolangOp::Add,
        ]);
    } else {
        expression.push(MolangOp::LoadQuery(1));
    }
    compiled.molang_symbols = symbols.into();
    compiled.molang_ops = (0..count)
        .flat_map(|_| expression.iter().copied())
        .collect();
    compiled.molang_expressions = (0..count)
        .map(|index| assets::CompiledMolangExpression {
            first_op: (index * expression.len()) as u32,
            op_count: expression.len() as u16,
            max_stack: if mutating { 2 } else { 1 },
        })
        .collect();
    (
        Arc::new(RuntimeEntityAssets::from_compiled(compiled).unwrap()),
        catalog,
    )
}

/// Measures real selection slot visits with borrowed owner values and a replaceable session clock.
fn measured(
    runtime: &mut AttachablesRuntime,
    assets: &RuntimeEntityAssets,
    catalog: &RuntimeEquipmentCatalog,
    owner: &dyn AnimatedActor,
    values: &MolangVariables,
    session: u64,
    limit: usize,
) -> (Option<Box<str>>, selection_work::Work, usize, usize) {
    let mut rig = tests::owner_rig();
    rig.actor.runtime_id = owner.runtime_id();
    rig.actor.spawn_revision = owner.spawn_revision();
    rig.actor.session_id = session;
    rig.animation_variables =
        ActorAnimationVariables::new(Some(assets), values, 100 + session, session);
    selection_work::take();
    let (selected, queries, spent) = select(runtime, catalog, owner, &rig, limit);
    (selected, selection_work::take(), queries, spent)
}

#[test]
fn large_conditional_search_bounds_actual_preparation_on_warm_and_dirty_owner_inputs() {
    let owner = crate::tests::actor_with_metadata(HashMap::new());
    for mutating in [false, true] {
        let (assets, catalog) = fixture(
            assets::MAX_EQUIPMENT_BINDINGS - 1,
            assets::MAX_MOLANG_EXPRESSIONS / 4,
            mutating,
        );
        let layout = VariableLayout::new(&assets);
        let mut values = layout.fresh(1);
        let variables = assets
            .molang_symbols()
            .iter()
            .filter(|symbol| symbol.kind == MolangSymbolKind::Variable)
            .count();
        let temps = assets
            .molang_symbols()
            .iter()
            .filter(|symbol| symbol.kind == MolangSymbolKind::Temporary)
            .count();
        for slot in 0..variables {
            values.set(Some(slot), 0.0);
        }
        let mut runtime = AttachablesRuntime::new(Arc::clone(&assets));
        for session in [1, 2] {
            for dirty in [false, true] {
                if dirty {
                    values.set_string(
                        layout.named_slot(&assets, "variable.padding_00000"),
                        "changed",
                    );
                }
                for limit in [
                    MAX_MOLANG_OPS_PER_ACTOR_TICK,
                    MAX_MOLANG_OPS_PER_ACTOR_TICK,
                    9,
                    0,
                    1,
                ] {
                    let (selected, work, queries, spent) = measured(
                        &mut runtime,
                        &assets,
                        &catalog,
                        &owner,
                        &values,
                        session,
                        limit,
                    );
                    assert_eq!(
                        selected, None,
                        "candidate mutations cannot admit a later mapping"
                    );
                    assert!(spent <= limit);
                    if limit == 0 {
                        assert_eq!(work, selection_work::Work::default());
                        assert_eq!((queries, spent), (0, 0));
                    } else {
                        assert_eq!(
                            work.reset,
                            variables + temps,
                            "only one full-layout preparation is allowed per search"
                        );
                        assert_eq!(
                            work.inherited, variables,
                            "inherited owner slots are visited once"
                        );
                        assert!(work.assigned <= variables * 2 + spent);
                        assert_eq!(
                            work.saved, work.restored,
                            "every mutation is restored even on budget exhaustion"
                        );
                        assert!(work.restored <= spent);
                    }
                }
            }
            runtime.clear();
        }
    }
}

#[test]
fn mutating_candidates_restore_typed_values_temps_and_rng_before_cache_validation() {
    let owner = crate::tests::actor_with_metadata(HashMap::new());
    let (assets, catalog) = fixture(8, 64, true);
    let layout = VariableLayout::new(&assets);
    let mut values = layout.fresh(1);
    let mut runtime = AttachablesRuntime::new(Arc::clone(&assets));
    let cold = measured(
        &mut runtime,
        &assets,
        &catalog,
        &owner,
        &values,
        1,
        MAX_MOLANG_OPS_PER_ACTOR_TICK,
    );
    let warm = measured(
        &mut runtime,
        &assets,
        &catalog,
        &owner,
        &values,
        1,
        MAX_MOLANG_OPS_PER_ACTOR_TICK,
    );
    assert_eq!((cold.0, warm.0), (None, None));
    assert!(cold.1.restored > 0);
    assert_eq!(
        warm.1.restored, 0,
        "every warm result must reuse the same seeded owner prefix"
    );
    assert!(warm.3 < cold.3);
    assert_eq!(cold.2, warm.2);
    values.set_string(
        layout.named_slot(&assets, "variable.padding_00000"),
        "dirty typed input",
    );
    let last_variable = assets
        .molang_symbols()
        .iter()
        .rev()
        .find(|symbol| symbol.kind == MolangSymbolKind::Variable)
        .unwrap();
    values.set_string(
        layout.named_slot(&assets, &last_variable.identifier),
        "typed mutation target",
    );
    let dirty = measured(
        &mut runtime,
        &assets,
        &catalog,
        &owner,
        &values,
        1,
        MAX_MOLANG_OPS_PER_ACTOR_TICK,
    );
    assert_eq!(dirty.0, None);
    assert!(
        dirty.1.restored > 0,
        "changed cached inputs must execute and restore predicates"
    );
    assert_eq!(dirty.1.saved, dirty.1.restored);
    let typed_warm = measured(
        &mut runtime,
        &assets,
        &catalog,
        &owner,
        &values,
        1,
        MAX_MOLANG_OPS_PER_ACTOR_TICK,
    );
    assert_eq!(typed_warm.0, None);
    assert_eq!(
        typed_warm.1.restored, 0,
        "restoration preserves the original assigned value type"
    );
    runtime.clear();
    let exhausted = measured(&mut runtime, &assets, &catalog, &owner, &values, 2, 9);
    assert_eq!(exhausted.0, None);
    assert!(
        exhausted.1.saved > 0,
        "the cold predicate writes before exhausting its budget"
    );
    assert_eq!(exhausted.1.saved, exhausted.1.restored);
    let after_clear = measured(
        &mut runtime,
        &assets,
        &catalog,
        &owner,
        &values,
        2,
        MAX_MOLANG_OPS_PER_ACTOR_TICK,
    );
    assert_eq!(after_clear.0, None);
    assert!(after_clear.1.restored > 0);
}

#[test]
fn standalone_selection_and_unresolved_or_unconditional_candidates_keep_their_preparation_rules() {
    let owner = crate::tests::actor_with_metadata(HashMap::new());
    let (assets, catalog) = fixture(2, 32, true);
    let layout = VariableLayout::new(&assets);
    let values = layout.fresh(1);
    let mut rig = tests::owner_rig();
    rig.animation_variables = ActorAnimationVariables::new(Some(&assets), &values, 100, 0);
    let mapped = catalog
        .bindings()
        .iter()
        .find_map(|binding| binding.attachable.as_deref())
        .unwrap();
    let mut runtime = AttachablesRuntime::new(Arc::clone(&assets));
    for _ in 0..2 {
        let mut left = MAX_MOLANG_OPS_PER_ACTOR_TICK;
        assert_eq!(
            runtime.matches_owner(mapped, &owner, &rig, Default::default(), &mut left),
            Some(false)
        );
    }
    for (name, budget, expected) in [
        (mapped, 0, Some(false)),
        ("test:missing", MAX_MOLANG_OPS_PER_ACTOR_TICK, None),
        ("minecraft:test_item", 0, Some(true)),
    ] {
        selection_work::take();
        let mut left = budget;
        assert_eq!(
            runtime.matches_owner(name, &owner, &rig, Default::default(), &mut left),
            expected
        );
        assert_eq!(selection_work::take(), selection_work::Work::default());
        assert_eq!(left, budget);
    }
}
