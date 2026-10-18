//! Bounded ingress drains shared by the network runtime and acceptance tests.

use chunk_pipeline::WorldStream;
use client_session::WorldIngress;
use client_world::ingestion::{Footprint, WorldStreamError};
use protocol::WorldEvent;
use std::{collections::VecDeque, time::Duration};
use web_time::Instant;

/// One frame's ingress time allowance, rechecked between payloads.
pub const WORLD_INGRESS_DRAIN_BUDGET: Duration = Duration::from_millis(2);
pub const OUTBOUND_SEND_BUDGET_PER_FRAME: usize = 16;
/// Drains a full control queue per frame so send receipts cannot accumulate and block
/// the writer while terrain requests and movement share the outbound channel.
pub const NETWORK_CONTROL_BUDGET_PER_FRAME: usize = client_session::CONTROL_EVENT_CAPACITY;

/// Takes at most one frame's control allowance without waiting for another event.
pub fn drain_network_controls<T>(
    receiver: &mut tokio::sync::mpsc::Receiver<T>,
    budget: usize,
) -> Vec<T> {
    drain_network_ingress(receiver, budget)
}

/// Drains a frame's ingress with bounded lookahead past full admission lanes.
/// Retained payloads register their dependencies in the stream before later events pass.
pub struct WorldIngressDrain {
    deadline: Instant,
    stopped: bool,
}

impl WorldIngressDrain {
    /// Starts a frame's drain without reserving consumer admission in advance.
    pub const fn new(deadline: Instant) -> Self {
        Self {
            deadline,
            stopped: false,
        }
    }

    /// Returns the next event with lane capacity, retaining blocked payloads across frames.
    /// The supplied clock is rechecked between payloads and retained scans.
    pub fn next(
        &mut self,
        receiver: &mut tokio::sync::mpsc::Receiver<WorldIngress>,
        pending: &mut VecDeque<WorldIngress>,
        mut stream: Option<&mut WorldStream>,
        mut now: impl FnMut() -> Instant,
    ) -> Result<Option<WorldIngress>, WorldStreamError> {
        if self.stopped || now() >= self.deadline {
            return Ok(None);
        }
        for index in 0..pending.len() {
            if now() >= self.deadline {
                return Ok(None);
            }
            let ingress = &pending[index];
            if let Some(stream) = stream.as_deref_mut()
                && !stream.reserve_ingress_light(sequence(ingress))
            {
                return Ok(None);
            }
            let footprint = stream
                .as_deref()
                .map(|stream| stream.retained_ingress_footprint(sequence(ingress)));
            let is_barrier = matches!(ingress, WorldIngress::FastTransferBarrier { .. })
                || footprint.is_some_and(|footprint| footprint.barrier);
            if index > 0 && is_barrier {
                return Ok(None);
            }
            if (index == 0 || footprint.is_none_or(bypasses_terrain))
                && admits_retained(ingress, stream.as_deref(), footprint)
            {
                let ingress = pending.remove(index).expect("the retained ingress exists");
                if let Some(stream) = stream.as_deref_mut() {
                    stream.release_ingress(sequence(&ingress));
                }
                return Ok(Some(self.admitted(ingress)));
            }
            if is_barrier {
                return Ok(None);
            }
        }
        while pending.len() < client_session::WORLD_EVENT_CAPACITY {
            if now() >= self.deadline {
                return Ok(None);
            }
            let Ok(ingress) = receiver.try_recv() else {
                return Ok(None);
            };
            if pending.is_empty() && admits(&ingress, stream.as_deref(), None) {
                return Ok(Some(self.admitted(ingress)));
            }
            let stream = stream
                .as_deref_mut()
                .expect("absent streams accept ingress");
            let footprint = footprint(&ingress, stream);
            if bypasses_terrain(&footprint) && admits(&ingress, Some(stream), Some(&footprint)) {
                return Ok(Some(self.admitted(ingress)));
            }
            let is_barrier = footprint.barrier;
            let has_credit = stream.retain_ingress(
                sequence(&ingress),
                footprint,
                client_session::WORLD_EVENT_CAPACITY,
            )?;
            pending.push_back(ingress);
            if is_barrier || !has_credit {
                return Ok(None);
            }
        }
        Ok(None)
    }

    /// Ends this drain when a transfer marker reaches ordinary admission.
    fn admitted(&mut self, ingress: WorldIngress) -> WorldIngress {
        self.stopped = matches!(ingress, WorldIngress::FastTransferBarrier { .. });
        ingress
    }
}

/// Includes a retained light payload's own credit when checking its admission.
fn admits_retained(
    ingress: &WorldIngress,
    stream: Option<&WorldStream>,
    footprint: Option<&Footprint>,
) -> bool {
    match (stream, footprint) {
        (Some(stream), Some(footprint)) if !footprint.heavy => {
            stream.retained_ingress_light_capacity(sequence(ingress)) > 0
        }
        _ => admits(ingress, stream, footprint),
    }
}

/// Checks the event's own admission lane without copying a chunk's payload buffer.
fn admits(
    ingress: &WorldIngress,
    stream: Option<&WorldStream>,
    footprint: Option<&Footprint>,
) -> bool {
    stream.is_none_or(|stream| {
        let can_admit = |event: &WorldEvent| {
            footprint.map_or_else(
                || stream.can_admit(event),
                |footprint| stream.can_admit_with_footprint(event, footprint),
            )
        };
        match ingress {
            WorldIngress::Event(event) => can_admit(&event.event),
            WorldIngress::LevelChunk { event, .. } => {
                can_admit(&WorldEvent::LevelChunk(event.clone()))
            }
            WorldIngress::FastTransferBarrier { .. } => stream.light_admission_capacity() > 0,
        }
    })
}

/// Allows light session events past retained terrain without reordering terrain preparation.
fn bypasses_terrain(footprint: &Footprint) -> bool {
    !footprint.heavy && !footprint.barrier && !footprint.retention && footprint.columns.is_empty()
}

/// Uses the stream's classification for both payload representations and transfer markers.
fn footprint(ingress: &WorldIngress, stream: &WorldStream) -> client_world::ingestion::Footprint {
    match ingress {
        WorldIngress::Event(event) => stream.ingress_footprint(&event.event),
        WorldIngress::LevelChunk { event, .. } => {
            stream.ingress_footprint(&WorldEvent::LevelChunk(event.clone()))
        }
        WorldIngress::FastTransferBarrier { .. } => client_world::ingestion::Footprint::barrier(),
    }
}

/// Identifies the reservation associated with a retained world payload.
fn sequence(ingress: &WorldIngress) -> u64 {
    match ingress {
        WorldIngress::Event(event) => event.sequence.get(),
        WorldIngress::LevelChunk { sequence, .. }
        | WorldIngress::FastTransferBarrier { sequence, .. } => sequence.get(),
    }
}

/// Takes up to budget queued events and leaves the remainder for another frame.
pub fn drain_network_ingress<T>(
    receiver: &mut tokio::sync::mpsc::Receiver<T>,
    budget: usize,
) -> Vec<T> {
    std::iter::from_fn(|| receiver.try_recv().ok())
        .take(budget)
        .collect()
}

#[cfg(test)]
mod tests;
