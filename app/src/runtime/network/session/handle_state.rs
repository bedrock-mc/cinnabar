use super::*;

impl NetworkHandle {
    /// The pump queues its terminal control before dropping the command
    /// receiver. When both conditions hold, outbound producers must let the
    /// bounded control drain classify that close instead of racing it with a
    /// generic send-side error.
    pub(crate) fn closed_command_has_pending_control(&self) -> bool {
        self.commands.is_closed() && !self.control_events.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn stub_with_control_sender() -> (Self, mpsc::Sender<NetworkControlEvent>) {
        let (control_event_tx, control_events) = mpsc::channel(CONTROL_EVENT_CAPACITY);
        let (_world_event_tx, world_events) = mpsc::channel(1);
        let (commands, _command_rx) = mpsc::channel(1);
        let (physics_reanchor, _physics_reanchor_rx) = watch::channel(0);
        let (shutdown, _shutdown_rx) = watch::channel(false);
        (
            Self {
                control_events,
                world_events,
                commands,
                physics_reanchor,
                shutdown,
                thread: None,
                readiness_ingress: Arc::new(ReadinessIngressCounter::default()),
            },
            control_event_tx,
        )
    }
}
