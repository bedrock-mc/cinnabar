//! Actual default desktop driver admission while its first larger body is retained.

use super::*;
use crate::session_transport::Inbound;
use bedrock_protocol_1_26_51::packets;
use std::sync::atomic::{AtomicUsize, Ordering};

#[tokio::test]
async fn ordinary_desktop_admission_holds_one_oversized_generated_body() {
    let capacity = bridge::contract::RELAY_QUEUED_FRAME_BYTES;
    let body =
        crate::session_transport::batch_body(&[crate::Packet::new(packets::ScriptMessage {
            message_id: "byte-admission".into(),
            message_value: vec![1; capacity].into(),
        })])
        .unwrap();
    let body_len = body.len();
    assert!(body_len > capacity);
    assert!(body_len < bedrock_net::BatchLimits::default().max_decoded_bytes);
    let reads = Arc::new(AtomicUsize::new(0));
    let (staged, first_staged) = oneshot::channel();
    let (observation, observed) = oneshot::channel();
    let (closed, actual_close) = oneshot::channel();
    let mut transport = SessionTransport::from_native(ObservedConnection {
        body,
        reads: Arc::clone(&reads),
        first_staged: Some(first_staged),
        observation: Some(observation),
        closed: Some(closed),
    });

    transport.read_ahead(body_len).await;
    let Inbound::Packet(retained) = transport.recv().await.unwrap() else {
        panic!("expected the original generated packet")
    };
    let (_, packets::Packet::ScriptMessage(decoded)) = retained.clone().decode_parts().unwrap()
    else {
        panic!("expected the original ScriptMessage")
    };
    assert_eq!(decoded.message_id.to_str().unwrap(), "byte-admission");
    assert_eq!(decoded.message_value.len(), capacity);
    drop(decoded);
    // The first body is now committed and held by its actual RawPacket, not pending in the pump.
    staged.send(()).unwrap();
    let observation = observed.await.unwrap();

    transport.close().await.unwrap();
    actual_close.await.unwrap();
    drop(retained);
    drop(transport);
    let actual_reads = reads.load(Ordering::SeqCst);
    drop(reads);

    assert_eq!(
        observation,
        Admission::MaintainedFirstBody,
        "default desktop read ahead instead of maintaining its retained oversized body"
    );
    assert_eq!(
        actual_reads, 1,
        "only the original oversized body is admitted"
    );
}

/// The actual I/O boundary reached while the original oversized packet is held.
#[derive(Debug, Eq, PartialEq)]
enum Admission {
    ThirdRead,
    MaintainedFirstBody,
}

/// Offers bounded clones and reports an actual third read or first-body maintenance once.
struct ObservedConnection {
    body: Bytes,
    reads: Arc<AtomicUsize>,
    first_staged: Option<oneshot::Receiver<()>>,
    observation: Option<oneshot::Sender<Admission>>,
    closed: Option<oneshot::Sender<()>>,
}

impl PacketConnection for ObservedConnection {
    async fn send_body(&mut self, _: Bytes) -> io::Result<()> {
        Ok(())
    }

    async fn recv_body(&mut self) -> io::Result<Bytes> {
        let read = self.reads.fetch_add(1, Ordering::SeqCst) + 1;
        if read < 3 {
            return Ok(self.body.clone());
        }
        if let Some(observation) = self.observation.take() {
            let _ = observation.send(Admission::ThirdRead);
        }
        std::future::pending().await
    }

    async fn maintain(&mut self) {
        if let Some(first_staged) = self.first_staged.as_mut() {
            let _ = first_staged.await;
            self.first_staged = None;
        }
        // Transient pending-input maintenance is not evidence of byte gating.
        if self.reads.load(Ordering::SeqCst) == 1
            && let Some(observation) = self.observation.take()
        {
            let _ = observation.send(Admission::MaintainedFirstBody);
        }
        std::future::pending::<()>().await;
    }

    async fn close(&mut self) -> io::Result<()> {
        if let Some(closed) = self.closed.take() {
            let _ = closed.send(());
        }
        Ok(())
    }
}
