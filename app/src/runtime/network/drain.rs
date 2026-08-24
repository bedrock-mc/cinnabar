//! Bounded ingress drains shared by the network runtime and acceptance tests.

pub(crate) fn acceptance_surface_anchor(position: [f32; 3]) -> [i32; 2] {
    [position[0].floor() as i32, position[2].floor() as i32]
}

pub(crate) fn drain_network_controls<T>(
    receiver: &mut tokio::sync::mpsc::Receiver<T>,
    budget: usize,
) -> Vec<T> {
    drain_network_ingress(receiver, budget)
}

pub(crate) fn drain_world_ingress_until_barrier(
    receiver: &mut tokio::sync::mpsc::Receiver<super::session::WorldIngress>,
    budget: usize,
) -> Vec<super::session::WorldIngress> {
    let mut drained = Vec::with_capacity(budget);
    for _ in 0..budget {
        let Ok(ingress) = receiver.try_recv() else {
            break;
        };
        let is_barrier = matches!(
            ingress,
            super::session::WorldIngress::FastTransferBarrier { .. }
        );
        drained.push(ingress);
        if is_barrier {
            break;
        }
    }
    drained
}

pub(crate) fn drain_network_ingress<T>(
    receiver: &mut tokio::sync::mpsc::Receiver<T>,
    budget: usize,
) -> Vec<T> {
    std::iter::from_fn(|| receiver.try_recv().ok())
        .take(budget)
        .collect()
}
