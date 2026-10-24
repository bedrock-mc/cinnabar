use super::*;

impl BlobCacheResolver {
    /// Stops network intake while ordinary work is retained behind an earlier cached column.
    #[must_use]
    pub fn ordinary_lane_needs_drain(&self) -> bool {
        !self.immediate_ready.is_empty()
    }

    /// Abandons the cached transactions that block retained ordinary work, including those
    /// that block it through a reconstructed cached packet naming another column. Returns
    /// whether anything was abandoned; a lane that cannot be unblocked is counted once.
    pub fn unblock_ordinary_lane(&mut self) -> Result<bool, BlobCacheError> {
        let mut blocked_before = HashMap::<ColumnKey, u64>::new();
        for ready in self.immediate_ready.values() {
            for column in &ready.columns {
                let bound = blocked_before.entry(*column).or_default();
                *bound = (*bound).max(ready.sequence);
            }
        }
        // Dependencies always point to earlier work. Walking backwards visits each dependent
        // before its blockers, so one pass carries the sequence bounds through every column.
        for ready in self.ready.values().rev() {
            if ready.columns.iter().any(|column| {
                blocked_before
                    .get(column)
                    .is_some_and(|bound| ready.sequence < *bound)
            }) {
                for column in &ready.columns {
                    let bound = blocked_before.entry(*column).or_default();
                    *bound = (*bound).max(ready.sequence);
                }
            }
        }
        let blockers = self
            .pending
            .iter()
            .filter(|(sequence, transaction)| {
                transaction.columns.iter().any(|column| {
                    blocked_before
                        .get(column)
                        .is_some_and(|bound| *sequence < bound)
                })
            })
            .map(|(sequence, _)| *sequence)
            .collect::<Vec<_>>();
        for sequence in &blockers {
            self.abandon_pending_transaction(*sequence)?;
        }
        if blockers.is_empty() {
            if !std::mem::replace(&mut self.ordinary_lane_stalled, true) {
                self.stats.ordinary_lane_stalls = self.stats.ordinary_lane_stalls.saturating_add(1);
                tracing::warn!(
                    target: "cinnabar::blob_cache",
                    immediate = self.immediate_ready.len(),
                    ready = self.ready.len(),
                    pending = self.pending.len(),
                    "retained ordinary world events have no abandonable cached blocker"
                );
            }
        } else {
            self.ordinary_lane_stalled = false;
        }
        self.refresh_pending_accounting()?;
        Ok(!blockers.is_empty())
    }

    pub(super) fn rotate_oldest_pending_transaction(
        &mut self,
    ) -> Result<Option<ChunkResyncEvent>, BlobCacheError> {
        let Some(sequence) = self.pending_order.first().copied() else {
            return Ok(None);
        };
        self.abandon_pending_transaction_with_recovery(sequence, true)
    }

    pub(super) fn merge_status_recovery(
        &mut self,
        status: &mut BlobCacheStatus,
        recovery: Option<ChunkResyncEvent>,
    ) {
        match (status.take_recovery(), recovery) {
            (Some(retained), Some(recovery)) => {
                self.enqueue_precounted_recovery(recovery);
                status.set_recovery(Some(retained));
            }
            (Some(retained), None) => status.set_recovery(Some(retained)),
            (None, recovery) => status.set_recovery(recovery),
        }
    }

    pub(super) fn relieve_cached_ready_pressure(&mut self) -> Result<bool, BlobCacheError> {
        if self.retained_cached_transaction_count() < MAX_CLIENT_BLOB_PENDING_TRANSACTIONS {
            return Ok(false);
        }
        self.unblock_cached_ready_lane()
    }

    /// Under transaction pressure, abandons only unresolved cached work that prevents an already
    /// reconstructed cached packet from being emitted. This preserves per-column ordering while
    /// ensuring the fixed transaction bound cannot permanently retain a blocked ready packet.
    fn unblock_cached_ready_lane(&mut self) -> Result<bool, BlobCacheError> {
        let blockers = self
            .pending
            .iter()
            .filter(|(pending_sequence, pending)| {
                self.ready.values().any(|ready| {
                    **pending_sequence < ready.sequence
                        && pending
                            .columns
                            .iter()
                            .any(|column| ready.columns.contains(column))
                })
            })
            .map(|(sequence, _)| *sequence)
            .collect::<Vec<_>>();
        for sequence in &blockers {
            self.abandon_pending_transaction(*sequence)?;
        }
        self.refresh_pending_accounting()?;
        Ok(!blockers.is_empty())
    }
}
