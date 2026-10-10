use super::*;

/// Cooperative frame allocation, with one progress item per ready service lane.
pub(super) const WORLD_POLL_BUDGET: Duration = Duration::from_millis(3);
/// High refresh rates get a smaller allocation so a streaming backlog cannot halve them.
pub(super) const WORLD_POLL_BUDGET_FLOOR: Duration = Duration::from_millis(1);
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
            (interval / WORLD_POLL_INTERVAL_SHARE).clamp(WORLD_POLL_BUDGET_FLOOR, WORLD_POLL_BUDGET)
        };
    }

    /// Starts the frame's shared ingress, commit and scheduling allocation.
    pub fn begin_frame_work(&mut self) {
        let now = Instant::now();
        self.frame_deadline = Some(now + self.poll_budget);
        self.poll_deadline =
            Some(now + self.poll_budget - self.poll_budget / WORLD_SCHEDULING_SHARE);
    }

    /// Starts a poll under the remaining frame allocation and reports its final deadline.
    pub(super) fn begin_poll_work(&mut self, now: Instant) -> Instant {
        let mut frame_deadline = self
            .frame_deadline
            .take()
            .unwrap_or_else(|| self.poll_deadline.unwrap_or(now + self.poll_budget));
        // A service that held the stream for a whole allocation between frames carries the
        // backlog: the frame keeps the floor allocation and leaves chunk-data commits to it.
        let offloaded = self.between_frames_service
            && self.service_yield.is_none()
            && self.service_window >= self.poll_budget;
        if offloaded {
            frame_deadline = frame_deadline.min(now + WORLD_POLL_BUDGET_FLOOR);
        }
        let remaining = frame_deadline.saturating_duration_since(now);
        let commit_deadline = now + remaining - remaining / WORLD_SCHEDULING_SHARE;
        self.poll_deadline = Some(
            self.poll_deadline
                .map_or(commit_deadline, |earlier| earlier.min(commit_deadline)),
        );
        self.polling = true;
        self.poll_heavy_guarantee = true;
        self.chunk_data_offloaded = offloaded;
        frame_deadline
    }

    /// Reports whether normal work has spent this poll's shared allocation, or a
    /// between-frames service has been asked to hand the stream back.
    pub(super) fn poll_budget_exhausted(&self) -> bool {
        self.service_yield_requested()
            || self
                .poll_deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
    }

    /// Whether the frame is reclaiming the stream from its between-frames service.
    pub(super) fn service_yield_requested(&self) -> bool {
        self.service_yield
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Acquire))
    }

    /// Commits every unblocked event. Heavy terrain steps stop at the poll deadline, with one
    /// guaranteed per poll across every pass it runs; light steps never wait for, or spend,
    /// the terrain allocation. A deferred retention change evicts first, as it preceded
    /// every commit still to come.
    pub(super) fn apply_ready(&mut self) {
        self.apply_due_chunk_retention();
        #[cfg(feature = "tracy")]
        let _zone = tracing::info_span!("stream.commit_ready").entered();
        let deadline = self
            .poll_deadline
            .unwrap_or_else(|| Instant::now() + self.poll_budget);
        let mut heavy_guaranteed =
            self.poll_deadline.is_none() || (self.polling && self.poll_heavy_guarantee);
        loop {
            // Without local physics the server position scopes retention; a committed
            // teleport, respawn or dimension change can hand it back mid-pass. An offloaded
            // frame poll leaves heavy chunk data to the service unless a ready mutation,
            // retention change or barrier waits behind it, which local authority observes.
            let budget = CommitBudget {
                heavy: heavy_guaranteed
                    || (Instant::now() < deadline && !self.service_yield_requested()),
                chunk_data: !self.chunk_data_offloaded,
                couple_position: self.local_player_chunk.is_none(),
            };
            let Some(step) = self.order.next_commit_within(budget) else {
                break;
            };
            self.commit_steps = self.commit_steps.wrapping_add(1);
            if self.order.last_step_was_heavy() {
                heavy_guaranteed = false;
                self.poll_heavy_guarantee = false;
            }
            match step {
                CommitStep::BatchStarted => {}
                CommitStep::Apply { sequence, event } => {
                    self.apply_prepared_with_sequence(event, Some(sequence));
                    self.finish_ordered_commit(sequence);
                }
                CommitStep::SyncedBlockUpdates { sequence, events } => {
                    let (batches, events) = self.snapshot_synced_block_mutation_batches(events);
                    if batches.is_empty() {
                        self.finish_ordered_commit(sequence);
                    } else if inline_block_batches(&batches) {
                        let ids = self.decode_ids(self.authority.current_dimension());
                        self.apply_block_mutations_inline(DecodeJob::SyncedBlockUpdates {
                            sequence,
                            batches,
                            events,
                            ids,
                        });
                    } else {
                        let ids = self.decode_ids(self.authority.current_dimension());
                        self.predictions
                            .begin_server_batch(batches.iter().map(|batch| batch.key));
                        self.enqueue_decode_job(DecodeJob::SyncedBlockUpdates {
                            sequence,
                            batches,
                            events,
                            ids,
                        });
                        self.order.defer_block_updates(sequence);
                    }
                }
                CommitStep::BlockUpdates { sequence, events } => {
                    let batches = self.snapshot_block_mutation_batches(events);
                    if batches.is_empty() {
                        self.finish_ordered_commit(sequence);
                    } else if inline_block_batches(&batches) {
                        let ids = self.decode_ids(self.authority.current_dimension());
                        self.apply_block_mutations_inline(DecodeJob::BlockUpdates {
                            sequence,
                            batches,
                            ids,
                        });
                    } else {
                        let ids = self.decode_ids(self.authority.current_dimension());
                        self.predictions
                            .begin_server_batch(batches.iter().map(|batch| batch.key));
                        self.enqueue_decode_job(DecodeJob::BlockUpdates {
                            sequence,
                            batches,
                            ids,
                        });
                        self.order.defer_block_updates(sequence);
                    }
                }
            }
        }
        if !self.polling {
            self.dispatch_urgent_work();
        }
    }

    /// Prepares and commits a small block batch on this thread, skipping the worker hop.
    fn apply_block_mutations_inline(&mut self, job: DecodeJob) {
        let completion = job.run(Instant::now());
        self.apply_prepared(completion.event);
        self.finish_ordered_commit(completion.sequence);
    }

    /// Releases a request reservation only when the lower owner finishes the whole event.
    fn finish_ordered_commit(&mut self, sequence: u64) {
        if self.order.finish_commit(sequence) {
            self.cancel_request_reservation(sequence);
        }
    }
}

/// Small batches prepare on the commit thread: copy-on-write costs microseconds, while a
/// worker round trip costs a frame.
fn inline_block_batches(batches: &[BlockMutationBatch]) -> bool {
    batches.len() <= INLINE_BLOCK_MUTATION_SUB_CHUNKS
        && batches
            .iter()
            .map(|batch| batch.updates.len())
            .sum::<usize>()
            <= INLINE_BLOCK_MUTATION_UPDATES
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
