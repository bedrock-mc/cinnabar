use super::{BatchSendError, NetworkCommand, NetworkHandle};

impl<P> NetworkHandle<P> {
    /// Whether a committed echo still awaits command FIFO capacity.
    pub fn has_pending_latency_reply(&self) -> bool {
        self.pending_latency_reply
            .lock()
            .expect("latency reply lock")
            .is_some()
    }

    /// Retains one committed echo until the outbound FIFO has room.
    pub fn send_latency_reply(&self, creation_time: u64) -> Result<(), BatchSendError> {
        let mut pending = self
            .pending_latency_reply
            .lock()
            .expect("latency reply lock");
        self.flush_latency_reply_locked(&mut pending)?;
        *pending = Some(protocol::network_stack_latency_reply(creation_time));
        match self.flush_latency_reply_locked(&mut pending) {
            Err(BatchSendError::Full) => Ok(()),
            result => result,
        }
    }

    /// Admits the committed echo before any later outbound packet.
    pub fn flush_latency_reply(&self) -> Result<(), BatchSendError> {
        let mut pending = self
            .pending_latency_reply
            .lock()
            .expect("latency reply lock");
        self.flush_latency_reply_locked(&mut pending)
    }

    /// Admits an echo under the same lock used to retain the next one.
    fn flush_latency_reply_locked(
        &self,
        pending: &mut Option<protocol::Packet>,
    ) -> Result<(), BatchSendError> {
        if pending.is_none() {
            return Ok(());
        }
        let mut frame = self.unflushed.lock().expect("outbound frame lock");
        let permit = match frame.reserve(&self.commands, 1) {
            Ok(mut permits) => permits.next().expect("one reserved command"),
            Err(BatchSendError::Full) => {
                return Err(BatchSendError::Full);
            }
            Err(BatchSendError::Closed) => {
                *pending = None;
                return Err(BatchSendError::Closed);
            }
        };
        permit.send(NetworkCommand::Send {
            packet: pending.take().expect("pending latency reply"),
            sub_chunk: None,
            chat: None,
            physics: None,
            physics_reanchor: None,
            interaction: None,
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    type NetworkHandle = super::NetworkHandle<()>;
    use crate::PacketSendError;
    use tokio::sync::mpsc;

    /// Creates one observable outbound FIFO slot.
    fn handle() -> (NetworkHandle, mpsc::Receiver<NetworkCommand>) {
        let (mut handle, _) = NetworkHandle::stub();
        let (commands, receiver) = super::super::frame_flush::command_channel(1);
        handle.commands = commands;
        (handle, receiver)
    }

    /// Checks the next packet's bytes against the expected probe identity.
    fn assert_probe(receiver: &mut mpsc::Receiver<NetworkCommand>, timestamp: u64) {
        let NetworkCommand::Send { packet, .. } = receiver.try_recv().unwrap() else {
            panic!("expected a latency reply, received loading completion");
        };
        assert_eq!(
            protocol::encode(&packet).unwrap(),
            protocol::encode(&protocol::network_stack_latency_reply(timestamp)).unwrap(),
        );
    }

    /// Consecutive replies share the frame with packets on either side, in admission order.
    #[test]
    fn latency_replies_share_the_ordinary_outbound_frame() {
        let (handle, mut captured) = NetworkHandle::stub_capturing_packets();
        let expected = [
            protocol::modal_form_cancel_response(1),
            protocol::network_stack_latency_reply(7),
            protocol::network_stack_latency_reply(8),
            protocol::modal_form_cancel_response(2),
        ];
        handle.send_packet(expected[0].clone()).unwrap();
        handle.send_latency_reply(7).unwrap();
        handle.send_latency_reply(8).unwrap();
        handle.send_packet(expected[3].clone()).unwrap();
        assert!(
            captured.take_flushed_frame().is_none(),
            "latency reply split the frame"
        );
        handle.flush_frame();
        let sent = captured.take_flushed_frame().expect("frame was flushed");
        let encode = |packets: &[protocol::Packet]| {
            packets
                .iter()
                .map(|packet| protocol::encode(packet).unwrap())
                .collect::<Vec<_>>()
        };
        assert_eq!(encode(&sent), encode(&expected));
        assert!(captured.take_flushed_frame().is_none());
    }

    #[test]
    fn two_latency_fences_and_later_packets_keep_fifo_order_under_backpressure() {
        let (handle, mut receiver) = handle();
        handle
            .send_packet(protocol::network_stack_latency_reply(1))
            .unwrap();
        handle.send_latency_reply(2).unwrap();
        assert_eq!(handle.pending_command_count(), 2);
        assert!(matches!(
            handle.send_latency_reply(3),
            Err(BatchSendError::Full)
        ));
        assert!(matches!(
            handle.send_packet(protocol::network_stack_latency_reply(4)),
            Err(PacketSendError::Full(_))
        ));
        assert_probe(&mut receiver, 1);
        handle.send_latency_reply(3).unwrap();
        assert!(matches!(
            handle.send_packet(protocol::network_stack_latency_reply(4)),
            Err(PacketSendError::Full(_))
        ));
        assert_probe(&mut receiver, 2);
        handle.flush_frame();
        assert_probe(&mut receiver, 3);
        assert!(matches!(
            receiver.try_recv().unwrap(),
            NetworkCommand::FlushFrame {
                cache_opportunity: false
            }
        ));
        handle
            .send_packet(protocol::network_stack_latency_reply(4))
            .unwrap();
        assert_probe(&mut receiver, 4);
        assert_eq!(handle.pending_command_count(), 0);
    }

    /// A full frame submits its reserved delimiter without losing the retained reply.
    #[test]
    fn full_frame_keeps_a_latency_reply_for_the_next_frame() {
        let (handle, mut receiver) = handle();
        handle
            .send_packet(protocol::network_stack_latency_reply(1))
            .unwrap();
        handle.send_latency_reply(2).unwrap();
        handle.flush_frame();
        assert!(handle.has_pending_latency_reply());
        assert_probe(&mut receiver, 1);
        assert!(matches!(
            receiver.try_recv().unwrap(),
            NetworkCommand::FlushFrame {
                cache_opportunity: false
            }
        ));
        handle.flush_frame();
        assert!(!handle.has_pending_latency_reply());
        assert_probe(&mut receiver, 2);
        assert!(matches!(
            receiver.try_recv().unwrap(),
            NetworkCommand::FlushFrame {
                cache_opportunity: false
            }
        ));
        assert_eq!(handle.pending_command_count(), 0);
    }

    #[test]
    fn review_concurrent_latency_fences_do_not_overwrite_retained_replies() {
        for _ in 0..128 {
            let (handle, _receiver) = handle();
            handle
                .send_packet(protocol::network_stack_latency_reply(1))
                .unwrap();
            let barrier = std::sync::Barrier::new(16);
            let admitted = std::thread::scope(|scope| {
                let handles: Vec<_> = (2..18)
                    .map(|timestamp| {
                        let handle = &handle;
                        let barrier = &barrier;
                        scope.spawn(move || {
                            barrier.wait();
                            handle.send_latency_reply(timestamp).is_ok()
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .map(|thread| thread.join().unwrap())
                    .filter(|admitted| *admitted)
                    .count()
            });
            assert_eq!(admitted, 1, "a full FIFO retains only one admitted reply");
        }
    }

    #[test]
    fn closing_or_replacing_a_session_drops_the_retained_echo() {
        let (mut handle, receiver) = handle();
        handle
            .send_packet(protocol::network_stack_latency_reply(1))
            .unwrap();
        handle.send_latency_reply(2).unwrap();
        drop(receiver);
        assert!(matches!(
            handle.flush_latency_reply(),
            Err(BatchSendError::Closed)
        ));
        assert!(handle.pending_latency_reply.lock().unwrap().is_none());
        handle.shutdown();
        assert!(
            NetworkHandle::disconnected()
                .pending_latency_reply
                .lock()
                .unwrap()
                .is_none()
        );
    }
}
