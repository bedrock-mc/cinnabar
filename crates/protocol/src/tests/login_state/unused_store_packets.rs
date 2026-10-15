//! Unused store traffic remains wire-checked and does not interrupt ordinary play.

use super::*;
use packets::{PurchaseReceipt, ServerStoreInfo, ShowStoreOffer};

const SENTINEL_TIME: i32 = 76_543;
const DRAIN_LIMIT: usize = 64;

#[tokio::test]
async fn unused_store_packets_are_counted_without_interrupting_receivers() {
    for cache_enabled in [false, true] {
        for world_receiver in [false, true] {
            let (mut session, core) = joined(Script {
                cache_enabled,
                ..Script::default()
            })
            .await;
            // Raw `recv` rejects the scripted out-of-bounds LevelChunk that world ingress skips,
            // so consume the scripted play traffic before the store packets.
            while !matches!(
                session.recv_world_event(0).await.unwrap(),
                WorldEvent::SetTime(protocol::SetTimeEvent {
                    time: SCRIPTED_PLAY_END_TIME
                })
            ) {}
            core.send_batch(&[
                Packet::new(ShowStoreOffer::default()),
                Packet::new(PurchaseReceipt::default()),
                Packet::new(ServerStoreInfo::default()),
                Packet::new(SetTime {
                    time: wire::ZigZag32(SENTINEL_TIME),
                }),
            ]);
            let mut reached = false;
            for _ in 0..DRAIN_LIMIT {
                reached = if world_receiver {
                    matches!(
                        session.recv_world_event(0).await.unwrap(),
                        WorldEvent::SetTime(protocol::SetTimeEvent {
                            time: SENTINEL_TIME
                        })
                    )
                } else {
                    matches!(&*session.recv().await.unwrap().data,
                        packets::Packet::SetTime(packet) if packet.time.0 == SENTINEL_TIME)
                };
                if reached {
                    break;
                }
            }
            assert!(reached, "ordinary traffic follows unused store packets");
            assert_eq!(session.unused_packet_count(), 3);
            assert_eq!(session.decode_error_count(), 0);
            core.finish(session).await;
        }
    }
}

#[tokio::test]
async fn malformed_unused_store_packets_remain_fatal() {
    for cache_enabled in [false, true] {
        for id in [
            PacketId::ShowStoreOffer,
            PacketId::PurchaseReceipt,
            PacketId::ServerStoreInfo,
        ] {
            let (mut session, core) = joined(Script {
                cache_enabled,
                ..Script::default()
            })
            .await;
            core.send_raw_packet(id, &[]);
            let mut failed = false;
            for _ in 0..DRAIN_LIMIT {
                if let Err(error) = session.recv_world_event(0).await {
                    assert!(matches!(error, ProtocolError::Session(_)));
                    failed = true;
                    break;
                }
            }
            assert!(failed, "truncated unused packet must fail wire validation");
            assert_eq!(session.unused_packet_count(), 0);
            assert_eq!(session.decode_error_count(), 1);
            core.finish(session).await;
        }
    }
}
