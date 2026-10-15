//! Bounded ingress drains shared by the network runtime and acceptance tests.

use std::time::{Duration, Instant};

/// One frame's world ingress time; terrain is bounded by admission, not by this.
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

/// Drains one frame's world ingress until the channel empties, admission fills, a transfer
/// barrier arrives, or the frame's time budget is spent. Unconsumed events stay in the
/// caller's retained head and channel.
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

    /// Checks the next event's own admission lane. A blocked head stays in `pending` across
    /// frames, ahead of the receiver; time and transfer boundaries never read ahead.
    pub fn next(
        &mut self,
        receiver: &mut tokio::sync::mpsc::Receiver<client_session::WorldIngress>,
        pending: &mut Option<client_session::WorldIngress>,
        admits: impl FnOnce(&client_session::WorldIngress) -> bool,
        now: Instant,
    ) -> Option<client_session::WorldIngress> {
        if self.stopped || now >= self.deadline {
            return None;
        }
        if pending.is_none() {
            *pending = Some(receiver.try_recv().ok()?);
        }
        if !admits(pending.as_ref().expect("the FIFO head is retained")) {
            return None;
        }
        let ingress = pending.take().expect("the FIFO head was admitted");
        self.stopped = matches!(
            ingress,
            client_session::WorldIngress::FastTransferBarrier { .. }
        );
        Some(ingress)
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
