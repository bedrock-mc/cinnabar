use super::*;

impl BlobCacheResolver {
    /// Stops network intake while ordinary work is retained behind an earlier cached column.
    #[must_use]
    pub fn ordinary_lane_needs_drain(&self) -> bool {
        !self.immediate_ready.is_empty()
    }

    /// Abandons only cached transactions that block retained ordinary work.
    pub fn unblock_ordinary_lane(&mut self) -> Result<bool, BlobCacheError> {
        let blocked_columns = self
            .immediate_ready
            .values()
            .flat_map(|ready| ready.columns.iter())
            .copied()
            .collect::<HashSet<_>>();
        let blockers = self
            .pending
            .iter()
            .filter(|(_, transaction)| {
                transaction
                    .columns
                    .iter()
                    .any(|column| blocked_columns.contains(column))
            })
            .map(|(sequence, _)| *sequence)
            .collect::<Vec<_>>();
        for sequence in &blockers {
            self.abandon_pending_transaction(*sequence)?;
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
