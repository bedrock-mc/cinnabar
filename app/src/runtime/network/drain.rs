//! Bounded ingress drains shared by the network runtime and acceptance tests.

pub(crate) fn drain_network_controls<T>(
    receiver: &mut tokio::sync::mpsc::Receiver<T>,
    budget: usize,
) -> Vec<T> {
    drain_network_ingress(receiver, budget)
}

/// Limits one frame's receives while leaving blocked or post-barrier events in the channel.
pub(crate) struct WorldIngressDrain {
    remaining: usize,
    stopped: bool,
}

impl WorldIngressDrain {
    /// Starts a frame's packet budget without reserving consumer admission in advance.
    pub(crate) const fn new(budget: usize) -> Self {
        Self {
            remaining: budget,
            stopped: false,
        }
    }

    /// Takes one event only after the caller rechecks headroom changed by the previous commit.
    pub(crate) fn next(
        &mut self,
        receiver: &mut tokio::sync::mpsc::Receiver<super::session::WorldIngress>,
        admission_capacity: usize,
    ) -> Option<super::session::WorldIngress> {
        if self.remaining == 0 || self.stopped || admission_capacity == 0 {
            return None;
        }
        let ingress = receiver.try_recv().ok()?;
        self.remaining -= 1;
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
