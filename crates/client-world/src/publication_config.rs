use std::{
    collections::BTreeMap,
    fmt,
    sync::{Arc, Mutex},
};

/// Frozen elapsed-time publication service bounds for the Phase 2 gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicationServiceConfig {
    pub minimum_items_per_second: u32,
    pub minimum_bytes_per_second: u64,
    pub target_items_per_second: u32,
    pub target_bytes_per_second: u64,
    pub maximum_frame_items: usize,
    pub maximum_frame_bytes: u64,
    pub maximum_burst_items: usize,
    pub maximum_burst_bytes: u64,
    pub maximum_zero_byte_operations_per_frame: usize,
}

impl PublicationServiceConfig {
    pub const PHASE2_GATE: Self = Self {
        minimum_items_per_second: 4_096,
        minimum_bytes_per_second: 64 * 1024 * 1024,
        target_items_per_second: 8_192,
        target_bytes_per_second: 128 * 1024 * 1024,
        maximum_frame_items: 512,
        maximum_frame_bytes: 64 * 1024 * 1024,
        maximum_burst_items: 8_192,
        maximum_burst_bytes: 128 * 1024 * 1024,
        maximum_zero_byte_operations_per_frame: 256,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicationPermitStage {
    MeshReady,
    Handoff,
    RenderEntity,
    GpuPrepared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PublicationPermitClass {
    Payload { bytes: u64 },
    ZeroByte,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LivePermit {
    stage: PublicationPermitStage,
    class: PublicationPermitClass,
}

#[derive(Debug)]
struct PublicationAllowanceState {
    config: PublicationServiceConfig,
    frame_sequence: u64,
    next_permit_id: u64,
    remaining_items: usize,
    remaining_bytes: u64,
    frame_remaining_items: usize,
    frame_remaining_bytes: u64,
    remaining_zero_byte_operations: usize,
    live_payload_items: usize,
    live_payload_bytes: u64,
    live_zero_byte_operations: usize,
    live: BTreeMap<u64, LivePermit>,
}

#[derive(Clone, Debug)]
pub struct PublicationAllowance {
    inner: Arc<Mutex<PublicationAllowanceState>>,
}

pub struct PublicationPermit {
    id: u64,
    inner: Arc<Mutex<PublicationAllowanceState>>,
    active: bool,
}

impl fmt::Debug for PublicationPermit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PublicationPermit")
            .field("id", &self.id)
            .field("stage", &self.stage())
            .finish()
    }
}

impl PublicationAllowance {
    #[must_use]
    pub fn new(config: PublicationServiceConfig) -> Self {
        Self {
            inner: Arc::new(Mutex::new(PublicationAllowanceState {
                config,
                frame_sequence: 0,
                next_permit_id: 0,
                remaining_items: 0,
                remaining_bytes: 0,
                frame_remaining_items: 0,
                frame_remaining_bytes: 0,
                remaining_zero_byte_operations: 0,
                live_payload_items: 0,
                live_payload_bytes: 0,
                live_zero_byte_operations: 0,
                live: BTreeMap::new(),
            })),
        }
    }

    pub fn begin_frame(
        &self,
        frame_sequence: u64,
        issued_items: usize,
        issued_bytes: u64,
        issued_zero_byte_operations: usize,
    ) {
        let mut state = self.lock();
        if frame_sequence <= state.frame_sequence {
            return;
        }
        state.frame_sequence = frame_sequence;
        state.remaining_items = state
            .remaining_items
            .checked_add(issued_items)
            .unwrap_or(state.config.maximum_burst_items)
            .min(state.config.maximum_burst_items);
        state.remaining_bytes = state
            .remaining_bytes
            .checked_add(issued_bytes)
            .unwrap_or(state.config.maximum_burst_bytes)
            .min(state.config.maximum_burst_bytes);
        state.frame_remaining_items = state.remaining_items.min(state.config.maximum_frame_items);
        state.frame_remaining_bytes = state.remaining_bytes.min(state.config.maximum_frame_bytes);
        state.remaining_zero_byte_operations =
            issued_zero_byte_operations.min(state.config.maximum_zero_byte_operations_per_frame);
    }

    #[must_use]
    pub fn try_admit_payload(&self, bytes: u64) -> Option<PublicationPermit> {
        if bytes == 0 {
            return None;
        }
        let mut state = self.lock();
        if state.remaining_items == 0
            || state.frame_remaining_items == 0
            || bytes > state.remaining_bytes
            || bytes > state.frame_remaining_bytes
            || state.live_payload_items >= state.config.maximum_frame_items
            || state
                .live_payload_bytes
                .checked_add(bytes)
                .is_none_or(|total| total > state.config.maximum_frame_bytes)
        {
            return None;
        }
        state.remaining_items = state.remaining_items.checked_sub(1)?;
        state.remaining_bytes = state.remaining_bytes.checked_sub(bytes)?;
        state.frame_remaining_items = state.frame_remaining_items.checked_sub(1)?;
        state.frame_remaining_bytes = state.frame_remaining_bytes.checked_sub(bytes)?;
        Some(insert_permit(
            &mut state,
            &self.inner,
            PublicationPermitClass::Payload { bytes },
        ))
    }

    #[must_use]
    pub fn try_admit_zero_byte(&self) -> Option<PublicationPermit> {
        let mut state = self.lock();
        if state.live_zero_byte_operations >= state.config.maximum_zero_byte_operations_per_frame {
            return None;
        }
        state.remaining_zero_byte_operations =
            state.remaining_zero_byte_operations.checked_sub(1)?;
        Some(insert_permit(
            &mut state,
            &self.inner,
            PublicationPermitClass::ZeroByte,
        ))
    }

    #[must_use]
    pub fn remaining_items(&self) -> usize {
        self.lock().remaining_items
    }

    #[must_use]
    pub fn remaining_bytes(&self) -> u64 {
        self.lock().remaining_bytes
    }

    #[must_use]
    pub fn remaining_zero_byte_operations(&self) -> usize {
        self.lock().remaining_zero_byte_operations
    }

    #[must_use]
    pub fn frame_remaining_items(&self) -> usize {
        self.lock().frame_remaining_items
    }

    #[must_use]
    pub fn frame_remaining_bytes(&self) -> u64 {
        self.lock().frame_remaining_bytes
    }

    #[must_use]
    pub fn live_permits(&self) -> usize {
        self.lock().live.len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, PublicationAllowanceState> {
        self.inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }
}

impl PublicationPermit {
    #[must_use]
    pub fn stage(&self) -> Option<PublicationPermitStage> {
        self.lock().live.get(&self.id).map(|permit| permit.stage)
    }

    #[must_use]
    pub fn bytes(&self) -> Option<u64> {
        self.lock()
            .live
            .get(&self.id)
            .map(|permit| match permit.class {
                PublicationPermitClass::Payload { bytes } => bytes,
                PublicationPermitClass::ZeroByte => 0,
            })
    }

    #[must_use]
    pub fn is_zero_byte(&self) -> bool {
        self.lock()
            .live
            .get(&self.id)
            .is_some_and(|permit| matches!(permit.class, PublicationPermitClass::ZeroByte))
    }

    fn transfer(
        mut self,
        expected: PublicationPermitStage,
        next: PublicationPermitStage,
    ) -> Result<Self, Self> {
        let inner = Arc::clone(&self.inner);
        let mut state = inner.lock().unwrap_or_else(|poison| poison.into_inner());
        let Some(permit) = state.live.get_mut(&self.id) else {
            drop(state);
            self.active = false;
            return Err(self);
        };
        if permit.stage != expected {
            drop(state);
            return Err(self);
        }
        permit.stage = next;
        drop(state);
        Ok(self)
    }

    pub fn into_handoff(self) -> Result<Self, Self> {
        self.transfer(
            PublicationPermitStage::MeshReady,
            PublicationPermitStage::Handoff,
        )
    }

    pub fn into_render_entity(self) -> Result<Self, Self> {
        self.transfer(
            PublicationPermitStage::Handoff,
            PublicationPermitStage::RenderEntity,
        )
    }

    pub fn into_gpu_prepared(self) -> Result<Self, Self> {
        self.into_gpu_prepared_with_additional_bytes(0)
    }

    /// Atomically charges whole-arena growth to the admitted item and moves
    /// its one linear capability into the terminal GPU-prepared stage.
    pub fn into_gpu_prepared_with_additional_bytes(
        mut self,
        additional_bytes: u64,
    ) -> Result<Self, Self> {
        let inner = Arc::clone(&self.inner);
        let mut state = inner.lock().unwrap_or_else(|poison| poison.into_inner());
        let Some(permit) = state.live.get(&self.id).copied() else {
            drop(state);
            self.active = false;
            return Err(self);
        };
        if permit.stage != PublicationPermitStage::RenderEntity {
            drop(state);
            return Err(self);
        }
        if additional_bytes == 0 {
            state
                .live
                .get_mut(&self.id)
                .expect("live permit was checked")
                .stage = PublicationPermitStage::GpuPrepared;
            drop(state);
            return Ok(self);
        }
        let PublicationPermitClass::Payload { bytes } = permit.class else {
            drop(state);
            return Err(self);
        };
        if additional_bytes > state.remaining_bytes
            || additional_bytes > state.frame_remaining_bytes
            || state
                .live_payload_bytes
                .checked_add(additional_bytes)
                .is_none_or(|total| total > state.config.maximum_frame_bytes)
        {
            drop(state);
            return Err(self);
        }
        let Some(next_bytes) = bytes.checked_add(additional_bytes) else {
            drop(state);
            return Err(self);
        };
        state.remaining_bytes -= additional_bytes;
        state.frame_remaining_bytes -= additional_bytes;
        state.live_payload_bytes += additional_bytes;
        let live = state
            .live
            .get_mut(&self.id)
            .expect("live permit was checked");
        live.class = PublicationPermitClass::Payload { bytes: next_bytes };
        live.stage = PublicationPermitStage::GpuPrepared;
        drop(state);
        Ok(self)
    }

    pub fn retire(mut self) -> bool {
        let inner = Arc::clone(&self.inner);
        let mut state = inner.lock().unwrap_or_else(|poison| poison.into_inner());
        let retired = remove_live_permit(&mut state, self.id);
        drop(state);
        self.active = false;
        retired
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, PublicationAllowanceState> {
        self.inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }
}

impl Drop for PublicationPermit {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let inner = Arc::clone(&self.inner);
        let mut state = inner.lock().unwrap_or_else(|poison| poison.into_inner());
        remove_live_permit(&mut state, self.id);
        self.active = false;
    }
}

fn insert_permit(
    state: &mut PublicationAllowanceState,
    inner: &Arc<Mutex<PublicationAllowanceState>>,
    class: PublicationPermitClass,
) -> PublicationPermit {
    state.next_permit_id = state.next_permit_id.checked_add(1).unwrap_or(1).max(1);
    let id = state.next_permit_id;
    let replaced = state.live.insert(
        id,
        LivePermit {
            stage: PublicationPermitStage::MeshReady,
            class,
        },
    );
    assert!(replaced.is_none(), "live publication permit id collided");
    match class {
        PublicationPermitClass::Payload { bytes } => {
            state.live_payload_items += 1;
            state.live_payload_bytes += bytes;
        }
        PublicationPermitClass::ZeroByte => state.live_zero_byte_operations += 1,
    }
    PublicationPermit {
        id,
        inner: Arc::clone(inner),
        active: true,
    }
}

fn remove_live_permit(state: &mut PublicationAllowanceState, id: u64) -> bool {
    let Some(permit) = state.live.remove(&id) else {
        return false;
    };
    match permit.class {
        PublicationPermitClass::Payload { bytes } => {
            state.live_payload_items = state.live_payload_items.saturating_sub(1);
            state.live_payload_bytes = state.live_payload_bytes.saturating_sub(bytes);
        }
        PublicationPermitClass::ZeroByte => {
            state.live_zero_byte_operations = state.live_zero_byte_operations.saturating_sub(1);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_linear_permit_transfers_across_every_stage() {
        let allowance = PublicationAllowance::new(PublicationServiceConfig::PHASE2_GATE);
        allowance.begin_frame(1, 1, 1, 1);
        let permit = allowance
            .try_admit_payload(1)
            .expect("one payload token is available");
        assert!(allowance.try_admit_payload(1).is_none());
        let permit = permit.into_handoff().unwrap();
        let permit = permit.into_render_entity().unwrap();
        let permit = permit.into_gpu_prepared().unwrap();
        assert!(permit.retire());
        assert_eq!(allowance.live_permits(), 0);
        assert_eq!(allowance.remaining_items(), 0);
        assert_eq!(allowance.remaining_bytes(), 0);
    }

    #[test]
    fn stale_or_dropped_permit_is_retired_without_refunding_spent_authority() {
        let allowance = PublicationAllowance::new(PublicationServiceConfig::PHASE2_GATE);
        allowance.begin_frame(1, 1, 64, 1);
        let permit = allowance.try_admit_payload(64).unwrap();

        let permit = permit.into_render_entity().unwrap_err();
        assert!(permit.retire());
        assert!(allowance.try_admit_payload(1).is_none());
        assert_eq!(allowance.live_permits(), 0);
        assert_eq!(allowance.remaining_items(), 0);
        assert_eq!(allowance.remaining_bytes(), 0);
    }

    #[test]
    fn dropping_the_linear_carrier_retires_the_live_permit_without_refund() {
        let allowance = PublicationAllowance::new(PublicationServiceConfig::PHASE2_GATE);
        allowance.begin_frame(1, 1, 64, 0);
        let permit = allowance.try_admit_payload(64).unwrap();
        drop(permit);
        assert_eq!(allowance.live_permits(), 0);
        assert_eq!(allowance.remaining_items(), 0);
        assert_eq!(allowance.remaining_bytes(), 0);
    }

    #[test]
    fn five_hundred_twelve_payload_and_zero_byte_attempts_enforce_distinct_exact_caps() {
        let config = PublicationServiceConfig::PHASE2_GATE;
        let allowance = PublicationAllowance::new(config);
        allowance.begin_frame(1, 512, 512, config.maximum_zero_byte_operations_per_frame);
        let payload = (0..512)
            .map(|_| allowance.try_admit_payload(1))
            .collect::<Vec<_>>();
        let zero_byte = (0..512)
            .map(|_| allowance.try_admit_zero_byte())
            .collect::<Vec<_>>();

        assert_eq!(payload.iter().flatten().count(), 512);
        assert_eq!(zero_byte.iter().flatten().count(), 256);
        assert_eq!(allowance.remaining_items(), 0);
        assert_eq!(allowance.remaining_bytes(), 0);
        assert_eq!(allowance.remaining_zero_byte_operations(), 0);
        allowance.begin_frame(2, 512, 512, config.maximum_zero_byte_operations_per_frame);
        assert!(allowance.try_admit_payload(1).is_none());
        assert!(allowance.try_admit_zero_byte().is_none());
        for permit in payload
            .into_iter()
            .flatten()
            .chain(zero_byte.into_iter().flatten())
        {
            assert!(permit.retire());
        }
        assert_eq!(allowance.live_permits(), 0);
    }

    #[test]
    fn live_payload_bytes_never_cross_the_literal_64_mib_boundary_across_frames() {
        let config = PublicationServiceConfig::PHASE2_GATE;
        let allowance = PublicationAllowance::new(config);
        allowance.begin_frame(1, 1, config.maximum_frame_bytes, 0);
        let permit = allowance
            .try_admit_payload(config.maximum_frame_bytes)
            .expect("the literal boundary itself is permitted");

        allowance.begin_frame(2, 512, config.maximum_frame_bytes, 0);
        assert!(allowance.try_admit_payload(1).is_none());
        let permit = permit.into_handoff().unwrap().into_render_entity().unwrap();
        let permit = permit
            .into_gpu_prepared_with_additional_bytes(1)
            .unwrap_err();
        assert!(permit.retire());
    }

    #[test]
    fn unused_payload_authority_accumulates_without_crossing_the_burst_ceiling() {
        let config = PublicationServiceConfig::PHASE2_GATE;
        let allowance = PublicationAllowance::new(config);

        allowance.begin_frame(1, 512, 1024, config.maximum_zero_byte_operations_per_frame);
        allowance.begin_frame(2, 512, 1024, config.maximum_zero_byte_operations_per_frame);
        assert_eq!(allowance.remaining_items(), 1_024);
        assert_eq!(allowance.remaining_bytes(), 2_048);

        for frame in 3..=18 {
            allowance.begin_frame(
                frame,
                config.maximum_frame_items,
                config.maximum_frame_bytes,
                config.maximum_zero_byte_operations_per_frame,
            );
        }
        assert_eq!(allowance.remaining_items(), config.maximum_burst_items);
        assert_eq!(allowance.remaining_bytes(), config.maximum_burst_bytes);
    }

    #[test]
    fn arena_growth_charges_bytes_to_the_existing_item_without_an_item_redebit() {
        let allowance = PublicationAllowance::new(PublicationServiceConfig::PHASE2_GATE);
        allowance.begin_frame(1, 1, 96, 0);
        let permit = allowance.try_admit_payload(64).unwrap();

        let permit = permit.into_handoff().unwrap().into_render_entity().unwrap();
        let permit = permit.into_gpu_prepared_with_additional_bytes(32).unwrap();
        assert_eq!(permit.bytes(), Some(96));
        assert_eq!(allowance.remaining_items(), 0);
        assert_eq!(allowance.remaining_bytes(), 0);
        assert!(permit.retire());
    }
}
