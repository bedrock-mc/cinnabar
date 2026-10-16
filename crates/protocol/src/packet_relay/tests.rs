use std::collections::VecDeque;
use std::sync::{Mutex as StdMutex, PoisonError};

use bedrock_protocol_1_26_51::packets::{self, SetTime};
use bedrock_protocol_1_26_51::wire;
use tokio::io::AsyncReadExt;

use super::*;
use crate::Packet;
use crate::session_transport::tests::unix::{Pair, pair, write_frame};
use crate::session_transport::{Inbound, batch_frame};

/// A relay that records what it sees and answers from scripted replies; an empty script forwards.
#[derive(Default)]
pub(crate) struct FakeRelay {
    pub(crate) state: StdMutex<FakeState>,
}

#[derive(Default)]
pub(crate) struct FakeState {
    pub(crate) started: Option<(SessionIdentity, String, Vec<u8>)>,
    pub(crate) packs: Vec<(Vec<u8>, bool)>,
    pub(crate) inbound: Vec<Vec<u8>>,
    pub(crate) startup_inbound: Vec<Vec<u8>>,
    pub(crate) inbound_replies: VecDeque<Option<Vec<u8>>>,
    pub(crate) outbound: Vec<Vec<u8>>,
    pub(crate) outbound_replies: VecDeque<Option<Vec<u8>>>,
    pub(crate) ticks: VecDeque<RelayOutput>,
    pub(crate) stopped: bool,
}

impl FakeRelay {
    pub(crate) fn state(&self) -> std::sync::MutexGuard<'_, FakeState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl PacketRelay for FakeRelay {
    fn start(
        &self,
        identity: &SessionIdentity,
        client_data: &str,
        startup: &[u8],
    ) -> Vec<RelayPack> {
        let mut state = self.state();
        state.started = Some((identity.clone(), client_data.to_owned(), startup.to_vec()));
        state
            .packs
            .iter()
            .map(|(archive, above_server)| RelayPack {
                archive: archive.clone(),
                above_server: *above_server,
            })
            .collect()
    }
    fn inbound_startup(&self, body: &[u8]) -> Option<Vec<u8>> {
        self.state().startup_inbound.push(body.to_vec());
        self.inbound(body)
    }

    fn inbound(&self, body: &[u8]) -> Option<Vec<u8>> {
        let mut state = self.state();
        state.inbound.push(body.to_vec());
        state.inbound_replies.pop_front().flatten()
    }
    fn outbound(&self, body: &[u8]) -> Option<Vec<u8>> {
        let mut state = self.state();
        state.outbound.push(body.to_vec());
        state.outbound_replies.pop_front().flatten()
    }
    fn tick(&self) -> RelayOutput {
        self.state().ticks.pop_front().unwrap_or_default()
    }
    fn stop(&self) {
        self.state().stopped = true;
    }
}

fn set_time(time: i32) -> Packet {
    Packet::new(SetTime {
        time: wire::ZigZag32(time),
    })
}

/// The batch body carrying `times` as SetTime packets.
pub(crate) fn body(times: &[i32]) -> Vec<u8> {
    let packets: Vec<_> = times.iter().copied().map(set_time).collect();
    batch_frame(&packets).unwrap()[1..].to_vec()
}

async fn relayed_pair(name: &str, relay: &Arc<FakeRelay>) -> Pair {
    let mut pair = pair(name, &[]).await;
    pair.transport.set_relay(relay.clone());
    pair.transport.finish_startup();
    pair
}

/// Reads the SetTime values of the next `count` packets.
async fn next_times(pair: &mut Pair, count: usize) -> Vec<i32> {
    let mut times = Vec::new();
    for _ in 0..count {
        let Inbound::Packet(raw) = pair.transport.recv().await.unwrap() else {
            panic!("a packet arrives before any terminal message")
        };
        let packets::Packet::SetTime(time) = *raw.decode().unwrap().data else {
            panic!("only SetTime packets are relayed")
        };
        times.push(time.time.0);
    }
    times
}

/// Reads one frame the transport wrote to the core.
async fn read_frame(pair: &mut Pair) -> Vec<u8> {
    let length = pair.peer.read_u32().await.unwrap();
    let mut frame = vec![0; length as usize];
    pair.peer.read_exact(&mut frame).await.unwrap();
    frame
}

/// Server batches pass through, are replaced or dropped, and a replacement that does not decode
/// is dropped without ending the session.
#[tokio::test]
async fn server_batches_pass_through_get_replaced_or_dropped() {
    let relay = Arc::new(FakeRelay::default());
    relay.state().inbound_replies = [
        None,
        Some(body(&[7])),
        Some(Vec::new()),
        Some(vec![0xff]),
        None,
    ]
    .into();
    let mut pair = relayed_pair("relay-inbound", &relay).await;
    for time in 1..=5 {
        write_frame(&mut pair.peer, &batch_frame(&[set_time(time)]).unwrap()).await;
    }
    assert_eq!(next_times(&mut pair, 3).await, [1, 7, 5]);
    let seen = relay.state().inbound.clone();
    assert_eq!(seen, (1..=5).map(|time| body(&[time])).collect::<Vec<_>>());
}

/// Client batches reach the core as the relay leaves them; a dropped batch writes nothing.
#[tokio::test]
async fn client_batches_are_relayed_in_write_order() {
    let relay = Arc::new(FakeRelay::default());
    relay.state().outbound_replies = [Some(body(&[5])), Some(Vec::new()), None].into();
    let mut pair = relayed_pair("relay-outbound", &relay).await;
    for time in 1..=3 {
        pair.transport.send(&[set_time(time)]).await.unwrap();
    }
    for expected in [5, 3] {
        let mut frame = vec![bridge::contract::KIND_BATCH];
        frame.extend_from_slice(&body(&[expected]));
        assert_eq!(read_frame(&mut pair).await, frame);
    }
    let seen = relay.state().outbound.clone();
    assert_eq!(seen, (1..=3).map(|time| body(&[time])).collect::<Vec<_>>());
}

/// Timed work injects bodies both ways, and dropping the session stops the relay.
#[tokio::test]
async fn ticks_inject_both_ways_and_the_session_stops_the_relay() {
    let relay = Arc::new(FakeRelay::default());
    relay.state().ticks = [RelayOutput {
        to_client: body(&[8]),
        to_server: body(&[9]),
    }]
    .into();
    let mut pair = relayed_pair("relay-tick", &relay).await;
    assert_eq!(next_times(&mut pair, 1).await, [8]);
    let mut frame = vec![bridge::contract::KIND_BATCH];
    frame.extend_from_slice(&body(&[9]));
    assert_eq!(read_frame(&mut pair).await, frame);
    assert!(!relay.state().stopped);
    drop(pair);
    assert!(relay.state().stopped);
}

/// The startup body carries each packet behind its varuint32 length.
#[test]
fn startup_body_joins_packets_with_length_prefixes() {
    let packets = [Bytes::from_static(&[1, 2]), Bytes::from(vec![3; 200])];
    let body = batch_body(&packets);
    let mut expected = vec![2, 1, 2, 200, 1];
    expected.extend_from_slice(&[3; 200]);
    assert_eq!(&body[..], &expected[..]);
}

/// Replacement and dropped bodies are observed once at the same bytes the client and core consume.
#[tokio::test]
async fn taps_record_relay_replacements_and_detached_writes_once() {
    use crate::packet_tap::PacketDirection;
    use crate::session_transport::tests::unix::tapped_pair;
    use crate::session_transport::tests::{RecordingTap, TapEvent};

    let relay = Arc::new(FakeRelay::default());
    relay.state().inbound_replies = [Some(body(&[7]))].into();
    relay.state().outbound_replies = [Some(body(&[5])), Some(Vec::new()), None].into();
    let tap = Arc::new(RecordingTap::default());
    let mut pair = tapped_pair("relay-tap", &[], Some(tap.clone())).await;
    pair.transport.set_relay(relay);
    pair.transport.finish_startup();

    write_frame(&mut pair.peer, &batch_frame(&[set_time(1)]).unwrap()).await;
    assert_eq!(next_times(&mut pair, 1).await, [7]);
    pair.transport.send(&[set_time(1)]).await.unwrap();
    pair.transport.send(&[set_time(2)]).await.unwrap();
    pair.transport
        .batch_writer()
        .send(batch_frame(&[set_time(3)]).unwrap())
        .await
        .unwrap();
    for time in [5, 3] {
        assert_eq!(
            read_frame(&mut pair).await,
            batch_frame(&[set_time(time)]).unwrap()
        );
    }

    let expected = |direction, time| {
        let packet = crate::raw_packet::decode_batch_body(
            Bytes::from(body(&[time])),
            crate::session_transport::MAX_SESSION_BATCH_BYTES,
        )
        .unwrap()
        .pop()
        .unwrap();
        TapEvent::Record(direction, packet.id as u32, packet.body().to_vec())
    };
    assert_eq!(
        tap.events(),
        [
            expected(PacketDirection::Clientbound, 7),
            expected(PacketDirection::Serverbound, 5),
            expected(PacketDirection::Serverbound, 3),
        ],
    );
}

/// Tick injections reach both observers even though they do not traverse the replacement callbacks.
#[tokio::test]
async fn taps_record_relay_tick_injections_in_both_directions() {
    use crate::packet_tap::PacketDirection;
    use crate::session_transport::tests::unix::tapped_pair;
    use crate::session_transport::tests::{RecordingTap, TapEvent};

    let relay = Arc::new(FakeRelay::default());
    relay.state().ticks = [RelayOutput {
        to_client: body(&[8]),
        to_server: body(&[9]),
    }]
    .into();
    let tap = Arc::new(RecordingTap::default());
    let mut pair = tapped_pair("relay-tick-tap", &[], Some(tap.clone())).await;
    pair.transport.set_relay(relay);
    pair.transport.finish_startup();
    assert_eq!(next_times(&mut pair, 1).await, [8]);
    assert_eq!(
        read_frame(&mut pair).await,
        batch_frame(&[set_time(9)]).unwrap()
    );

    let events = tap.events();
    assert_eq!(events.len(), 2);
    for direction in [PacketDirection::Clientbound, PacketDirection::Serverbound] {
        assert_eq!(
            events.iter().filter(|event| matches!(event, TapEvent::Record(observed, _, _) if *observed == direction)).count(),
            1,
        );
    }
}
