use std::collections::VecDeque;

use protocol::EquipmentEvent;

pub const MAX_PRE_IDENTITY_EQUIPMENT: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EquipmentRoute {
    LocalSelected {
        fifo_sequence: u64,
        event: EquipmentEvent,
    },
    ActorPresentation {
        fifo_sequence: u64,
        event: EquipmentEvent,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EquipmentRouteResult {
    Buffered,
    Routed(EquipmentRoute),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InventoryRouterError {
    WrongSession { expected: u64, actual: u64 },
    StaleFifoSequence { previous: u64, actual: u64 },
    InvalidRuntimeId(u64),
    ConflictingRuntimeId { previous: u64, actual: u64 },
    PreIdentityBufferFull { maximum: usize },
}

#[derive(Clone, Debug)]
struct PendingEquipment {
    fifo_sequence: u64,
    event: EquipmentEvent,
}

#[derive(Clone, Debug)]
pub struct InventoryEquipmentRouter {
    session_id: u64,
    local_runtime_id: Option<u64>,
    last_fifo_sequence: Option<u64>,
    pending: VecDeque<PendingEquipment>,
}

impl InventoryEquipmentRouter {
    #[must_use]
    pub fn new(session_id: u64) -> Self {
        Self {
            session_id,
            local_runtime_id: None,
            last_fifo_sequence: None,
            pending: VecDeque::with_capacity(MAX_PRE_IDENTITY_EQUIPMENT),
        }
    }

    #[must_use]
    pub const fn session_id(&self) -> u64 {
        self.session_id
    }

    #[must_use]
    pub const fn local_runtime_id(&self) -> Option<u64> {
        self.local_runtime_id
    }

    #[must_use]
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    pub fn begin_session(&mut self, session_id: u64) {
        if self.session_id == session_id {
            return;
        }
        self.session_id = session_id;
        self.local_runtime_id = None;
        self.last_fifo_sequence = None;
        self.pending.clear();
    }

    pub fn publish_local_runtime_id(
        &mut self,
        session_id: u64,
        runtime_id: u64,
    ) -> Result<Vec<EquipmentRoute>, InventoryRouterError> {
        self.validate_session(session_id)?;
        if runtime_id == 0 {
            return Err(InventoryRouterError::InvalidRuntimeId(runtime_id));
        }
        if let Some(previous) = self.local_runtime_id {
            if previous == runtime_id {
                return Ok(Vec::new());
            }
            return Err(InventoryRouterError::ConflictingRuntimeId {
                previous,
                actual: runtime_id,
            });
        }
        self.local_runtime_id = Some(runtime_id);
        Ok(self
            .pending
            .drain(..)
            .map(|pending| route_for(runtime_id, pending.fifo_sequence, pending.event))
            .collect())
    }

    pub fn route(
        &mut self,
        session_id: u64,
        fifo_sequence: u64,
        event: EquipmentEvent,
    ) -> Result<EquipmentRouteResult, InventoryRouterError> {
        self.validate_session(session_id)?;
        if let Some(previous) = self.last_fifo_sequence
            && fifo_sequence <= previous
        {
            return Err(InventoryRouterError::StaleFifoSequence {
                previous,
                actual: fifo_sequence,
            });
        }
        let result = if let Some(local_runtime_id) = self.local_runtime_id {
            EquipmentRouteResult::Routed(route_for(local_runtime_id, fifo_sequence, event))
        } else {
            if self.pending.len() >= MAX_PRE_IDENTITY_EQUIPMENT {
                return Err(InventoryRouterError::PreIdentityBufferFull {
                    maximum: MAX_PRE_IDENTITY_EQUIPMENT,
                });
            }
            self.pending.push_back(PendingEquipment {
                fifo_sequence,
                event,
            });
            EquipmentRouteResult::Buffered
        };
        self.last_fifo_sequence = Some(fifo_sequence);
        Ok(result)
    }

    fn validate_session(&self, session_id: u64) -> Result<(), InventoryRouterError> {
        if session_id != self.session_id {
            return Err(InventoryRouterError::WrongSession {
                expected: self.session_id,
                actual: session_id,
            });
        }
        Ok(())
    }
}

fn route_for(local_runtime_id: u64, fifo_sequence: u64, event: EquipmentEvent) -> EquipmentRoute {
    if event.actor_runtime_id == local_runtime_id {
        EquipmentRoute::LocalSelected {
            fifo_sequence,
            event,
        }
    } else {
        EquipmentRoute::ActorPresentation {
            fifo_sequence,
            event,
        }
    }
}
