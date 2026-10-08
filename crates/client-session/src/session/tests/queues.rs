use super::{
    Arc, NetworkControlEvent, NetworkHandle, ReadinessIngressCounter, SequencedWorldEvent,
    WorldEvent, WorldIngress, mpsc, test_packet, watch,
};

#[test]
fn network_pending_counts_include_ingress_and_outbound_queues() {
    let (control_event_tx, control_events) = mpsc::channel(2);
    let (world_event_tx, world_events) = mpsc::channel(2);
    let (commands, mut command_rx) = mpsc::channel(2);
    let (shutdown, _shutdown_rx) = watch::channel(false);
    let (physics_reanchor, _physics_reanchor_rx) = watch::channel(0);
    let mut handle = NetworkHandle {
        session_generation: 0,
        control_events,
        world_events,
        commands,
        pending_latency_reply: std::sync::Mutex::new(None),
        physics_reanchor,
        shutdown,
        thread: None,
        readiness_ingress: Arc::new(ReadinessIngressCounter::default()),
        experience_gate: Arc::default(),
        unflushed: Default::default(),
    };

    assert_eq!(handle.pending_event_count(), 0);
    assert_eq!(handle.pending_command_count(), 0);
    control_event_tx
        .try_send(NetworkControlEvent::Stopped {
            decode_error_count: 0,
        })
        .unwrap();
    assert_eq!(handle.pending_event_count(), 1);
    world_event_tx
        .try_send(WorldIngress::Event(SequencedWorldEvent {
            session_generation: 7,
            sequence: 1,
            event: WorldEvent::ChunkRadiusUpdated(16),
        }))
        .unwrap();
    assert_eq!(handle.pending_event_count(), 2);
    handle.control_events_mut().try_recv().unwrap();
    handle.world_events_mut().try_recv().unwrap();
    assert_eq!(handle.pending_event_count(), 0);

    handle.send_packet(test_packet()).unwrap();
    assert_eq!(handle.pending_command_count(), 1);
    command_rx.try_recv().unwrap();
    assert_eq!(handle.pending_command_count(), 0);
}
