use std::sync::Arc;

use bedrock_protocol_1_26_51::packets::{PacketId, SetTime};
use bedrock_protocol_1_26_51::wire;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixListener;

use super::PlaySession;
use crate::packet_tap::{PacketDirection, PacketOutcome};
use crate::session_transport::tests::{RecordingTap, TapEvent};
use crate::session_transport::{SessionTransport, batch_frame};
use crate::{Packet, ResourcePackHandoff, WorldEvent};

/// Play reports the malformed packet that ended the session, with its body, and nothing for
/// the packets it accepted.
#[tokio::test]
async fn a_malformed_world_packet_is_reported_failed_with_its_body() {
    let dir = tempfile::tempdir().unwrap();
    let listener = UnixListener::bind(bridge::session_endpoint_path(dir.path())).unwrap();
    let (reader, frames) = bridge::connect_session(dir.path()).await.unwrap();
    let (mut peer, _) = listener.accept().await.unwrap();
    let tap = Arc::new(RecordingTap::default());
    let transport = SessionTransport::new(reader, frames, &[]).with_tap(Some(tap.clone()));
    let mut session = PlaySession::new(transport, None, ResourcePackHandoff::default(), 1, false);

    let mut frame = batch_frame(&[Packet::new(SetTime {
        time: wire::ZigZag32(5),
    })])
    .unwrap()
    .to_vec();
    // A SetTime entry whose body is missing its time.
    frame.extend([1, PacketId::SetTime as u8]);
    peer.write_u32(frame.len() as u32).await.unwrap();
    peer.write_all(&frame).await.unwrap();

    assert!(matches!(
        session.recv_world_event(0).await.unwrap(),
        WorldEvent::SetTime(_)
    ));
    assert!(session.recv_world_event(0).await.is_err());
    assert_eq!(session.decode_error_count(), 1);
    let set_time = PacketId::SetTime as u32;
    let events = tap.events();
    assert!(matches!(
        &events[..2],
        [
            TapEvent::Record(PacketDirection::Clientbound, first, _),
            TapEvent::Record(PacketDirection::Clientbound, second, body),
        ] if *first == set_time && *second == set_time && body.is_empty()
    ));
    assert_eq!(
        events[2..],
        [TapEvent::Reject(
            1,
            set_time,
            PacketOutcome::Failed,
            Vec::new()
        )]
    );
}
