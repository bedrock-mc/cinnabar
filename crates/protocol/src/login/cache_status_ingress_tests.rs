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
    let mut session = PlaySession::new(transport, Some(cache), 1, false);
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

/// A block update held behind a multi-column cached SubChunk whose pending owner names another
/// column must reach the owner's recovery. The drain used to find no blocker on the update's own
/// column and re-loop without ever yielding, so this polls on a watchdog thread.
#[test]
fn transitively_blocked_ordinary_work_never_spins_ingress() {
    use bedrock_protocol_1_26_51::enums::SubChunkRequestResult;
    use bedrock_protocol_1_26_51::packets::{SubChunk, UpdateBlock};
    use bedrock_protocol_1_26_51::types::{
        BlockPos, DimensionType, SubChunkData, SubChunkPos, SubChunkPosOffset,
    };

    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let recovered = runtime.block_on(async {
            let cache = ClientBlobCache::default();
            let resident = cache.insert(b"resident sub-chunk").unwrap();
            let mut owner =
                super::tests::cached_level_chunk(vec![crate::client_blob_hash(b"missing column")]);
            owner.chunk_position.x = wire::ZigZag32(7);
            owner.chunk_position.z = wire::ZigZag32(13);
            let entry = |x, z| SubChunkData {
                sub_chunk_pos_offset: SubChunkPosOffset {
                    subchunk_offset_x: wire::I8(x),
                    subchunk_offset_y: wire::I8(0),
                    subchunk_offset_z: wire::I8(z),
                },
                sub_chunk_request_result: SubChunkRequestResult::Success,
                serialized_sub_chunk: Some(Bytes::new()),
                blob_id: Some(wire::U64LE(resident)),
                ..Default::default()
            };
            let reply = SubChunk {
                cache_enabled: true,
                dimension_type: DimensionType(0),
                center_pos: SubChunkPos {
                    subchunk_position_x: wire::I32LE(4),
                    subchunk_position_y: wire::I32LE(0),
                    subchunk_position_z: wire::I32LE(9),
                },
                sub_chunk_data: vec![entry(0, 0), entry(3, 4)],
            };
            let update = UpdateBlock {
                block_position: BlockPos {
                    x: wire::ZigZag32(4 * 16),
                    y: wire::ZigZag32(0),
                    z: wire::ZigZag32(9 * 16),
                },
                block_runtime_id: wire::VarUInt(1),
                layer: wire::VarUInt(0),
                ..Default::default()
            };
            let startup: Vec<_> = [Packet::new(owner), Packet::new(reply), Packet::new(update)]
                .iter()
                .map(crate::session_transport::tests::packet_bytes)
                .collect();
            let (reader, frames, _writer) = bridge::tasks::prepare_connection(
                tokio::io::empty(),
                tokio::io::sink(),
                bridge::MAX_FRAME_LEN,
            );
            let transport = SessionTransport::new(reader, frames, &startup);
            let mut session = PlaySession::new(transport, Some(cache), 1, false);
            let waker = noop_waker();
            let mut cx = Context::from_waker(&waker);
            for _ in 0..8 {
                let mut receive = std::pin::pin!(session.recv_world_ingress_with_blob_cache(0));
                match receive.as_mut().poll(&mut cx) {
                    Poll::Ready(Ok(WorldIngress::Event(WorldEvent::ChunkResync(resync)))) => {
                        return Some((resync.x, resync.z));
                    }
                    Poll::Ready(Ok(_)) => {}
                    Poll::Ready(Err(_)) | Poll::Pending => return None,
                }
            }
            None
        });
        let _ = done.send(recovered);
    });
    let recovered = finished
        .recv_timeout(std::time::Duration::from_secs(20))
        .expect("ingress spun without yielding while ordinary work was transitively blocked");
    assert_eq!(recovered, Some((7, 13)));
}
