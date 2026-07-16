use crate::chunk::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransparentWitnessRequestError {
    InvalidRevision,
    Empty,
    TooMany,
    Duplicate,
}

#[derive(Resource, ExtractResource, Debug, Clone, Default, PartialEq, Eq)]
pub struct TransparentWitnessRequest {
    pub(in crate::chunk) revision: u64,
    pub(in crate::chunk) keys: Arc<[SubChunkKey]>,
}

impl TransparentWitnessRequest {
    pub fn try_new(
        revision: u64,
        mut keys: Vec<SubChunkKey>,
    ) -> Result<Self, TransparentWitnessRequestError> {
        if revision == 0 {
            return Err(TransparentWitnessRequestError::InvalidRevision);
        }
        if keys.is_empty() {
            return Err(TransparentWitnessRequestError::Empty);
        }
        if keys.len() > MAX_TRANSPARENT_WITNESS_KEYS {
            return Err(TransparentWitnessRequestError::TooMany);
        }
        keys.sort_unstable();
        if keys.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(TransparentWitnessRequestError::Duplicate);
        }
        Ok(Self {
            revision,
            keys: Arc::from(keys),
        })
    }

    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    #[must_use]
    pub fn keys(&self) -> &[SubChunkKey] {
        &self.keys
    }

    pub(in crate::chunk) const fn enabled(&self) -> bool {
        self.revision != 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransparentWitnessEvent {
    pub revision: u64,
    pub sequence: u64,
    pub generation: u64,
    pub key_count: usize,
    pub consecutive: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransparentWitnessIncompleteEvent {
    pub revision: u64,
    pub sequence: u64,
    pub generation: u64,
    pub missing_keys: Arc<[SubChunkKey]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransparentWitnessStageRecord {
    pub key: SubChunkKey,
    pub extracted_visible: bool,
    pub instance_present: bool,
    pub liquid_quad_count: usize,
    pub instance_generation: u64,
    pub allocation_present: bool,
    pub liquid_range_len: u32,
    pub lighting_range_len: u32,
    pub allocation_matches: bool,
    pub committed_member: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransparentWitnessStageEvent {
    pub revision: u64,
    pub committed_generation: u64,
    pub records: Arc<[TransparentWitnessStageRecord]>,
}

#[derive(Debug, Clone)]
pub(in crate::chunk) struct TransparentWitnessToken {
    pub(in crate::chunk) request: TransparentWitnessRequest,
    pub(in crate::chunk) sequence: u64,
    pub(in crate::chunk) generation: u64,
    pub(in crate::chunk) missing_keys: Arc<[SubChunkKey]>,
}

#[derive(Debug, Default)]
pub(in crate::chunk) struct TransparentWitnessEvidenceState {
    pub(in crate::chunk) active: TransparentWitnessRequest,
    pub(in crate::chunk) next_sequence: u64,
    pub(in crate::chunk) in_flight: Option<u64>,
    pub(in crate::chunk) consecutive: u8,
    pub(in crate::chunk) events: VecDeque<TransparentWitnessEvent>,
    pub(in crate::chunk) last_missing_keys: Arc<[SubChunkKey]>,
    pub(in crate::chunk) incomplete_events: VecDeque<TransparentWitnessIncompleteEvent>,
    pub(in crate::chunk) last_stage_records: Arc<[TransparentWitnessStageRecord]>,
    pub(in crate::chunk) stage_event_count: u8,
    pub(in crate::chunk) stage_events: VecDeque<TransparentWitnessStageEvent>,
}

#[derive(Resource, Debug, Clone, Default)]
pub struct TransparentWitnessEvidence(
    pub(in crate::chunk) Arc<Mutex<TransparentWitnessEvidenceState>>,
);

impl TransparentWitnessEvidence {
    #[cfg(test)]
    pub(in crate::chunk) fn try_reserve(
        &self,
        request: &TransparentWitnessRequest,
        generation: u64,
        complete: bool,
    ) -> Option<TransparentWitnessToken> {
        let missing = if complete {
            Vec::new()
        } else {
            request.keys().to_vec()
        };
        self.try_reserve_missing(request, generation, missing)
    }

    pub(in crate::chunk) fn try_reserve_missing(
        &self,
        request: &TransparentWitnessRequest,
        generation: u64,
        missing_keys: Vec<SubChunkKey>,
    ) -> Option<TransparentWitnessToken> {
        if !request.enabled() {
            return None;
        }
        let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        if state.active != *request {
            return None;
        }
        if state.in_flight.is_some() || state.consecutive >= 2 {
            return None;
        }
        state.next_sequence = state.next_sequence.checked_add(1)?;
        let sequence = state.next_sequence;
        state.in_flight = Some(sequence);
        Some(TransparentWitnessToken {
            request: request.clone(),
            sequence,
            generation,
            missing_keys: missing_keys.into(),
        })
    }

    pub(in crate::chunk) fn complete(&self, token: TransparentWitnessToken) -> bool {
        let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        if state.active != token.request || state.in_flight != Some(token.sequence) {
            return false;
        }
        state.in_flight = None;
        if !token.missing_keys.is_empty() {
            state.consecutive = 0;
            state.events.clear();
            if state.last_missing_keys != token.missing_keys {
                state.last_missing_keys = Arc::clone(&token.missing_keys);
                if state.incomplete_events.len() == 4 {
                    state.incomplete_events.pop_front();
                }
                state
                    .incomplete_events
                    .push_back(TransparentWitnessIncompleteEvent {
                        revision: token.request.revision,
                        sequence: token.sequence,
                        generation: token.generation,
                        missing_keys: token.missing_keys,
                    });
            }
            return true;
        }
        state.consecutive = state.consecutive.saturating_add(1).min(2);
        let event = TransparentWitnessEvent {
            revision: token.request.revision,
            sequence: token.sequence,
            generation: token.generation,
            key_count: token.request.keys.len(),
            consecutive: state.consecutive,
        };
        if state.events.len() == 4 {
            state.events.pop_front();
        }
        state.events.push_back(event);
        true
    }

    pub fn drain_events(&self) -> Vec<TransparentWitnessEvent> {
        let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        state.events.drain(..).collect()
    }

    pub fn drain_incomplete_events(&self) -> Vec<TransparentWitnessIncompleteEvent> {
        let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        state.incomplete_events.drain(..).collect()
    }

    pub(in crate::chunk) fn record_stage_snapshot(
        &self,
        revision: u64,
        committed_generation: u64,
        mut records: Vec<TransparentWitnessStageRecord>,
    ) -> bool {
        records.sort_unstable_by_key(|record| record.key);
        let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        if revision == 0
            || state.active.revision() != revision
            || records.len() != state.active.keys().len()
            || !records
                .iter()
                .zip(state.active.keys())
                .all(|(record, key)| record.key == *key)
            || state.last_stage_records.as_ref() == records.as_slice()
            || state.stage_event_count >= 8
        {
            return false;
        }
        state.last_stage_records = Arc::from(records);
        state.stage_event_count += 1;
        let records = Arc::clone(&state.last_stage_records);
        state.stage_events.push_back(TransparentWitnessStageEvent {
            revision,
            committed_generation,
            records,
        });
        true
    }

    pub fn drain_stage_events(&self) -> Vec<TransparentWitnessStageEvent> {
        let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        state.stage_events.drain(..).collect()
    }

    pub fn set_authoritative_request(&self, request: &TransparentWitnessRequest) {
        let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        if state.active == *request {
            return;
        }
        state.active = request.clone();
        state.in_flight = None;
        state.consecutive = 0;
        state.events.clear();
        state.last_missing_keys = Arc::default();
        state.incomplete_events.clear();
        state.last_stage_records = Arc::default();
        state.stage_event_count = 0;
        state.stage_events.clear();
    }

    pub fn reset(&self) {
        self.set_authoritative_request(&TransparentWitnessRequest::default());
    }
}
