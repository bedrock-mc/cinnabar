use super::*;

/// Cooperative frame allocation, with one progress item per ready service lane.
pub(super) const WORLD_POLL_BUDGET: Duration = Duration::from_millis(3);
/// High refresh rates get a smaller allocation so a streaming backlog cannot halve them.
const WORLD_POLL_BUDGET_FLOOR: Duration = Duration::from_millis(1);
/// The allocation never exceeds this fraction of the display interval, down to the floor.
const WORLD_POLL_INTERVAL_SHARE: u32 = 2;
/// Reserves half of the frame allocation for light and mesh service.
pub(super) const WORLD_SCHEDULING_SHARE: u32 = 2;
/// Lighting gates geometry; reserve a third of the remaining service time for meshing.
pub(super) const WORLD_MESH_SHARE: u32 = 3;

impl WorldStream {
    /// Sizes the per-frame allocation to the display interval; zero keeps the default.
    pub fn set_display_interval(&mut self, interval: Duration) {
        self.poll_budget = if interval.is_zero() {
            WORLD_POLL_BUDGET
        } else {
            (interval / WORLD_POLL_INTERVAL_SHARE)
                .clamp(WORLD_POLL_BUDGET_FLOOR, WORLD_POLL_BUDGET)
        };
    }

    /// Starts the frame's shared ingress, commit and scheduling allocation.
    pub fn begin_frame_work(&mut self) {
        let now = Instant::now();
        self.frame_deadline = Some(now + self.poll_budget);
        self.poll_deadline =
            Some(now + self.poll_budget - self.poll_budget / WORLD_SCHEDULING_SHARE);
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
            .unwrap_or_else(|| Instant::now() + self.poll_budget);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Fast displays get half their interval, slow ones keep the cap, and nothing drops
    /// below the floor that keeps streaming progressing.
    #[test]
    fn frame_allocation_follows_the_display_interval_within_bounds() {
        let mut stream = WorldStream::new(WorldBootstrap {
            local_player_unique_id: 1,
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        });
        let hz = |rate: u64| Duration::from_nanos(1_000_000_000 / rate);
        for (interval, expected) in [
            (hz(240), hz(240) / 2),
            (hz(60), WORLD_POLL_BUDGET),
            (hz(1000), WORLD_POLL_BUDGET_FLOOR),
            (Duration::ZERO, WORLD_POLL_BUDGET),
        ] {
            stream.set_display_interval(interval);
            let before = Instant::now();
            stream.begin_frame_work();
            let after = Instant::now();
            let deadline = stream.frame_deadline.expect("frame work started");
            assert!(deadline >= before + expected && deadline <= after + expected);
        }
    }
}
