use std::sync::atomic::{AtomicU64, Ordering};

use super::*;
use crate::LoadingScreenPhase;
use crate::session_transport::tests::{RecordingTap, TapEvent};

/// Counts headers without allocating.
#[derive(Default)]
struct Counted {
    packets: AtomicU64,
    bytes: AtomicU64,
}

impl PacketTap for Counted {
    fn record(&self, _: PacketDirection, _: u32, body: PacketBody<'_>) -> PacketRecordId {
        self.bytes.fetch_add(body.len() as u64, Ordering::Relaxed);
        PacketRecordId(self.packets.fetch_add(1, Ordering::Relaxed))
    }

    fn reject(&self, _: PacketRecordId, _: u32, _: PacketOutcome, _: PacketBody<'_>) {}

    fn reject_batch(&self, _: usize) {}
}

fn sent() -> [crate::Packet; 2] {
    [
        crate::request_chunk_radius_packet(16, 16),
        crate::loading_screen_packet(LoadingScreenPhase::Start, None),
    ]
}

#[test]
fn serverbound_frames_report_each_encoded_packet_once_in_order() {
    let frame = crate::session_transport::batch_frame(&sent()).unwrap();
    let tap = RecordingTap::default();
    record_serverbound(&tap, &frame);
    let expected: Vec<_> = crate::raw_packet::decode_batch_body(frame.slice(1..), usize::MAX)
        .unwrap()
        .into_iter()
        .map(|raw| {
            TapEvent::Record(
                PacketDirection::Serverbound,
                raw.id as u32,
                raw.body().to_vec(),
            )
        })
        .collect();
    assert_eq!(expected.len(), 2);
    assert_eq!(tap.events(), expected);
}

#[test]
fn serverbound_recording_reads_headers_without_allocating() {
    let frame = crate::session_transport::batch_frame(&sent()).unwrap();
    let tap = Counted::default();
    let ((), allocations) = test_alloc::count(|| record_serverbound(&tap, &frame));
    assert_eq!(allocations, 0);
    assert_eq!(tap.packets.load(Ordering::Relaxed), 2);
}

#[test]
fn wire_values_round_trip_and_reject_unknown_values() {
    for direction in PacketDirection::ALL {
        assert_eq!(PacketDirection::from_wire(direction as u8), Some(direction));
    }
    for outcome in PacketOutcome::ALL {
        assert_eq!(PacketOutcome::from_wire(outcome as u8), Some(outcome));
    }
    assert_eq!(PacketDirection::from_wire(2), None);
    assert_eq!(PacketOutcome::from_wire(3), None);
}

#[test]
fn secret_bearing_packets_are_named_and_others_are_not() {
    for id in [
        PacketId::Login,
        PacketId::ServerToClientHandshake,
        PacketId::ResourcePacksInfo,
        PacketId::SubClientLogin,
    ] {
        assert!(carries_secrets(id as u32), "{id:?}");
    }
    for id in [
        PacketId::StartGame,
        PacketId::ResourcePackStack,
        PacketId::LevelChunk,
        PacketId::Text,
    ] {
        assert!(!carries_secrets(id as u32), "{id:?}");
    }
}

#[test]
fn packet_names_come_from_the_pinned_protocol() {
    assert_eq!(
        PacketName(PacketId::StartGame as u32).to_string(),
        "StartGame"
    );
    assert_eq!(PacketName(UNFRAMED_BATCH_ID).to_string(), "UnframedBatch");
    assert_eq!(PacketName(0x3ff).to_string(), "Unknown(1023)");
}
