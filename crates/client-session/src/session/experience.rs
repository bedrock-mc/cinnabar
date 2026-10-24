//! Consent is checked before optional traffic can consume world queue capacity.

use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};

use server_experience::rate_limit::{RateClass, RateLimit};

use super::{InboundWorldEvent, NetworkHandle, WorldEvent};

#[derive(Default)]
pub(super) struct ExperienceGate {
    state: AtomicU8,
    /// Highest world-notice epoch admitted or acknowledged; anything at or below it is a duplicate.
    notice_epoch: AtomicU64,
}

impl ExperienceGate {
    /// Tests the shared revocation flag immediately before a socket write.
    pub(super) fn enabled(&self) -> bool {
        self.state.load(Ordering::Acquire) == 1
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
        let class = if let Some((_, epoch)) = message.world_notice() {
            // Raising the watermark at admission drops a duplicate that arrives before its
            // acknowledgment is written, while the peer's next notice still has a higher epoch.
            if message.as_bytes().len() > protocol::MAX_EXPERIENCE_CONTROL_BYTES
                || self.notice_epoch.fetch_max(epoch, Ordering::AcqRel) >= epoch
            {
                return false;
            }
            RateClass::WorldNotice
        } else {
            RateClass::Data
        };
        if rate
            .get_or_insert_with(|| RateLimit::new(now_ms))
            .charge_class(class, message.as_bytes().len(), now_ms)
            .is_err()
        {
            self.state.store(2, Ordering::Release);
            return false;
        }
        true
    }

    /// Retires equal and older notices before the acknowledgment reaches the transport.
    pub(super) fn acknowledged(&self, epoch: u64) {
        self.notice_epoch.fetch_max(epoch, Ordering::AcqRel);
    }
}

impl<P> NetworkHandle<P> {
    /// Opens only after local consent; a failed channel cannot reopen this session.
    pub fn set_experience_enabled(&self, enabled: bool) {
        if enabled {
            let _ = self.experience_gate.state.compare_exchange(
                0,
                1,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        } else {
            let _ = self.experience_gate.state.compare_exchange(
                1,
                0,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
    }

    /// Reports a flood without disconnecting ordinary Bedrock play.
    pub fn experience_failed(&self) -> bool {
        self.experience_gate.state.load(Ordering::Acquire) == 2
    }
}

/// Exercises normalized carrier input through the production network admission gate.
#[cfg(any(test, feature = "test-support"))]
pub struct ExperienceIngress {
    gate: ExperienceGate,
    rate: Option<RateLimit>,
}

#[cfg(any(test, feature = "test-support"))]
impl Default for ExperienceIngress {
    /// Opens an offline connection with local consent already granted.
    fn default() -> Self {
        Self {
            gate: ExperienceGate {
                state: AtomicU8::new(1),
                ..Default::default()
            },
            rate: None,
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl ExperienceIngress {
    /// Normalizes a carrier and returns it only when the real gate admits it.
    pub fn admit(&mut self, bytes: Vec<u8>, now_ms: u64) -> Option<protocol::ExperienceMessage> {
        let event = protocol::into_world_event(protocol::experience_packet(bytes)?, 0).ok()??;
        let event = InboundWorldEvent::Event(event);
        if !self.gate.admit(&event, &mut self.rate, now_ms) {
            return None;
        }
        let InboundWorldEvent::Event(WorldEvent::Experience(message)) = event else {
            return None;
        };
        Some(message)
    }

    /// Models handing an outbound acknowledgment to the transport.
    pub fn written(&self, bytes: Vec<u8>) {
        if let Some(epoch) =
            protocol::experience_world_ack(&protocol::experience_packet(bytes).unwrap())
        {
            self.gate.acknowledged(epoch);
        }
    }

    /// Reports whether rejected traffic quarantined the optional connection.
    pub fn failed(&self) -> bool {
        !self.gate.enabled()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Constructs only the optional event, with no network session.
    fn optional() -> InboundWorldEvent {
        InboundWorldEvent::Event(WorldEvent::Experience(protocol::ExperienceMessage::new(
            b"unsolicited".to_vec(),
        )))
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
        let gate = ExperienceGate {
            state: AtomicU8::new(1),
            ..Default::default()
        };
        let mut rate = None;
        for _ in 0..server_experience::policy::MAX_MESSAGES_PER_SECOND {
            assert!(gate.admit(&optional(), &mut rate, 0));
        }
        assert!(!gate.admit(&optional(), &mut rate, 0));
        assert!(!gate.enabled());
        assert!(!gate.admit(&optional(), &mut rate, 10_000));
    }

    #[test]
    fn world_notice_survives_exhausted_data_credits() {
        let mut ingress = ExperienceIngress::default();
        for _ in 0..server_experience::policy::MAX_MESSAGES_PER_SECOND {
            assert!(ingress.admit(b"data".to_vec(), 0).is_some());
        }
        let notice = br#"{"kind":"epoch","body":{"session":"session","world_epoch":2}}"#.to_vec();
        assert!(ingress.admit(notice, 0).is_some());
        assert!(!ingress.failed());
    }

    /// Builds a server notice with a stable session and a caller-selected epoch.
    fn world_notice(epoch: u64) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"kind":"epoch","body":{
            "session":"session", "world_epoch":epoch
        }}))
        .unwrap()
    }

    /// Echoes a notice while advancing to the same client world epoch.
    fn world_ack(epoch: u64) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"kind":"world_ack","body":{
            "session":"session", "world_epoch":epoch, "requested_epoch":epoch
        }}))
        .unwrap()
    }

    #[test]
    fn acknowledged_epochs_drop_equal_and_older_notices_without_spending_data_credits() {
        let mut ingress = ExperienceIngress::default();
        assert!(ingress.admit(world_notice(2), 0).is_some());
        ingress.written(world_ack(2));
        for _ in 0..server_experience::policy::MAX_MESSAGES_PER_SECOND {
            assert!(ingress.admit(world_notice(2), 0).is_none());
            assert!(ingress.admit(world_notice(1), 0).is_none());
            assert!(ingress.admit(b"data".to_vec(), 0).is_some());
        }
        // A late acknowledgment cannot move the admitted epoch backward.
        ingress.written(world_ack(1));
        assert!(ingress.admit(world_notice(2), 0).is_none());
        assert!(ingress.admit(world_notice(3), 0).is_some());
        assert!(!ingress.failed());
    }

    #[tokio::test]
    async fn coalesced_notice_is_acknowledged_before_the_previous_ack_write_completes() {
        use super::super::{
            NetworkCommand,
            outbound::{OutboundRuntime, run_outbound},
        };
        use std::sync::{Arc, Mutex};
        use tokio::sync::{mpsc, watch};

        struct Writer {
            gate: Arc<ExperienceGate>,
            commands: Option<mpsc::Sender<NetworkCommand>>,
            written: Arc<Mutex<Vec<u64>>>,
        }
        impl super::super::OutboundSession for Writer {
            type Error = &'static str;

            /// Delivers the coalesced notice while the previous flush is still unresolved.
            async fn send_batch(
                &mut self,
                packets: Vec<protocol::Packet>,
                _: Option<simulation_clock::Moment>,
            ) -> Result<(), Self::Error> {
                assert_eq!(packets.len(), 1);
                let epoch = protocol::experience_world_ack(&packets[0]).unwrap();
                self.written.lock().unwrap().push(epoch);
                if epoch == 2 {
                    let event = InboundWorldEvent::Event(WorldEvent::Experience(
                        protocol::ExperienceMessage::new(world_notice(3)),
                    ));
                    assert!(self.gate.admit(&event, &mut None, 0));
                    let commands = self.commands.take().unwrap();
                    commands
                        .try_send(NetworkCommand::Send {
                            packet: protocol::experience_packet(world_ack(3)).unwrap(),
                            sub_chunk: None,
                            chat: None,
                            physics: None,
                            physics_reanchor: None,
                            interaction: None,
                        })
                        .unwrap();
                    commands
                        .try_send(NetworkCommand::FlushFrame {
                            cache_opportunity: false,
                        })
                        .unwrap();
                }
                Ok(())
            }
        }

        let mut ingress = ExperienceIngress::default();
        assert!(ingress.admit(world_notice(2), 0).is_some());
        let gate = Arc::new(ingress.gate);
        let written = Arc::new(Mutex::new(Vec::new()));
        let (commands, receiver) = mpsc::channel(2);
        commands
            .try_send(NetworkCommand::Send {
                packet: protocol::experience_packet(world_ack(2)).unwrap(),
                sub_chunk: None,
                chat: None,
                physics: None,
                physics_reanchor: None,
                interaction: None,
            })
            .unwrap();
        commands
            .try_send(NetworkCommand::FlushFrame {
                cache_opportunity: false,
            })
            .unwrap();
        let (hooks, _hook_rx) = mpsc::unbounded_channel();
        let (_shutdown, shutdown_rx) = watch::channel(false);
        run_outbound(OutboundRuntime {
            outbound: Writer {
                gate: Arc::clone(&gate),
                commands: Some(commands),
                written: Arc::clone(&written),
            },
            commands: receiver,
            control_event_tx: mpsc::channel::<super::super::NetworkControlEvent>(1).0,
            hooks,
            shutdown_rx,
            experience_gate: Arc::clone(&gate),
            trace_line: |_: u64, _: &protocol::Packet| None::<String>,
            write_trace: |_: &str| {},
            fast_transfer_action_marker: None,
        })
        .await;
        assert_eq!(*written.lock().unwrap(), [2, 3]);
        let event = InboundWorldEvent::Event(WorldEvent::Experience(
            protocol::ExperienceMessage::new(world_notice(3)),
        ));
        assert!(!gate.admit(&event, &mut ingress.rate, 0));
        assert!(gate.enabled());
    }

    struct RecordingSession(std::sync::Arc<std::sync::Mutex<Vec<bytes::Bytes>>>);

    impl super::super::NetworkSession for RecordingSession {
        type Error = String;
        type Outbound = super::super::tests::PacketOutbound<String>;

        /// Captures the same generated encoding used by the ordinary transport.
        fn outbound(&mut self) -> Result<Self::Outbound, String> {
            let sent = std::sync::Arc::clone(&self.0);
            Ok(super::super::tests::PacketOutbound::new(move |packet| {
                sent.lock()
                    .unwrap()
                    .push(protocol::encode(&packet).unwrap());
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
        let expected: Vec<_> = packets
            .iter()
            .map(|packet| protocol::encode(packet).unwrap())
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
        commands
            .try_send(NetworkCommand::FlushFrame {
                cache_opportunity: false,
            })
            .unwrap();
        drop(commands);
        let (control, _control_rx) = mpsc::channel(4);
        let (world, _world_rx) = mpsc::channel(4);
        let (_shutdown, shutdown_rx) = watch::channel(false);
        run_network_pump(
            RecordingSession(std::sync::Arc::clone(&sent)),
            NetworkSequencer::new(simulation_clock::identity::SessionGeneration::new(1), 0, 1),
            receiver,
            control,
            world,
            shutdown_rx,
        )
        .await;
        assert_eq!(*sent.lock().unwrap(), expected);
    }
}
