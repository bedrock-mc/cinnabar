use crate::chunk::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelWitnessRequestError {
    InvalidRevision,
    InvalidHash,
    Empty,
    TooMany,
    Duplicate,
}

#[derive(Resource, ExtractResource, Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelWitnessRequest {
    pub(in crate::chunk) revision: u64,
    pub(in crate::chunk) request_hash: [u8; 32],
    pub(in crate::chunk) keys: Arc<[SubChunkKey]>,
}

impl ModelWitnessRequest {
    pub fn try_new(
        revision: u64,
        request_hash: [u8; 32],
        mut keys: Vec<SubChunkKey>,
    ) -> Result<Self, ModelWitnessRequestError> {
        if revision == 0 {
            return Err(ModelWitnessRequestError::InvalidRevision);
        }
        if request_hash == [0; 32] {
            return Err(ModelWitnessRequestError::InvalidHash);
        }
        if keys.is_empty() {
            return Err(ModelWitnessRequestError::Empty);
        }
        if keys.len() > MAX_MODEL_WITNESS_KEYS {
            return Err(ModelWitnessRequestError::TooMany);
        }
        keys.sort_unstable();
        if keys.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(ModelWitnessRequestError::Duplicate);
        }
        Ok(Self {
            revision,
            request_hash,
            keys: Arc::from(keys),
        })
    }

    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    #[must_use]
    pub const fn request_hash(&self) -> &[u8; 32] {
        &self.request_hash
    }

    #[must_use]
    pub fn keys(&self) -> &[SubChunkKey] {
        &self.keys
    }

    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.revision != 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ModelWitnessManifestRecord {
    pub key: SubChunkKey,
    pub generation: u64,
    pub model_ref_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelWitnessFrameAck {
    pub revision: u64,
    pub request_hash: [u8; 32],
    pub frame_sequence: u64,
    pub view_generation: u64,
    pub present_returned_at: Instant,
    pub gpu_completed_at: Instant,
    pub total_model_ref_count: usize,
    pub manifest: Arc<[ModelWitnessManifestRecord]>,
    pub missing_key_count: usize,
    pub stale_generation_count: usize,
    pub wrong_stream_count: usize,
    pub zero_model_ref_count: usize,
    pub draw_mismatch_count: usize,
}

impl ModelWitnessFrameAck {
    #[must_use]
    pub fn is_exact(&self) -> bool {
        !self.manifest.is_empty()
            && self.total_model_ref_count != 0
            && self.missing_key_count == 0
            && self.stale_generation_count == 0
            && self.wrong_stream_count == 0
            && self.zero_model_ref_count == 0
            && self.draw_mismatch_count == 0
    }

    #[must_use]
    pub fn forms_stable_exact_pair_with(&self, next: &Self) -> bool {
        self.is_exact()
            && next.is_exact()
            && self.revision == next.revision
            && self.request_hash == next.request_hash
            && self.view_generation == next.view_generation
            && self.total_model_ref_count == next.total_model_ref_count
            && self.manifest == next.manifest
            && self.frame_sequence.checked_add(1) == Some(next.frame_sequence)
            && self.present_returned_at <= next.present_returned_at
            && self.gpu_completed_at <= next.gpu_completed_at
            && self.present_returned_at <= self.gpu_completed_at
            && next.present_returned_at <= next.gpu_completed_at
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelWitnessEvent {
    pub acknowledgement: ModelWitnessFrameAck,
    pub consecutive: u8,
}

#[derive(Debug, Default)]
pub(in crate::chunk) struct ModelWitnessEvidenceState {
    pub(in crate::chunk) active: ModelWitnessRequest,
    pub(in crate::chunk) first: Option<ModelWitnessFrameAck>,
    pub(in crate::chunk) complete: bool,
    pub(in crate::chunk) events: VecDeque<ModelWitnessEvent>,
}

#[derive(Resource, Debug, Clone, Default)]
pub struct ModelWitnessEvidence(pub(in crate::chunk) Arc<Mutex<ModelWitnessEvidenceState>>);

impl ModelWitnessEvidence {
    pub fn set_authoritative_request(&self, request: &ModelWitnessRequest) {
        let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        if state.active == *request {
            return;
        }
        state.active = request.clone();
        state.first = None;
        state.complete = false;
        state.events.clear();
    }

    pub fn observe_presented_frame(
        &self,
        request: &ModelWitnessRequest,
        acknowledgement: &PresentedFrameAck,
    ) {
        let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        if state.complete || state.active != *request {
            return;
        }
        if !acknowledgement.is_model_witness_compatible() {
            state.first = None;
            return;
        }
        let Some(current) = acknowledgement.model_witness.as_ref().filter(|current| {
            current.revision == request.revision
                && current.request_hash == request.request_hash
                && current.manifest.len() == request.keys.len()
                && current.is_exact()
        }) else {
            state.first = None;
            return;
        };
        let Some(first) = state.first.take() else {
            state.first = Some(current.clone());
            return;
        };
        if !first.forms_stable_exact_pair_with(current) {
            state.first = Some(current.clone());
            return;
        }
        state.events.push_back(ModelWitnessEvent {
            acknowledgement: first,
            consecutive: 1,
        });
        state.events.push_back(ModelWitnessEvent {
            acknowledgement: current.clone(),
            consecutive: 2,
        });
        state.complete = true;
    }

    pub fn drain_events(&self) -> Vec<ModelWitnessEvent> {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .events
            .drain(..)
            .collect()
    }

    #[must_use]
    pub fn is_complete_for(&self, request: &ModelWitnessRequest) -> bool {
        let state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        state.complete && state.active == *request
    }

    pub fn reset(&self) {
        self.set_authoritative_request(&ModelWitnessRequest::default());
    }
}
