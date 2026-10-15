use super::*;

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
        .send(Finished {
            column,
            generation,
            summary: far_terrain::ColumnSummary::default().encode(),
        })
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
        .send(Finished {
            column,
            generation,
            summary: far_terrain::ColumnSummary::default().encode(),
        })
        .unwrap();
    inbox.drain_budgeted(&mut out, &mut SummaryBudget::new(1, usize::MAX));
    assert!(out.is_empty());
    assert!(!inbox.is_idle());
    inbox.drain(&mut out);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].generation, generation);
    assert!(inbox.is_idle());
}
