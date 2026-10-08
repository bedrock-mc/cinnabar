use super::*;

impl<P> NetworkHandle<P> {
    /// The pump queues its terminal control before dropping the command
    /// receiver. When both conditions hold, outbound producers must let the
    /// bounded control drain classify that close instead of racing it with a
    /// generic send-side error.
    pub fn closed_command_has_pending_control(&self) -> bool {
        self.commands.is_closed() && !self.control_events.is_empty()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn stub_with_control_sender() -> (Self, mpsc::Sender<NetworkControlEvent<P>>) {
        let (control_event_tx, control_events) = mpsc::channel(CONTROL_EVENT_CAPACITY);
        let (_world_event_tx, world_events) = mpsc::channel(1);
        let (commands, _command_rx) = mpsc::channel(1);
        let (physics_reanchor, _physics_reanchor_rx) = watch::channel(0);
        let (shutdown, _shutdown_rx) = watch::channel(false);
        (
            Self {
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
            },
            control_event_tx,
        )
    }
}

/// Every packet a stub handle queued, in send order.
#[cfg(any(test, feature = "test-support"))]
pub struct CapturedPackets(mpsc::Receiver<NetworkCommand>);

#[cfg(any(test, feature = "test-support"))]
impl CapturedPackets {
    pub fn drain(&mut self) -> Vec<Packet> {
        std::iter::from_fn(|| self.0.try_recv().ok())
            .filter_map(|command| match command {
                NetworkCommand::Send { packet, .. } => Some(packet),
                NetworkCommand::FinishLoading | NetworkCommand::FlushFrame => None,
            })
            .collect()
    }
}

#[cfg(any(test, feature = "test-support"))]
impl<P> NetworkHandle<P> {
    pub fn stub_capturing_packets() -> (Self, CapturedPackets) {
        let (mut handle, _) = Self::stub();
        let (commands, command_rx) = mpsc::channel(64);
        handle.commands = commands;
        (handle, CapturedPackets(command_rx))
    }
}
