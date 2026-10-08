//! Consent is checked before optional traffic can consume world queue capacity.

use std::sync::atomic::{AtomicU8, Ordering};

use server_experience::wire::RateLimit;

use super::{InboundWorldEvent, NetworkHandle, WorldEvent};

#[derive(Default)]
pub(super) struct ExperienceGate(AtomicU8);

impl ExperienceGate {
    /// Tests the shared revocation flag immediately before a socket write.
    pub(super) fn enabled(&self) -> bool {
        self.0.load(Ordering::Acquire) == 1
    }

    /// Drops pre-consent data before assigning any world publication sequence.
    pub(super) fn admit(
        &self,
        event: &InboundWorldEvent,
        rate: &mut Option<RateLimit>,
        now_ms: u64,
    ) -> bool {
        let InboundWorldEvent::Event(WorldEvent::Experience(message)) = event else {
            return true;
        };
        if !self.enabled() {
            return false;
        }
        if rate
            .get_or_insert_with(|| RateLimit::new(now_ms))
            .charge(message.bytes.len(), now_ms)
            .is_err()
        {
            self.0.store(2, Ordering::Release);
            return false;
        }
        true
    }
}

impl<P> NetworkHandle<P> {
    /// Opens only after local consent; a failed channel cannot reopen this session.
    pub fn set_experience_enabled(&self, enabled: bool) {
        if enabled {
            let _ =
                self.experience_gate
                    .0
                    .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire);
        } else {
            let _ =
                self.experience_gate
                    .0
                    .compare_exchange(1, 0, Ordering::AcqRel, Ordering::Acquire);
        }
    }

    /// Reports a flood without disconnecting ordinary Bedrock play.
    pub fn experience_failed(&self) -> bool {
        self.experience_gate.0.load(Ordering::Acquire) == 2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Constructs only the optional event, with no network session.
    fn optional() -> InboundWorldEvent {
        InboundWorldEvent::Event(WorldEvent::Experience(protocol::ExperienceMessage {
            bytes: b"unsolicited".to_vec(),
        }))
    }

    #[test]
    fn absent_consent_does_not_create_queue_or_rate_state() {
        let gate = ExperienceGate::default();
        let mut rate = None;
        for _ in 0..1000 {
            assert!(!gate.admit(&optional(), &mut rate, 0));
        }
        assert!(rate.is_none());
        assert!(gate.admit(
            &InboundWorldEvent::Event(WorldEvent::ChunkRadiusUpdated(16)),
            &mut rate,
            0,
        ));
    }

    #[test]
    fn flood_quarantines_only_the_optional_channel() {
        let gate = ExperienceGate(AtomicU8::new(1));
        let mut rate = None;
        for _ in 0..server_experience::policy::MAX_MESSAGES_PER_SECOND {
            assert!(gate.admit(&optional(), &mut rate, 0));
        }
        assert!(!gate.admit(&optional(), &mut rate, 0));
        assert!(!gate.enabled());
        assert!(!gate.admit(&optional(), &mut rate, 10_000));
    }

    struct RecordingSession(std::sync::Arc<std::sync::Mutex<Vec<bytes::Bytes>>>);

    impl super::super::NetworkSession for RecordingSession {
        type Error = String;
        type Outbound = super::super::tests::PacketOutbound<String>;

        /// Captures the same generated encoding used by the ordinary transport.
        fn outbound(&mut self) -> Result<Self::Outbound, String> {
            let sent = std::sync::Arc::clone(&self.0);
            Ok(super::super::tests::PacketOutbound::new(move |packet| {
                let session = protocol::BedrockSession { shield_item_id: 0 };
                sent.lock()
                    .unwrap()
                    .push(protocol::encode(&packet, &session).unwrap());
                std::future::ready(Ok(()))
            }))
        }

        /// Keeps this fixture strictly offline and outbound-only.
        async fn receive_world_event(&mut self, _: i32) -> Result<WorldEvent, String> {
            std::future::pending().await
        }

        /// This fixture never decodes network input.
        fn decode_error_count(&self) -> u64 {
            0
        }
    }

    #[tokio::test]
    async fn unadvertised_pump_preserves_vanilla_bytes_and_sends_no_probe() {
        use super::super::{NetworkCommand, NetworkSequencer, pump_runtime::run_network_pump};
        use tokio::sync::{mpsc, watch};

        let sent = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let packets = [
            protocol::modal_form_cancel_response(7),
            protocol::modal_form_cancel_response(9),
        ];
        let session = protocol::BedrockSession { shield_item_id: 0 };
        let expected: Vec<_> = packets
            .iter()
            .map(|packet| protocol::encode(packet, &session).unwrap())
            .collect();
        let (commands, receiver) = mpsc::channel(5);
        for packet in packets
            .into_iter()
            .chain([protocol::experience_packet(b"must not escape".to_vec()).unwrap()])
        {
            commands
                .try_send(NetworkCommand::Send {
                    packet,
                    sub_chunk: None,
                    chat: None,
                    physics: None,
                    physics_reanchor: None,
                    interaction: None,
                })
                .unwrap();
        }
        commands.try_send(NetworkCommand::FlushFrame).unwrap();
        drop(commands);
        let (control, _control_rx) = mpsc::channel(4);
        let (world, _world_rx) = mpsc::channel(4);
        let (_shutdown, shutdown_rx) = watch::channel(false);
        run_network_pump(
            RecordingSession(std::sync::Arc::clone(&sent)),
            NetworkSequencer::new(1, 0, 1),
            receiver,
            control,
            world,
            shutdown_rx,
        )
        .await;
        assert_eq!(*sent.lock().unwrap(), expected);
    }
}
