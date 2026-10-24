use super::{
    Arc, NetworkControlEvent, NetworkHandle, ReadinessIngressCounter, SequencedWorldEvent,
    WorldEvent, WorldIngress, mpsc, test_packet, watch,
};
use protocol::identity::FifoSequence;
use simulation_clock::identity::{PositionAuthorityGeneration, SessionGeneration};

#[test]
fn network_pending_counts_include_ingress_and_outbound_queues() {
    let (control_event_tx, control_events) = mpsc::channel(2);
    let (world_event_tx, world_events) = mpsc::channel(2);
    let (commands, mut command_rx) = super::super::frame_flush::command_channel(2);
    let (shutdown, _shutdown_rx) = watch::channel(false);
    let (physics_reanchor, _physics_reanchor_rx) =
        watch::channel(PositionAuthorityGeneration::default());
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
        connection_observation: Default::default(),
        unflushed: Default::default(),
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        packet_delay: Default::default(),
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
            session_generation: SessionGeneration::new(7),
            sequence: FifoSequence::new(1),
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

/// Models raw transport input separately from world-event decoding.
#[derive(Default)]
struct BackpressuredSession {
    input: std::collections::VecDeque<WorldEvent>,
    buffered: std::collections::VecDeque<WorldEvent>,
    decoded: usize,
}

impl super::NetworkSession for BackpressuredSession {
    type Error = &'static str;
    type Outbound = super::PacketOutbound<&'static str>;

    /// Supplies a writer independent of the stalled world consumer.
    fn outbound(&mut self) -> Result<Self::Outbound, Self::Error> {
        Ok(super::PacketOutbound::accepting())
    }

    /// Decodes only when world admission permits normal reception again.
    async fn receive_world_event(&mut self, _: i32) -> Result<WorldEvent, Self::Error> {
        let event = self.buffered.pop_front().or_else(|| self.input.pop_front());
        self.decoded += 1;
        event.ok_or("input ended")
    }

    /// Reports that this fixture's transport input is valid.
    fn decode_error_count(&self) -> u64 {
        0
    }

    /// Retains already available input without applying or decoding it.
    async fn read_ahead(&mut self) {
        self.buffered.append(&mut self.input);
    }
}

/// A full world channel must still read transport input without decoding it or overtaking
/// the event that is waiting for channel capacity.
#[tokio::test]
async fn full_world_ingress_reads_ahead_without_reordering_or_decoding() {
    use super::NetworkSession;
    use crate::session::pump::{WorldSideWork, wait_for_world_side_work};
    use std::{future::Future, task::Context};

    let event = |time| WorldEvent::SetTime(protocol::SetTimeEvent { time });
    let ingress = |sequence, time| {
        WorldIngress::Event(SequencedWorldEvent {
            session_generation: SessionGeneration::new(1),
            sequence: FifoSequence::new(sequence),
            event: event(time),
        })
    };
    let (tx, mut rx) = mpsc::channel(1);
    tx.try_send(ingress(1, 10)).unwrap();
    let mut session = BackpressuredSession {
        input: [event(30), event(40)].into(),
        ..Default::default()
    };
    {
        let mut work = std::pin::pin!(wait_for_world_side_work(&mut session, 0, &tx, true));
        let mut context = Context::from_waker(std::task::Waker::noop());
        assert!(work.as_mut().poll(&mut context).is_pending());
    }
    assert!(
        session.input.is_empty(),
        "transport reads stopped with world admission"
    );
    assert_eq!(session.buffered.len(), 2);
    assert_eq!(session.decoded, 0);
    assert!(
        matches!(rx.try_recv().unwrap(), WorldIngress::Event(event) if event.sequence.get() == 1)
    );
    let WorldSideWork::Capacity(Ok(permit)) =
        wait_for_world_side_work(&mut session, 0, &tx, true).await
    else {
        panic!("free capacity must admit the retained world event")
    };
    permit.send(ingress(2, 20));
    assert!(
        matches!(rx.try_recv().unwrap(), WorldIngress::Event(event) if event.sequence.get() == 2)
    );
    for expected in [30, 40] {
        assert!(
            matches!(session.receive_world_event(0).await.unwrap(), WorldEvent::SetTime(event) if event.time == expected)
        );
    }
    assert_eq!(session.decoded, 2);
}
