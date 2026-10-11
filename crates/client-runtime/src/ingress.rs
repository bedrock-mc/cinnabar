//! Bounded ingress drains shared by the network runtime and acceptance tests.

use std::time::{Duration, Instant};

/// One frame's world ingress time; terrain is bounded by admission, not by this.
pub const WORLD_INGRESS_DRAIN_BUDGET: Duration = Duration::from_millis(2);
pub const OUTBOUND_SEND_BUDGET_PER_FRAME: usize = 16;

/// Takes at most one frame's control allowance without waiting for another event.
pub fn drain_network_controls<T>(
    receiver: &mut tokio::sync::mpsc::Receiver<T>,
    budget: usize,
) -> Vec<T> {
    drain_network_ingress(receiver, budget)
}

/// Drains one frame's world ingress until the channel empties, admission fills, a transfer
/// barrier arrives, or the frame's time budget is spent; the rest stays in the channel.
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

    /// Takes one event only after the caller rechecks headroom changed by the previous commit.
    pub fn next(
        &mut self,
        receiver: &mut tokio::sync::mpsc::Receiver<client_session::WorldIngress>,
        admission_capacity: usize,
        now: Instant,
    ) -> Option<client_session::WorldIngress> {
        if self.stopped || admission_capacity == 0 || now >= self.deadline {
            return None;
        }
        let ingress = receiver.try_recv().ok()?;
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
