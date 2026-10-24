//! Ordering and membership queries over the production client schedule graph.

use bevy::ecs::schedule::{
    IntoSystemSet, NodeId, ScheduleGraph, SystemSet,
    graph::{DiGraph, Direction},
};
use client_state::ClientFrameSet;

/// Checks ordering inherited from containing sets without treating shared membership as order.
pub(super) fn schedule_precedes(graph: &ScheduleGraph, from: NodeId, target: NodeId) -> bool {
    let ancestors = |node| {
        let mut pending = vec![node];
        let mut found = Vec::new();
        while let Some(node) = pending.pop() {
            if !found.contains(&node) {
                found.push(node);
                pending.extend(
                    graph
                        .hierarchy()
                        .graph()
                        .neighbors_directed(node, Direction::Incoming),
                );
            }
        }
        found
    };
    let targets = ancestors(target);
    ancestors(from).into_iter().any(|from| {
        targets.iter().any(|&target| {
            from != target && schedule_reaches(graph.dependency().graph(), from, target)
        })
    })
}

/// Follows explicit graph edges through any number of intermediate nodes.
fn schedule_reaches(graph: &DiGraph<NodeId>, from: NodeId, target: NodeId) -> bool {
    let mut pending = vec![from];
    let mut visited = Vec::new();
    while let Some(node) = pending.pop() {
        if node == target {
            return true;
        }
        if !visited.contains(&node) {
            visited.push(node);
            pending.extend(graph.neighbors(node));
        }
    }
    false
}

/// The set node of a production frame stage.
pub(super) fn stage_node(graph: &ScheduleGraph, stage: ClientFrameSet) -> NodeId {
    let key = graph
        .system_sets
        .get_key(stage.intern())
        .unwrap_or_else(|| panic!("missing production stage {stage:?}"));
    NodeId::Set(key)
}

/// Asserts that a production system belongs to a frame stage.
pub(super) fn assert_system_in_stage<M>(
    graph: &ScheduleGraph,
    system: impl IntoSystemSet<M>,
    label: &str,
    stage: ClientFrameSet,
) {
    assert!(
        schedule_reaches(
            graph.hierarchy().graph(),
            stage_node(graph, stage),
            system_node(graph, system, label),
        ),
        "production system {label} is not a member of {stage:?}"
    );
}

/// The one system node registered for `system`; panics when it is missing or duplicated.
pub(super) fn system_node<M>(
    graph: &ScheduleGraph,
    system: impl IntoSystemSet<M>,
    label: &str,
) -> NodeId {
    let type_set = graph
        .system_sets
        .get_key(system.into_system_set().intern())
        .unwrap_or_else(|| panic!("missing production system type set {label}"));
    let parent = NodeId::Set(type_set);
    let mut matches = graph.systems.iter().filter_map(|(key, _, _)| {
        let child = NodeId::System(key);
        graph
            .hierarchy()
            .graph()
            .contains_edge(parent, child)
            .then_some(child)
    });
    let node = matches
        .next()
        .unwrap_or_else(|| panic!("missing production system {label}"));
    assert!(
        matches.next().is_none(),
        "production system {label} is registered more than once"
    );
    node
}
