use super::*;

#[test]
fn mutation_completion_revalidates_the_frozen_raw_publisher_cohort() {
    let coordinate = [14, 71, -6];
    let key = SubChunkKey::new(0, 0, 4, -1);
    let mut acceptance = AcceptanceRun::new(Some(900), None, false, false);
    acceptance.set_mutation_coordinate(coordinate);
    let observed = Instant::now() + Duration::from_millis(1);
    let frozen = exact_destination_status();
    assert!(acceptance.bind_mutation_cohort(frozen));
    acceptance.observe_mutation(
        &WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
            dimension: 0,
            position: coordinate,
            layer: 0,
            network_id: 7,
        }]),
        observed,
    );

    let mut changed = frozen;
    changed.publisher_epoch += 1;
    assert_eq!(
        acceptance.acknowledge_mutation(key, 1, observed, observed, Some(changed)),
        None
    );

    changed = frozen;
    changed.required_hash ^= 1;
    assert_eq!(
        acceptance.acknowledge_mutation(key, 1, observed, observed, Some(changed)),
        None
    );
    assert_eq!(
        acceptance.acknowledge_mutation(key, 1, observed, observed, Some(frozen)),
        Some(Duration::ZERO)
    );
}

#[test]
fn post_world_ready_required_growth_revokes_the_emitted_cohort() {
    let mut acceptance = AcceptanceRun::new(Some(900), None, false, false);
    let frozen = exact_destination_status();
    assert!(acceptance.bind_mutation_cohort(frozen));
    acceptance.begin_world_ready(Instant::now(), [0.5, 70.0, 0.5], 1);
    assert!(acceptance.world_ready);

    let mut expanded = frozen;
    expanded.expected += 1;
    expanded.required_hash ^= 0x55aa;

    assert!(acceptance.revoke_world_ready_if_cohort_changed(Some(expanded)));
    assert!(!acceptance.world_ready);
    assert_eq!(acceptance.deadline, None);
    assert_eq!(acceptance.mutation_cohort, None);
}
