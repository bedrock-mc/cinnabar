use super::*;
use bevy::ecs::schedule::graph::Direction;

pub(super) fn stage_node(graph: &ScheduleGraph, stage: ClientFrameSet) -> NodeId {
    let key = graph
        .system_sets
        .get_key(stage.intern())
        .expect("production stage");
    NodeId::Set(key)
}

/// Follows dependencies inherited from containing sets without ordering their siblings.
pub(super) fn dependency_path_exists(graph: &ScheduleGraph, before: NodeId, after: NodeId) -> bool {
    let dependencies = graph.dependency().graph();
    let targets = containing_nodes(graph, after);
    let mut pending = containing_nodes(graph, before);
    let mut visited = HashSet::new();
    while let Some(node) = pending.pop() {
        if !visited.insert(node) {
            continue;
        }
        for successor in dependencies.neighbors(node) {
            if targets.contains(&successor) {
                return true;
            }
            pending.push(successor);
        }
    }
    false
}

/// Includes a node and every set whose ordering constraints apply to it.
fn containing_nodes(graph: &ScheduleGraph, node: NodeId) -> Vec<NodeId> {
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
}

pub(super) fn assert_system_in_stage<M>(
    graph: &ScheduleGraph,
    system: impl IntoSystemSet<M>,
    label: &str,
    stage: ClientFrameSet,
) {
    assert!(
        graph
            .hierarchy()
            .graph()
            .contains_edge(stage_node(graph, stage), system_node(graph, system, label),)
    );
}

pub(super) fn system_set_node<M>(
    system: impl IntoSystemSet<M>,
    graph: &ScheduleGraph,
    label: &str,
) -> NodeId {
    let key = graph
        .system_sets
        .get_key(system.into_system_set().intern())
        .unwrap_or_else(|| panic!("missing {label}"));
    NodeId::Set(key)
}

pub(super) fn system_node<M>(
    graph: &ScheduleGraph,
    system: impl IntoSystemSet<M>,
    label: &str,
) -> NodeId {
    let key = graph
        .system_sets
        .get_key(system.into_system_set().intern())
        .unwrap_or_else(|| panic!("missing {label}"));
    let parent = NodeId::Set(key);
    graph
        .systems
        .iter()
        .find_map(|(key, _, _)| {
            let child = NodeId::System(key);
            graph
                .hierarchy()
                .graph()
                .contains_edge(parent, child)
                .then_some(child)
        })
        .unwrap_or_else(|| panic!("missing {label}"))
}
