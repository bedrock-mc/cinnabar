use std::future::Future;
use std::task::{Context, Poll};

use bedrock_protocol_1_26_51::packets::SetTime;
use futures::task::noop_waker;

use super::*;

/// Ready cached terrain and later ordinary ingress do not wait for status socket writes.
#[tokio::test]
async fn cache_status_aggregation_does_not_hold_ready_ingress_for_writes() {
    let cache = ClientBlobCache::default();
    let hash = cache.insert(b"body").unwrap();
    let first = super::tests::cached_level_chunk(vec![hash, hash]);
    let mut second = super::tests::cached_level_chunk(vec![hash]);
    second.chunk_position.x = wire::ZigZag32(1);
    let startup: Vec<_> = [
        Packet::new(first),
        Packet::new(second),
        Packet::new(SetTime {
            time: wire::ZigZag32(42),
        }),
    ]
    .iter()
    .map(crate::session_transport::tests::packet_bytes)
    .collect();
    // Keeping the writer unpolled makes every attempted status flush stay pending.
    let (reader, frames, _writer) = bridge::tasks::prepare_connection(
        tokio::io::empty(),
        tokio::io::sink(),
        bridge::MAX_FRAME_LEN,
    );
    let transport = SessionTransport::new(reader, frames, &startup);
    let mut session = PlaySession::new(
        transport,
        Some(cache),
        crate::login::startup::ready_fixture(1),
    );
    let waker = noop_waker();
    let mut cx = Context::from_waker(&waker);
    for x in [0, 1] {
        let mut receive = std::pin::pin!(session.recv_world_ingress_with_blob_cache(0));
        let Poll::Ready(Ok(WorldIngress::LevelChunk(event, _))) = receive.as_mut().poll(&mut cx)
        else {
            panic!("ready cached column must precede any status write completion");
        };
        assert_eq!(event.x, x);
    }
    let mut receive = std::pin::pin!(session.recv_world_ingress_with_blob_cache(0));
    assert!(matches!(
        receive.as_mut().poll(&mut cx),
        Poll::Ready(Ok(WorldIngress::Event(WorldEvent::SetTime(_))))
    ));
}
