use super::super::{CustomSelection, CustomStateAxis, Nbt};
use super::*;

/// Creates a two-state definition without network or pack fixtures.
fn block() -> CustomBlock {
    CustomBlock {
        name: "test:block".into(),
        tags: Arc::default(),
        state_count: 2,
        collides: true,
        collision_boxes: None,
        selection: CustomSelection::Default,
        state_physics: Arc::default(),
        visual: Arc::new(CustomBlockVisuals {
            state_axes: Box::new([CustomStateAxis {
                name: "test:active".into(),
                values: Box::new([CustomStateValue::Bool(false), CustomStateValue::Bool(true)]),
            }]),
            ..Default::default()
        }),
    }
}

#[test]
fn consumers_and_cloned_definitions_share_canonical_records_and_values() {
    let block = block();
    let first = block.hashed_states();
    let next = block.clone().hashed_states();
    assert_eq!(first.as_ptr(), next.as_ptr());
    assert!(Arc::ptr_eq(
        &first[1].values,
        &block.state_values(1).unwrap()
    ));
    let mut owned = next.into_iter();
    assert_eq!(owned.len(), first.len());
    let head = owned.next().unwrap();
    assert!(Arc::ptr_eq(&first[0].values, &head.values));
    assert_eq!(owned.len(), first.len() - 1);
    let tail: Vec<_> = owned.collect();
    assert!(Arc::ptr_eq(&first[1].values, &tail[0].values));
    for state in first.iter() {
        assert_eq!(
            state.hash,
            super::super::block_state_network_hash(
                &block.name,
                block
                    .visual
                    .state_axes
                    .iter()
                    .map(|axis| axis.name.as_ref())
                    .zip(state.values.iter())
            )
        );
    }
}

#[test]
fn changing_an_immutable_definition_detaches_and_recompiles_its_identities() {
    let mut block = block();
    let old = block.hashed_states();
    Arc::make_mut(&mut block.visual).state_axes[0].values[1] = CustomStateValue::Int(7);
    let changed = block.hashed_states();
    assert_ne!(old.as_ptr(), changed.as_ptr());
    assert_ne!(old[1].hash, changed[1].hash);
    assert_eq!(old[1].values[0], CustomStateValue::Bool(true));
    block.name = "test:renamed".into();
    assert_ne!(changed[1].hash, block.hashed_states()[1].hash);
}

#[test]
fn aggregate_state_and_byte_budgets_reject_before_expansion_without_spending_credit() {
    let mut definition = super::super::parse_definition(&Nbt::Compound(Vec::new())).unwrap();
    let mut budget = AdmissionBudget::default();
    definition.state_count = MAX_AGGREGATE_STATES as u32;
    assert!(budget.admit("test:block", &definition.visual, definition.state_count));
    definition.state_count = 1;
    assert!(!budget.admit("test:overflow", &definition.visual, definition.state_count));
    assert_eq!(budget.states, MAX_AGGREGATE_STATES);
    let mut budget = AdmissionBudget::default();
    let oversized_name = "n".repeat(MAX_AGGREGATE_STATE_BYTES);
    assert!(!budget.admit(&oversized_name, &definition.visual, definition.state_count));
    assert_eq!((budget.states, budget.bytes), (0, 0));
    assert!(budget.admit("test:small", &definition.visual, definition.state_count));
}

#[test]
fn compiled_state_consumers_allocate_nothing() {
    let block = block();
    let expected = block.hashed_states();
    let (fresh, before) = crate::test_allocations::measure(|| block.compile_states());
    let ((shared, values), after) = crate::test_allocations::measure(|| {
        (block.hashed_states(), block.state_values(1).unwrap())
    });
    assert_eq!(fresh.len(), shared.len());
    assert_eq!(expected.as_ptr(), shared.as_ptr());
    assert!(Arc::ptr_eq(&shared[1].values, &values));
    assert_eq!(after, 0);
    println!("two-state identity allocations: compile={before}, shared consumers={after}");
}

#[test]
fn unsupported_definitions_share_an_empty_result_without_cache_admission() {
    let mut first = block();
    Arc::make_mut(&mut first.visual).state_identity_incomplete = true;
    let mut next = block();
    Arc::make_mut(&mut next.visual).state_identity_incomplete = true;
    let _ = SharedStates::empty();
    let ((first, next), allocations) =
        crate::test_allocations::measure(|| (first.hashed_states(), next.hashed_states()));
    assert!(first.is_empty() && next.is_empty());
    assert_eq!(allocations, 0);
}

#[test]
fn reconnect_refills_a_full_cache_with_linear_eviction_work() {
    let mut cache = Cache::default();
    for _ in 0..MAX_CACHED_DEFINITIONS {
        let mut definition = block();
        definition.state_count = 1;
        definition.visual = Arc::default();
        assert_eq!(cache.resolve(&definition).len(), 1);
    }
    assert_eq!(cache.entries.len(), MAX_CACHED_DEFINITIONS);
    assert_eq!(cache.eviction_visits, 0);
    let retained = cache.entries.values().next().unwrap().states.clone();
    for _ in 0..MAX_CACHED_DEFINITIONS {
        let mut definition = block();
        definition.state_count = 1;
        definition.visual = Arc::default();
        let states = cache.resolve(&definition);
        assert_eq!(states.len(), 1);
        assert_eq!(states[0].hash, retained[0].hash);
    }
    assert_eq!(cache.eviction_visits, MAX_CACHED_DEFINITIONS);
    assert_eq!(cache.entries.len(), MAX_CACHED_DEFINITIONS);
    assert_eq!(cache.eviction_order.len(), MAX_CACHED_DEFINITIONS);
    assert_eq!(cache.states, MAX_AGGREGATE_STATES);
    assert!(cache.bytes <= MAX_AGGREGATE_STATE_BYTES);
    assert_eq!(retained.len(), 1);
}
