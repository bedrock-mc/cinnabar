//! The public relay constructor admits only primary, known input before interpreting controls.

use super::*;
use protocol::{NativeRelayConnecting, RawClientData, session_wire::CoreMessage};
use std::{io, net::SocketAddr, time::Duration};

const TRANSFER_HOST: &str = "relay-fixture.example.test";
const TRANSFER_PORT: u16 = 19134;

#[derive(Clone, Copy)]
enum Case {
    UnknownStartup,
    SecondaryStartup,
    UnknownLive,
    SecondaryLive,
    Retired,
    Disconnect,
    Transfer,
    MalformedHeader,
    MalformedTerminal,
}

struct Observed {
    startup: Result<Vec<Bytes>, protocol::ProtocolError>,
    packets: Vec<Bytes>,
    terminal: Option<CoreMessage>,
    read_error: Option<io::Error>,
    after_terminal: Option<io::ErrorKind>,
    closed: io::Result<()>,
}

impl Case {
    /// Coalesces the real stack, ignored controls, primary StartGame and live prefix in one batch.
    fn entries(self) -> Vec<Bytes> {
        let [stack, start] = negotiation::empty_stack_and_start_game(42);
        let mut entries = vec![entry(&stack)];
        match self {
            Self::UnknownStartup => entries.push(negotiation::unknown_server_entry()),
            Self::SecondaryStartup => {
                for (runtime, sender, target) in [(7, 1, 0), (8, 0, 1)] {
                    let [_, secondary] = negotiation::empty_stack_and_start_game(runtime);
                    entries.push(negotiation::server_entry(&secondary, sender, target));
                }
            }
            Self::MalformedHeader => entries.push(Bytes::from_static(&[0x80])),
            _ => {}
        }
        entries.push(entry(&start));
        entries.push(entry(&negotiation::raw_text(
            "first original packet".into(),
        )));
        match self {
            Self::UnknownLive | Self::Retired => {
                entries.push(negotiation::unknown_server_entry());
            }
            Self::SecondaryLive => {
                entries.push(negotiation::server_entry(&negotiation::disconnect(), 1, 0));
                entries.push(negotiation::server_entry(
                    &negotiation::transfer(TRANSFER_HOST, TRANSFER_PORT),
                    0,
                    1,
                ));
            }
            Self::MalformedTerminal => entries.push(negotiation::header_only_server_entry(
                &negotiation::disconnect(),
            )),
            _ => {}
        }
        entries.push(entry(&negotiation::raw_text(
            "second original packet".into(),
        )));
        let terminal = if matches!(self, Self::Transfer) {
            negotiation::transfer(TRANSFER_HOST, TRANSFER_PORT)
        } else {
            negotiation::disconnect()
        };
        entries.push(entry(&terminal));
        entries.push(entry(&negotiation::raw_text(
            "after original terminal".into(),
        )));
        entries
    }
}

/// Encodes a primary packet using the same generated encoder as the original peer fixture.
fn entry(packet: &Packet) -> Bytes {
    negotiation::server_entry(packet, 0, 0)
}

impl Peer {
    /// Uses the unchanged actual handshake and client acknowledgments, then waits for physical close.
    async fn ingress(mut self, case: Case) {
        let codec = BatchCodec::new(BatchHeader::RakNet, BatchLimits::default());
        assert!(matches!(
            self.packet(&codec).await,
            ClientPacket::RequestNetworkSettings
        ));
        self.send(&codec, &[negotiation::network_settings(u16::MAX)])
            .await;
        let codec = codec.negotiate(CompressionSettings {
            algorithm: CompressionAlgorithm::Deflate,
            threshold: u16::MAX,
        });
        assert!(matches!(self.packet(&codec).await, ClientPacket::Login));
        self.send(&codec, &negotiation::login_success_and_pack_info())
            .await;
        assert!(matches!(
            self.packet(&codec).await,
            ClientPacket::ClientCacheStatus
        ));
        assert!(matches!(
            self.packet(&codec).await,
            ClientPacket::ResourcePackClientResponse
        ));
        let messages = codec.encode(&case.entries()).unwrap();
        assert_eq!(
            messages.len(),
            1,
            "all input shares the actual received batch cursor"
        );
        for message in messages {
            self.session
                .as_mut()
                .expect("genuinely negotiated peer")
                .send(
                    message.to_vec().into_boxed_slice(),
                    RakReliability::ReliableOrdered,
                    RakPriority::Normal,
                    std::time::Instant::now(),
                )
                .unwrap();
        }
        assert!(self.drain().await.is_none());
        assert!(matches!(
            self.packet(&codec).await,
            ClientPacket::ResourcePackClientResponse
        ));
        assert!(matches!(self.next().await, Input::Closed));
    }
}

/// Uses the original pump while retaining its one driver beside every borrowed receive wait.
async fn pumped(connection: protocol::NativeRelayConnection, startup: Vec<Bytes>) -> Observed {
    let (sender, mut incoming, mut driver) = connection.into_pump();
    let mut packets = Vec::new();
    let mut terminal = None;
    let mut read_error = None;
    let mut after_terminal = None;
    {
        let reading = async {
            // The script is finite; stop at its first real terminal/error, never a replacement cause.
            for _ in 0..Case::SecondaryLive.entries().len() {
                match incoming.recv().await {
                    Ok(CoreMessage::Batch(batch)) => packets.extend(batch),
                    Ok(message @ (CoreMessage::Disconnect(_) | CoreMessage::Transfer(_))) => {
                        terminal = Some(message);
                        after_terminal = incoming.recv().await.err().map(|error| error.kind());
                        break;
                    }
                    Ok(other) => panic!("unexpected established relay message: {other:?}"),
                    Err(error) => {
                        read_error = Some(error);
                        break;
                    }
                }
            }
        };
        tokio::pin!(reading);
        tokio::select! {
            () = &mut reading => {}
            _ = driver.drive() => reading.await,
        }
    }
    let mut closing = driver.begin_close();
    let closed = closing.finish_close().await;
    drop((closing, incoming, sender));
    Observed {
        startup: Ok(startup),
        packets,
        terminal,
        read_error,
        after_terminal,
        closed,
    }
}

/// Drains only the already accepted same-batch cursor after consuming application Send.
async fn retired(connection: protocol::NativeRelayConnection, startup: Vec<Bytes>) -> Observed {
    let mut connection = connection.begin_close();
    let closed = connection.finish_close().await;
    let mut packets = Vec::new();
    let mut terminal = None;
    let mut read_error = None;
    let mut after_terminal = None;
    for _ in 0..Case::Retired.entries().len() {
        match connection.recv().await {
            Ok(CoreMessage::Batch(batch)) => packets.extend(batch),
            Ok(message @ (CoreMessage::Disconnect(_) | CoreMessage::Transfer(_))) => {
                terminal = Some(message);
                after_terminal = connection.recv().await.err().map(|error| error.kind());
                break;
            }
            Ok(other) => panic!("unexpected retired relay message: {other:?}"),
            Err(error) => {
                read_error = Some(error);
                break;
            }
        }
    }
    drop(connection);
    Observed {
        startup: Ok(startup),
        packets,
        terminal,
        read_error,
        after_terminal,
        closed,
    }
}

/// Returns only after the actual public connection, original peer and both UDP tuples are released.
async fn exercise(case: Case) -> Observed {
    tokio::time::timeout(Duration::from_secs(20), async {
        let peer = Peer::new().await;
        let address = peer.socket.local_addr().unwrap();
        let mut client_address: Option<SocketAddr> = None;
        let client = async {
            let actual = bedrock_net::raknet::RakNetConnection::connect(
                address,
                bedrock_net::raknet::RakNetOptions::default(),
            )
            .await
            .unwrap();
            client_address = Some(actual.local_addr());
            let mut joining = NativeRelayConnecting::from_raknet(actual, address.to_string());
            let claims = protocol::login_client_data(
                "Fixture",
                protocol::ClientRandomId::new(1).unwrap(),
                None,
                &protocol::LoginSettings::default(),
            );
            let claims = serde_json::value::to_raw_value(&claims).unwrap();
            if let Err(error) = joining
                .negotiate(RawClientData::from_raw(&claims).unwrap(), false, None)
                .await
            {
                let mut retiring = joining.begin_close();
                let closed = retiring.finish_close().await;
                drop(retiring);
                return Observed {
                    startup: Err(error),
                    packets: Vec::new(),
                    terminal: None,
                    read_error: None,
                    after_terminal: None,
                    closed,
                };
            }
            let (connected, handoff) = joining
                .try_finish()
                .unwrap_or_else(|_| panic!("the actual StartGame did not provide its handoff"));
            assert!(handoff.archives.is_empty());
            let startup = handoff.handoff.startup;
            if matches!(case, Case::Retired) {
                retired(connected, startup).await
            } else {
                pumped(connected, startup).await
            }
        };
        let ((), observed) = tokio::join!(peer.ingress(case), client);
        assert!(
            UdpSocket::bind(address).await.is_ok(),
            "original peer socket stayed owned"
        );
        assert!(
            UdpSocket::bind(client_address.unwrap()).await.is_ok(),
            "original client socket stayed owned"
        );
        observed
    })
    .await
    .expect("finite real relay ingress fixture stalled")
}

/// Asserts original handoff, ordered primary packets and exactly the first actual terminal.
fn original_prefix(observed: Observed, transfer: bool) {
    observed.closed.unwrap();
    let [_, start] = negotiation::empty_stack_and_start_game(42);
    assert_eq!(
        observed
            .startup
            .expect("unused input ended relay negotiation"),
        [entry(&start)]
    );
    assert_eq!(
        observed.packets,
        [
            entry(&negotiation::raw_text("first original packet".into())),
            entry(&negotiation::raw_text("second original packet".into())),
        ],
        "unused input changed the primary accepted FIFO"
    );
    assert!(
        observed.read_error.is_none(),
        "unused input ended live relay input: {:?}",
        observed.read_error
    );
    match observed.terminal {
        Some(CoreMessage::Transfer(target)) if transfer => {
            assert_eq!(target.address, TRANSFER_HOST);
            assert_eq!(target.port, TRANSFER_PORT);
            assert!(target.reload_world);
        }
        Some(CoreMessage::Disconnect(_)) if !transfer => {}
        other => panic!("wrong first primary terminal: {other:?}"),
    }
    assert_eq!(observed.after_terminal, Some(io::ErrorKind::UnexpectedEof));
}

#[tokio::test]
async fn native_relay_ingress_skips_unknown_between_stack_and_start_game() {
    original_prefix(exercise(Case::UnknownStartup).await, false);
}

#[tokio::test]
async fn native_relay_ingress_skips_both_secondary_start_games_before_handoff() {
    original_prefix(exercise(Case::SecondaryStartup).await, false);
}

#[tokio::test]
async fn native_relay_ingress_skips_unknown_during_actual_live_pump() {
    original_prefix(exercise(Case::UnknownLive).await, false);
}

#[tokio::test]
async fn native_relay_ingress_skips_secondary_disconnect_and_transfer_during_play() {
    original_prefix(exercise(Case::SecondaryLive).await, false);
}

#[tokio::test]
async fn native_relay_ingress_retired_cursor_keeps_the_same_admitted_prefix() {
    original_prefix(exercise(Case::Retired).await, false);
}

#[tokio::test]
async fn native_relay_ingress_primary_terminals_keep_fifo_and_end_exactly_once() {
    original_prefix(exercise(Case::Disconnect).await, false);
    original_prefix(exercise(Case::Transfer).await, true);
}

#[tokio::test]
async fn native_relay_ingress_malformed_primary_header_is_rejected() {
    let observed = exercise(Case::MalformedHeader).await;
    observed.closed.unwrap();
    assert!(
        observed.startup.is_err(),
        "malformed primary header was skipped"
    );
}

#[tokio::test]
async fn native_relay_ingress_malformed_primary_terminal_is_rejected_after_prefix() {
    let observed = exercise(Case::MalformedTerminal).await;
    observed.closed.unwrap();
    assert!(observed.startup.is_ok());
    assert_eq!(
        observed.packets,
        [entry(&negotiation::raw_text(
            "first original packet".into()
        ))]
    );
    assert!(observed.terminal.is_none());
    assert!(
        observed.read_error.is_some(),
        "malformed primary terminal was skipped"
    );
}
