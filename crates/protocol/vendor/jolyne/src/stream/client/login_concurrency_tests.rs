use super::*;
use crate::batch::{decode_batch, encode_batch_multi};
use crate::stream::transport::{TransportMessage, TransportRecvMessage};
use bytes::Bytes;
use std::future::{Future, poll_fn};
use std::io;
use std::pin::Pin;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::task::{Context, Poll};

#[derive(Default)]
struct Gate {
    released: AtomicBool,
    sent: Mutex<Vec<TransportMessage>>,
    transport_drops: AtomicUsize,
}

struct SettingsTransport {
    gate: Arc<Gate>,
    reply: Option<Bytes>,
    fail_receive: bool,
}

impl Drop for SettingsTransport {
    fn drop(&mut self) {
        self.gate.transport_drops.fetch_add(1, Ordering::SeqCst);
    }
}

impl Transport for SettingsTransport {
    type Error = io::Error;
    const USES_BATCH_PREFIX: bool = true;

    fn poll_send(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        message: TransportMessage,
    ) -> Poll<Result<(), Self::Error>> {
        self.gate.sent.lock().unwrap().push(message);
        Poll::Ready(Ok(()))
    }

    fn poll_recv(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<Result<TransportRecvMessage, Self::Error>>> {
        if !self.gate.released.load(Ordering::SeqCst) {
            return Poll::Pending;
        }
        if self.fail_receive {
            return Poll::Ready(Some(Err(io::Error::other("settings failed"))));
        }
        Poll::Ready(
            self.reply
                .take()
                .map(|reply| Ok(TransportRecvMessage::Contiguous(reply))),
        )
    }

    fn peer_addr(&self) -> SocketAddr {
        "127.0.0.1:0".parse().unwrap()
    }
}

fn stream(
    gate: Arc<Gate>,
    fail_receive: bool,
) -> BedrockStream<Handshake, Client, SettingsTransport> {
    let settings = McpePacket::from(crate::valentine::NetworkSettingsPacket {
        compression_algorithm: NetworkSettingsPacketCompressionAlgorithm::None,
        ..Default::default()
    });
    let reply = encode_batch_multi(&[settings], false, 0, 0, true).unwrap();
    BedrockStream::from_transport(BedrockTransport::new(SettingsTransport {
        gate,
        reply: Some(reply),
        fail_receive,
    }))
}

fn sent_ids(gate: &Gate) -> Vec<McpePacketName> {
    gate.sent
        .lock()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(index, message)| {
            let mut bytes = message.buffer.clone();
            let packets = decode_batch(
                &mut bytes,
                &valentine::bedrock::context::BedrockSession { shield_item_id: 0 },
                index != 0,
                None,
            )
            .unwrap();
            assert_eq!(packets.len(), 1);
            packets[0].header.id
        })
        .collect()
}

#[tokio::test]
async fn login_payload_prepares_while_settings_are_pending_and_sends_after_them() {
    let gate = Arc::new(Gate::default());
    let prepared = Arc::new(AtomicBool::new(false));
    let observation = Arc::clone(&prepared);
    let prepare = async move {
        observation.store(true, Ordering::SeqCst);
        Ok(LoginPacket {
            client_network_version: crate::valentine::PROTOCOL_VERSION,
            connection_request: vec![1, 2, 3],
        })
    };
    let mut negotiation =
        Box::pin(stream(Arc::clone(&gate), false).prepare_during_settings(prepare));
    assert!(
        poll_fn(|cx| Poll::Ready(negotiation.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    assert!(prepared.load(Ordering::SeqCst));
    assert_eq!(
        sent_ids(&gate),
        [McpePacketName::RequestNetworkSettingsPacket]
    );

    gate.released.store(true, Ordering::SeqCst);
    let (login, packet) = negotiation.await.unwrap();
    let _secure = login.send_prepared_login(packet).await.unwrap();
    assert_eq!(
        sent_ids(&gate),
        [
            McpePacketName::RequestNetworkSettingsPacket,
            McpePacketName::LoginPacket
        ]
    );
}

struct DropCounter(Arc<AtomicUsize>);

impl Drop for DropCounter {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn pending_preparation(
    drops: Arc<AtomicUsize>,
) -> impl Future<Output = Result<LoginPacket, JolyneError>> {
    let guard = DropCounter(drops);
    async move {
        let _guard = guard;
        std::future::pending().await
    }
}

#[tokio::test]
async fn settings_failure_cancels_preparation_without_sending_login() {
    let gate = Arc::new(Gate::default());
    gate.released.store(true, Ordering::SeqCst);
    let drops = Arc::new(AtomicUsize::new(0));
    let result = stream(Arc::clone(&gate), true)
        .prepare_during_settings(pending_preparation(Arc::clone(&drops)))
        .await;
    assert!(result.is_err());
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(gate.transport_drops.load(Ordering::SeqCst), 1);
    assert_eq!(
        sent_ids(&gate),
        [McpePacketName::RequestNetworkSettingsPacket]
    );
}

#[tokio::test]
async fn preparation_failure_cancels_settings_without_sending_login() {
    let gate = Arc::new(Gate::default());
    let mut negotiation = Box::pin(
        stream(Arc::clone(&gate), false)
            .prepare_during_settings(async { Err(pack_handoff_error("fixture")) }),
    );
    let result = poll_fn(|cx| Poll::Ready(negotiation.as_mut().poll(cx))).await;
    assert!(matches!(result, Poll::Ready(Err(_))));
    drop(negotiation);
    assert_eq!(gate.transport_drops.load(Ordering::SeqCst), 1);
    assert_eq!(
        sent_ids(&gate),
        [McpePacketName::RequestNetworkSettingsPacket]
    );
}

#[tokio::test]
async fn owner_cancellation_drops_settings_and_preparation_without_sending_login() {
    let gate = Arc::new(Gate::default());
    let drops = Arc::new(AtomicUsize::new(0));
    let mut negotiation = Box::pin(
        stream(Arc::clone(&gate), false)
            .prepare_during_settings(pending_preparation(Arc::clone(&drops))),
    );
    assert!(
        poll_fn(|cx| Poll::Ready(negotiation.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    drop(negotiation);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(gate.transport_drops.load(Ordering::SeqCst), 1);
    assert_eq!(
        sent_ids(&gate),
        [McpePacketName::RequestNetworkSettingsPacket]
    );
}
