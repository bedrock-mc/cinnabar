//! Bounded ingress drains shared by the network runtime and acceptance tests.

use std::time::Instant;

pub(crate) fn drain_network_controls<T>(
    receiver: &mut tokio::sync::mpsc::Receiver<T>,
    budget: usize,
) -> Vec<T> {
    drain_network_ingress(receiver, budget)
}

/// Drains one frame's world ingress until the channel empties, admission fills, a transfer
/// barrier arrives, or the frame's time budget is spent; the rest stays in the channel.
pub(crate) struct WorldIngressDrain {
    deadline: Instant,
    stopped: bool,
}

impl WorldIngressDrain {
    /// Starts a frame's drain without reserving consumer admission in advance.
    pub(crate) const fn new(deadline: Instant) -> Self {
        Self {
            deadline,
            stopped: false,
        }
    }

    /// Takes one event only after the caller rechecks headroom changed by the previous commit.
    pub(crate) fn next(
        &mut self,
        receiver: &mut tokio::sync::mpsc::Receiver<super::session::WorldIngress>,
        admission_capacity: usize,
        now: Instant,
    ) -> Option<super::session::WorldIngress> {
        if self.stopped || admission_capacity == 0 || now >= self.deadline {
            return None;
        }
        let ingress = receiver.try_recv().ok()?;
        self.stopped = matches!(
            ingress,
            super::session::WorldIngress::FastTransferBarrier { .. }
        );
        Some(ingress)
    }
}

pub(crate) fn drain_network_ingress<T>(
    receiver: &mut tokio::sync::mpsc::Receiver<T>,
    budget: usize,
) -> Vec<T> {
    std::iter::from_fn(|| receiver.try_recv().ok())
        .take(budget)
        .collect()
}

#[cfg(test)]
mod tests;
