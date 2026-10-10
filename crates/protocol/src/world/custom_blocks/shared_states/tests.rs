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
    let owned: Vec<_> = next.into_iter().collect();
    assert!(Arc::ptr_eq(&first[1].values, &owned[1].values));
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
