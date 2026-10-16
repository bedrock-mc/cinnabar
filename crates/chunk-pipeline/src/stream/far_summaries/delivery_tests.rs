use super::*;

/// A committed transition disconnects deferred and late output, even after a round trip.
#[test]
fn dimension_round_trip_disconnects_deferred_and_late_summaries() {
    let mut stream = WorldStream::new(protocol::WorldBootstrap {
        local_player_unique_id: 1,
        local_player_runtime_id: 1,
        dimension: 0,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    });
    stream.set_far_terrain_enabled(true);
    let inbox = &mut stream.far_terrain.inbox;
    let column = finish(inbox, 0);
    let late = inbox.tx.clone();
    let generation = inbox.issue(column);
    let mut out = Vec::new();
    inbox.drain_budgeted(&mut out, &mut SummaryBudget::new(0, 0));
    for (sequence, dimension) in [(1, 1), (2, 0)] {
        stream
            .submit(
                sequence,
                protocol::WorldEvent::ChangeDimension(protocol::ChangeDimensionEvent {
                    dimension,
                    ..Default::default()
                }),
            )
            .unwrap();
    }
    assert!(
        late.send(Event::Finished(Finished {
            column,
            generation,
            summary: far_terrain::ColumnSummary::default().encode(),
        }))
        .is_err(),
        "a worker still delivers into the departed world"
    );
    stream.drain_far_summaries(&mut out);
    assert!(out.is_empty());
    finish(&mut stream.far_terrain.inbox, column.x);
    stream.drain_far_summaries(&mut out);
    assert_eq!(out.len(), 1);
    assert!(out[0].generation > generation);
}

/// Completes one issued column without a worker or a clock dependency.
fn finish(inbox: &mut FarSummaryInbox, x: i32) -> ChunkKey {
    let column = ChunkKey {
        x,
        z: 0,
        dimension: 0,
    };
    let generation = inbox.issue(column);
    inbox
        .tx
        .send(Event::Finished(Finished {
            column,
            generation,
            summary: far_terrain::ColumnSummary::default().encode(),
        }))
        .unwrap();
    column
}

/// Limits delivery by bytes and columns without acknowledging deferred results.
#[test]
fn deferred_summaries_stay_in_flight_until_delivered() {
    let mut inbox = FarSummaryInbox::default();
    finish(&mut inbox, 0);
    let deferred = finish(&mut inbox, 1);
    let bytes = far_terrain::ColumnSummary::default().encode().bytes.len();
    let mut out = Vec::new();
    inbox.drain_budgeted(&mut out, &mut SummaryBudget::new(10, bytes));
    assert_eq!(out.len(), 1);
    assert!(inbox.has_finished());
    assert!(!inbox.is_idle());
    assert!(inbox.issued.contains_key(&deferred));
    inbox.drain_budgeted(&mut out, &mut SummaryBudget::new(1, usize::MAX));
    assert_eq!(out.len(), 2);
    assert!(inbox.is_idle());
    assert!(!inbox.has_finished());
}

/// A newer issue invalidates a result deferred on an earlier frame; stale work is charged too.
#[test]
fn deferred_results_still_obey_generation_order() {
    let mut inbox = FarSummaryInbox::default();
    let column = finish(&mut inbox, 0);
    let mut out = Vec::new();
    inbox.drain_budgeted(&mut out, &mut SummaryBudget::new(0, 0));
    let generation = inbox.issue(column);
    inbox
        .tx
        .send(Event::Finished(Finished {
            column,
            generation,
            summary: far_terrain::ColumnSummary::default().encode(),
        }))
        .unwrap();
    inbox.drain_budgeted(&mut out, &mut SummaryBudget::new(1, usize::MAX));
    assert!(out.is_empty());
    assert!(!inbox.is_idle());
    inbox.drain(&mut out);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].generation, generation);
    assert!(inbox.is_idle());
}

#[test]
fn cancelling_a_resource_generation_discards_deferred_and_late_results() {
    let mut inbox = FarSummaryInbox::default();
    let column = finish(&mut inbox, 0);
    let late = inbox.tx.clone();
    let generation = inbox.issue(column);
    let mut out = Vec::new();
    inbox.drain_budgeted(&mut out, &mut SummaryBudget::new(0, 0));
    inbox.cancel_pending();
    assert!(
        late.send(Event::Finished(Finished {
            column,
            generation,
            summary: far_terrain::ColumnSummary::default().encode(),
        }))
        .is_err()
    );
    inbox.drain(&mut out);
    assert!(out.is_empty());
    assert!(inbox.is_idle());
    finish(&mut inbox, 0);
    inbox.drain(&mut out);
    assert_eq!(out.len(), 1);
    assert!(out[0].generation > generation);
}
