//! Owner predicate caching preserves input changes and a bounded selection budget.
use super::*;

#[test]
fn worn_selection_uses_the_owner_animation_epoch_and_refreshes_cached_time() {
    let mut compiled = owner_reference_tests::compiled_fixture(true);
    compiled.rig_bindings[0].item_condition = Some(0);
    compiled.molang_symbols[1].identifier = "query.anim_time".into();
    compiled.molang_ops = [
        assets::MolangOp::LoadQuery(1),
        assets::MolangOp::Push(crate::test_support::scalar(1.0)),
        assets::MolangOp::Less,
    ]
    .into();
    compiled.molang_expressions[0].op_count = 3;
    let mut runtime = AttachablesRuntime::new(Arc::new(
        RuntimeEntityAssets::from_compiled(compiled).unwrap(),
    ));
    let owner = crate::tests::actor_with_metadata(HashMap::new());
    let variables = crate::MolangVariables::default();
    let mut rig = tests::owner_rig();
    for (completed, lifetime, animation, alpha, expected) in [
        (1000, 0, 0, 0.0, true),
        (1005, 5, 5, 0.0, true),
        (1019, 19, 19, 0.5, true),
        (1020, 20, 20, 0.0, false),
        (2000, 1000, 0, 0.0, true),
        (2020, 1020, 20, 0.0, false),
    ] {
        rig.completed_tick = completed;
        rig.animation_variables =
            ActorAnimationVariables::new(None, &variables, lifetime, animation);
        let input = AttachableAnimationInput {
            worn: true,
            frame_alpha: alpha,
            ..Default::default()
        };
        let mut budget = MAX_MOLANG_OPS_PER_ACTOR_TICK;
        assert_eq!(
            runtime.matches_owner("minecraft:test_item", &owner, &rig, input, &mut budget),
            Some(expected),
            "completed {completed}, elapsed animation {animation}"
        );
        let remaining = budget;
        rig.completed_tick += 1;
        assert_eq!(
            runtime.matches_owner("minecraft:test_item", &owner, &rig, input, &mut budget),
            Some(expected)
        );
        assert!(
            budget < remaining,
            "cached relative clocks charge read validation"
        );
    }
}

/// Makes the owner-reference fixture's expression select the attachable itself.
fn selection_fixture() -> Arc<RuntimeEntityAssets> {
    let mut compiled = owner_reference_tests::compiled_fixture(true);
    compiled.rig_bindings[0].item_condition = Some(0);
    Arc::new(RuntimeEntityAssets::from_compiled(compiled).unwrap())
}

#[test]
fn owner_selection_reuses_unchanged_inputs_and_refreshes_changed_owner_queries() {
    let mut owner = crate::tests::actor_with_metadata(HashMap::new());
    let mut runtime = AttachablesRuntime::new(selection_fixture());
    let rig = tests::owner_rig();
    let input = AttachableAnimationInput {
        first_person: true,
        ..Default::default()
    };
    let mut budget = MAX_MOLANG_OPS_PER_ACTOR_TICK;
    assert_eq!(
        runtime.matches_owner("minecraft:test_item", &owner, &rig, input, &mut budget),
        Some(false)
    );
    assert!(budget < MAX_MOLANG_OPS_PER_ACTOR_TICK);
    let remaining = budget;
    assert_eq!(
        runtime.matches_owner("minecraft:test_item", &owner, &rig, input, &mut budget),
        Some(false)
    );
    assert!(
        budget < remaining,
        "unchanged owner inputs still charge validation"
    );
    owner.metadata.insert(0, ActorMetadataValue::Flags(1 << 40));
    assert_eq!(
        runtime.matches_owner("minecraft:test_item", &owner, &rig, input, &mut budget),
        Some(true)
    );
    assert!(budget < remaining);
    let remaining = budget;
    assert_eq!(
        runtime.matches_owner("minecraft:test_item", &owner, &rig, input, &mut budget),
        Some(true)
    );
    assert!(budget < remaining, "cache reuse charges its observed reads");
    let input = AttachableAnimationInput {
        first_person: false,
        ..input
    };
    assert_eq!(
        runtime.matches_owner("minecraft:test_item", &owner, &rig, input, &mut budget),
        Some(false)
    );
}

#[test]
fn owner_predicates_share_the_callers_operation_limit() {
    let owner = crate::tests::actor_with_metadata(HashMap::new());
    let mut runtime = AttachablesRuntime::new(selection_fixture());
    let rig = tests::owner_rig();
    let input = AttachableAnimationInput {
        first_person: true,
        ..Default::default()
    };
    let mut budget = 1;
    assert_eq!(
        runtime.matches_owner("minecraft:test_item", &owner, &rig, input, &mut budget),
        Some(false)
    );
    assert_eq!(budget, 0);
    assert_eq!(
        runtime.matches_owner("minecraft:test_item", &owner, &rig, input, &mut budget),
        Some(false)
    );
    assert_eq!(budget, 0);
}

#[test]
fn owner_selection_seeds_the_owning_entity_reference() {
    let owner =
        crate::tests::actor_with_metadata(HashMap::from([(0, ActorMetadataValue::Flags(1 << 40))]));
    let mut runtime = AttachablesRuntime::new(selection_fixture());
    let mut budget = MAX_MOLANG_OPS_PER_ACTOR_TICK;
    assert_eq!(
        runtime.matches_owner(
            "minecraft:test_item",
            &owner,
            &tests::owner_rig(),
            AttachableAnimationInput {
                first_person: true,
                ..Default::default()
            },
            &mut budget
        ),
        Some(true)
    );
}

#[test]
fn owner_selection_does_not_cache_budget_exhaustion_as_a_rejection() {
    let owner =
        crate::tests::actor_with_metadata(HashMap::from([(0, ActorMetadataValue::Flags(1 << 40))]));
    let mut runtime = AttachablesRuntime::new(selection_fixture());
    let rig = tests::owner_rig();
    let input = AttachableAnimationInput {
        first_person: true,
        ..Default::default()
    };
    let mut budget = 1;
    assert_eq!(
        runtime.matches_owner("minecraft:test_item", &owner, &rig, input, &mut budget),
        Some(false)
    );
    budget = MAX_MOLANG_OPS_PER_ACTOR_TICK;
    assert_eq!(
        runtime.matches_owner("minecraft:test_item", &owner, &rig, input, &mut budget),
        Some(true)
    );
    runtime.clear();
    budget = 0;
    assert_eq!(
        runtime.matches_owner("minecraft:test_item", &owner, &rig, input, &mut budget),
        Some(false)
    );
}

#[test]
fn cached_owner_predicates_observe_frame_inputs_only_when_read() {
    let mut compiled = owner_reference_tests::compiled_fixture(true);
    compiled.rig_bindings[0].item_condition = Some(0);
    compiled.molang_symbols[1].identifier = "query.frame_alpha".into();
    let mut runtime = AttachablesRuntime::new(Arc::new(
        RuntimeEntityAssets::from_compiled(compiled).unwrap(),
    ));
    let owner = crate::tests::actor_with_metadata(HashMap::new());
    let rig = tests::owner_rig();
    let mut budget = MAX_MOLANG_OPS_PER_ACTOR_TICK;
    for (frame_alpha, expected) in [(0.0, false), (0.5, true), (0.5, true), (0.0, false)] {
        assert_eq!(
            runtime.matches_owner(
                "minecraft:test_item",
                &owner,
                &rig,
                AttachableAnimationInput {
                    frame_alpha,
                    first_person: true,
                    ..Default::default()
                },
                &mut budget
            ),
            Some(expected)
        );
    }
}

#[test]
fn cached_owner_predicates_refresh_queries_with_arguments() {
    let mut compiled = owner_reference_tests::compiled_fixture(true);
    compiled.rig_bindings[0].item_condition = Some(0);
    compiled.molang_symbols[1].identifier = "query.item_remaining_use_duration".into();
    let mut symbols = compiled.molang_symbols.into_vec();
    symbols.push(assets::MolangSymbol {
        kind: assets::MolangSymbolKind::String,
        identifier: "main_hand".into(),
    });
    compiled.molang_symbols = symbols.into();
    compiled.molang_ops = [
        assets::MolangOp::PushString(4),
        assets::MolangOp::Push(crate::test_support::scalar(1.0)),
        assets::MolangOp::CallQuery(assets::MolangCall {
            symbol: 1,
            arguments: 2,
        }),
        assets::MolangOp::Push(crate::test_support::scalar(0.5)),
        assets::MolangOp::Greater,
    ]
    .into();
    compiled.molang_expressions[0].max_stack = 2;
    let mut runtime = AttachablesRuntime::new(Arc::new(
        RuntimeEntityAssets::from_compiled(compiled).unwrap(),
    ));
    let owner = crate::tests::actor_with_metadata(HashMap::new());
    let rig = tests::owner_rig();
    for (elapsed, expected) in [(5, true), (15, false), (5, true)] {
        let input = AttachableAnimationInput {
            use_elapsed_ticks: Some(elapsed),
            max_use_ticks: 20,
            ..Default::default()
        };
        let mut budget = MAX_MOLANG_OPS_PER_ACTOR_TICK;
        assert_eq!(
            runtime.matches_owner("minecraft:test_item", &owner, &rig, input, &mut budget),
            Some(expected)
        );
        let remaining = budget;
        assert_eq!(
            runtime.matches_owner("minecraft:test_item", &owner, &rig, input, &mut budget),
            Some(expected)
        );
        assert!(budget < remaining, "cache reuse charges its observed reads");
    }
}

#[test]
fn cached_owner_predicates_do_not_treat_scratch_writes_as_inherited_inputs() {
    let mut compiled = owner_reference_tests::compiled_fixture(true);
    compiled.rig_bindings[0].item_condition = Some(0);
    compiled.molang_ops = [
        assets::MolangOp::LoadVariable(2),
        assets::MolangOp::StoreVariable(3),
        assets::MolangOp::LoadVariable(3),
    ]
    .into();
    compiled.molang_expressions[0].op_count = 3;
    compiled.molang_expressions[0].max_stack = 1;
    let mut runtime = AttachablesRuntime::new(Arc::new(
        RuntimeEntityAssets::from_compiled(compiled).unwrap(),
    ));
    let owner = crate::tests::actor_with_metadata(HashMap::new());
    let rig = tests::owner_rig();
    let input = AttachableAnimationInput {
        first_person: true,
        ..Default::default()
    };
    let mut budget = MAX_MOLANG_OPS_PER_ACTOR_TICK;
    assert_eq!(
        runtime.matches_owner("minecraft:test_item", &owner, &rig, input, &mut budget),
        Some(true)
    );
    let remaining = budget;
    assert_eq!(
        runtime.matches_owner("minecraft:test_item", &owner, &rig, input, &mut budget),
        Some(true)
    );
    assert!(budget < remaining, "cache reuse charges its observed reads");
}

#[path = "selection_budget_tests.rs"]
mod budget;
