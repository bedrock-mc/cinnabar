use {super::*, inventory::CraftingPreview};

#[test]
fn production_registration_orders_actual_network_control_before_observer_drain() {
    use crate::runtime::network::receive_network_events;
    use crate::ui_runtime::drain_inventory_authority;
    use bevy::{
        ecs::schedule::{IntoSystemSet, NodeId, ScheduleGraph, Schedules},
        prelude::*,
    };
    /// Finds the concrete system node under its automatic system set.
    fn node<M>(graph: &ScheduleGraph, system: impl IntoSystemSet<M>) -> NodeId {
        let key = graph
            .system_sets
            .get_key(system.into_system_set().intern())
            .unwrap();
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
            .unwrap()
    }
    let mut app = App::new();
    crate::app::configure_client_frame_schedule(&mut app);
    crate::app::configure_client_production_frame_systems(&mut app);
    let schedules = app.world().resource::<Schedules>();
    let graph = schedules.get(Update).unwrap().graph();
    let drain = NodeId::Set(
        graph
            .system_sets
            .get_key(drain_inventory_authority.into_system_set().intern())
            .unwrap(),
    );
    assert!(
        graph
            .dependency()
            .graph()
            .contains_edge(node(graph, receive_network_events), drain)
    );
}

#[test]
fn actual_control_and_committed_drain_execute_transfer_fence_with_valid_old_frontier() {
    use {
        crate::{
            movement::PhysicsAuthorityGate,
            runtime::{
                network::{
                    NetworkControlEvent, NetworkHandle, ResourcePackAdmissionState,
                    SessionTransferTarget, receive_network_events,
                },
                publication::PublicationController,
                visibility::AppMetrics,
            },
        },
        client_presentation::{camera::AutoFly, local_player::LocalAvatarPresentation},
    };
    let mut app = app();
    ingress(&mut app, 1, clear_recipes());
    complete_empty_grid(&mut app, 2);
    app.update();
    let old_frontier = app
        .world()
        .resource::<ClientWorld>()
        .stream
        .as_ref()
        .unwrap()
        .inventory_committed_through();
    assert!(old_frontier.is_some());
    assert_eq!(
        app.world()
            .resource::<PlayerRuntime>()
            .inventory
            .crafting_preview(),
        Some(CraftingPreview::NoMatch)
    );
    let (network, sender) = NetworkHandle::stub_with_control_sender();
    sender
        .try_send(NetworkControlEvent::Transferred {
            target: SessionTransferTarget {
                host: "127.0.0.1".into(),
                port: 60475,
            },
            decode_error_count: 0,
        })
        .unwrap();
    app.insert_resource(network)
        .insert_resource(AppMetrics(diagnostics::metrics::MetricsCollector::new()))
        .insert_resource(AutoFly::new(false))
        .init_resource::<ResourcePackAdmissionState>()
        .init_resource::<LocalAvatarPresentation>()
        .init_resource::<PhysicsAuthorityGate>()
        .init_resource::<render::ChunkUploadAcknowledgements>()
        .init_resource::<PublicationController>();
    let mut schedule = Schedule::default();
    schedule.add_systems(
        (
            receive_network_events,
            drain_committed_ui_before_authority,
            drain_inventory_authority,
        )
            .chain(),
    );
    schedule.run(app.world_mut());
    let world = app.world().resource::<ClientWorld>();
    assert!(world.transfer_notice.is_some());
    assert_eq!(
        world.stream.as_ref().unwrap().inventory_committed_through(),
        old_frontier
    );
    assert_eq!(
        app.world()
            .resource::<PlayerRuntime>()
            .inventory
            .crafting_preview(),
        Some(CraftingPreview::NoMatch),
        "the terminal observation fence runs while the old projection is still valid"
    );
}
