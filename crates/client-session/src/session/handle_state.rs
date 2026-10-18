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
        let (physics_reanchor, _physics_reanchor_rx) =
            watch::channel(simulation_clock::identity::PositionAuthorityGeneration::default());
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
pub struct CapturedPackets {
    commands: mpsc::Receiver<NetworkCommand>,
    pending: Vec<Packet>,
}

#[cfg(any(test, feature = "test-support"))]
impl CapturedPackets {
    /// Drains queued packets regardless of whether their frame has been flushed.
    pub fn drain(&mut self) -> Vec<Packet> {
        let mut packets = std::mem::take(&mut self.pending);
        while let Ok(command) = self.commands.try_recv() {
            if let NetworkCommand::Send { packet, .. } = command {
                packets.push(packet);
            }
        }
        packets
    }

    /// Returns one submitted frame, retaining unflushed packets for the next call.
    pub fn take_flushed_frame(&mut self) -> Option<Vec<Packet>> {
        while let Ok(command) = self.commands.try_recv() {
            match command {
                NetworkCommand::Send { packet, .. } => self.pending.push(packet),
                NetworkCommand::FlushFrame { .. } => {
                    return Some(std::mem::take(&mut self.pending));
                }
                NetworkCommand::FinishLoading => {}
            }
        }
        None
    }
}

#[cfg(any(test, feature = "test-support"))]
impl<P> NetworkHandle<P> {
    /// Captures packets from the normal bounded command FIFO.
    pub fn stub_capturing_packets() -> (Self, CapturedPackets) {
        Self::stub_capturing_packets_with_capacity(COMMAND_CAPACITY)
    }

    /// Captures real command admission with a chosen payload capacity.
    pub fn stub_capturing_packets_with_capacity(capacity: usize) -> (Self, CapturedPackets) {
        let (mut handle, _) = Self::stub();
        let (commands, command_rx) = frame_flush::command_channel(capacity);
        handle.commands = commands;
        (
            handle,
            CapturedPackets {
                commands: command_rx,
                pending: Vec::new(),
            },
        )
    }
}
