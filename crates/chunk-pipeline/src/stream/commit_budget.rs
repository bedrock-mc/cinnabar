use super::*;

/// Cooperative frame allocation, with one progress item per ready service lane.
pub(super) const WORLD_POLL_BUDGET: Duration = Duration::from_millis(3);
/// Reserves half of the frame allocation for light and mesh service.
pub(super) const WORLD_SCHEDULING_SHARE: u32 = 2;
/// Lighting gates geometry; reserve a third of the remaining service time for meshing.
pub(super) const WORLD_MESH_SHARE: u32 = 3;

impl WorldStream {
    /// Starts the frame's shared ingress, commit and scheduling allocation.
    pub fn begin_frame_work(&mut self) {
        let now = Instant::now();
        self.frame_deadline = Some(now + WORLD_POLL_BUDGET);
        self.poll_deadline =
            Some(now + WORLD_POLL_BUDGET - WORLD_POLL_BUDGET / WORLD_SCHEDULING_SHARE);
    }

    /// Reports whether normal work has spent this poll's shared allocation.
    pub(super) fn poll_budget_exhausted(&self) -> bool {
        self.poll_deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
    }

    /// Commits FIFO work without crossing a partially published batch or mutation fence.
    pub(super) fn apply_ready(&mut self) {
        if self.order.blocking_block_updates().is_some() {
            return;
        }
        let deadline = self
            .poll_deadline
            .unwrap_or_else(|| Instant::now() + WORLD_POLL_BUDGET);
        let mut progressed = self.poll_deadline.is_some() && !self.polling;
        while !progressed || Instant::now() < deadline {
            let Some(step) = self.order.next_commit() else {
                break;
            };
            match step {
                CommitStep::BatchStarted => continue,
                CommitStep::Apply { sequence, event } => {
                    self.apply_prepared_with_sequence(event, Some(sequence));
                    self.finish_ordered_commit(sequence);
                }
                CommitStep::SyncedBlockUpdates { sequence, events } => {
                    let (batches, events) = self.snapshot_synced_block_mutation_batches(events);
                    if batches.is_empty() {
                        self.finish_ordered_commit(sequence);
                    } else {
                        let ids = self.decode_ids(self.authority.current_dimension());
                        self.predictions.begin_server_batch();
                        self.enqueue_decode_job(DecodeJob::SyncedBlockUpdates {
                            sequence,
                            batches,
                            events,
                            ids,
                        });
                        self.order.defer_block_updates(sequence);
                        break;
                    }
                }
                CommitStep::BlockUpdates { sequence, events } => {
                    let batches = self.snapshot_block_mutation_batches(events);
                    if batches.is_empty() {
                        self.finish_ordered_commit(sequence);
                    } else {
                        let ids = self.decode_ids(self.authority.current_dimension());
                        self.predictions.begin_server_batch();
                        self.enqueue_decode_job(DecodeJob::BlockUpdates {
                            sequence,
                            batches,
                            ids,
                        });
                        self.order.defer_block_updates(sequence);
                        break;
                    }
                }
            }
            progressed = true;
        }
    }

    /// Releases a request reservation only when the lower owner finishes the whole event.
    fn finish_ordered_commit(&mut self, sequence: u64) {
        if self.order.finish_commit(sequence) {
            self.cancel_request_reservation(sequence);
        }
    }
}
