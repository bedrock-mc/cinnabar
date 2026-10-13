use super::*;

/// Request inputs retained independently of the column's biome decode.
pub(super) struct AdmittedRequest {
    key: ChunkKey,
    base_y: i32,
    count: usize,
    pub(super) started: bool,
}

impl WorldStream {
    /// Retains request inputs at admission without copying the column payload.
    pub(super) fn admit_level_chunk_request(
        &mut self,
        sequence: u64,
        event: &LevelChunkEvent,
        range: DimensionRange,
    ) {
        let count = match event.mode {
            LevelChunkMode::LimitedRequests { highest } => {
                usize::from(highest).min(range.sub_chunk_count)
            }
            LevelChunkMode::LimitlessRequests => range.sub_chunk_count,
            LevelChunkMode::Inline { .. } => unreachable!("request decode has a request mode"),
        };
        if count != 0 {
            self.requests.level_chunks.insert(
                sequence,
                AdmittedRequest {
                    key: ChunkKey::new(event.dimension, event.x, event.z),
                    base_y: range.base_sub_chunk_y,
                    count,
                    started: false,
                },
            );
            self.start_admitted_level_chunk_requests();
        }
    }

    /// Issues requests before decode while respecting earlier column, definition and view fences.
    pub(super) fn start_admitted_level_chunk_requests(&mut self) {
        let mut admissions = std::mem::take(&mut self.requests.level_chunks);
        for (&sequence, request) in &mut admissions {
            if request.started
                || !self
                    .order
                    .preparation_is_unblocked(sequence, self.local_player_chunk.is_none())
                || !self.column_is_data_interesting(request.key)
            {
                continue;
            }
            self.loaded_columns.remove(&request.key);
            self.requests.collision_failures.remove(&request.key);
            self.requests.purge_columns(&BTreeSet::from([request.key]));
            self.enqueue_request(request.key, request.base_y, request.count, Some(sequence));
            request.started = true;
        }
        self.requests.level_chunks = admissions;
    }

    /// Cancels an early request if its column could not commit; successful commits remove it first.
    pub(super) fn cancel_admitted_level_chunk_request(&mut self, sequence: u64) {
        if let Some(request) = self.requests.level_chunks.remove(&sequence)
            && request.started
        {
            self.requests.purge_columns(&BTreeSet::from([request.key]));
        }
    }
}
