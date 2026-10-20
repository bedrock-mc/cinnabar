//! A public RakNet endpoint with actual UDP and generated Settings/Login/pack packets.

use bedrock_net::{
    BatchCodec, BatchHeader, BatchLimits, BatchPhase, CompressionAlgorithm, CompressionSettings,
    DecodeOutcome,
};
use bytes::Bytes;
use protocol::{
    Packet,
    test_support::negotiation::{self, ClientPacket},
};
use raknet::prelude::{
    Established, RakClock, RakPriority, RakReliability, RakServer, RakServerConfig, RakServerInput,
    RakServerOutput, RakSession, RakSessionInput, RakSessionOutput, Sans,
};
use std::net::Ipv4Addr;
use tokio::net::UdpSocket;

mod ingress;

pub(super) struct Peer {
    pub(super) socket: UdpSocket,
    server: RakServer,
    session: Option<RakSession<Established>>,
}

enum Input {
    Message(Bytes),
    Closed,
}

impl Peer {
    /// Refuses the real Login before StartGame, then drives the original peer through retirement.
    pub(super) async fn refuse_login(mut self) {
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
        self.send(&codec, &[negotiation::disconnect()]).await;
        assert!(
            matches!(self.next().await, Input::Closed),
            "refused Login produced another application message"
        );
    }

    /// Creates only an actual loopback socket and the public unconnected endpoint.
    pub(super) async fn new() -> Self {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let now = std::time::Instant::now();
        let server = RakServer::new(
            RakServerConfig::default(),
            socket.local_addr().unwrap(),
            RakClock::new(now),
            now,
        )
        .unwrap();
        Self {
            socket,
            server,
            session: None,
        }
    }

    /// Drives the real handshake, offers no packs, and retains the session until actual retirement.
    pub(super) async fn serve(mut self) {
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
        self.send(&codec, &negotiation::empty_stack_and_start_game(42))
            .await;
        assert!(matches!(
            self.packet(&codec).await,
            ClientPacket::ResourcePackClientResponse
        ));
        // A bounded batch repeats valid text to exercise the real browser writer.
        let text =
            negotiation::raw_text("x".repeat(BatchLimits::default().max_decoded_bytes / 1024));
        let limits = BatchLimits::default();
        let encoded = negotiation::server_frame(&text);
        let count = (limits.max_decoded_bytes / 2 / encoded.len()).min(limits.max_packets);
        assert!(count > 1, "fixture must offer repeated legal text packets");
        let text = vec![text; count];
        for _ in 0..8 {
            self.send(&codec, &text).await;
        }
        match self.next().await {
            Input::Closed => {}
            Input::Message(_) => {
                panic!("empty-upload browser submitted an unexpected application message")
            }
        }
    }

    /// Decodes the original client batch using the same public codec and generated decoder.
    async fn packet<S: BatchPhase>(&mut self, codec: &BatchCodec<S>) -> ClientPacket {
        let Input::Message(message) = self.next().await else {
            panic!("peer retired before the native negotiation completed")
        };
        let DecodeOutcome::Packets { mut packets, .. } = codec.decode(message).unwrap() else {
            panic!("expected an actual client batch")
        };
        assert_eq!(packets.len(), 1);
        let packet = packets.next().unwrap();
        negotiation::client_packet(&packet)
    }

    /// Frames generated server packets and submits them through the actual established core.
    async fn send<S: BatchPhase>(&mut self, codec: &BatchCodec<S>, packets: &[Packet]) {
        let limits = BatchLimits::default();
        let entries =
            negotiation::server_entries(packets, limits.max_packets, limits.max_decoded_bytes);
        let messages = codec.encode(&entries).unwrap();
        assert_eq!(messages.len(), 1, "fixture input must fit one real batch");
        for message in messages {
            let session = self.session.as_mut().expect("actual established peer");
            assert!(message.len() <= session.max_message_len());
            session
                .send(
                    message.to_vec().into_boxed_slice(),
                    RakReliability::ReliableOrdered,
                    RakPriority::Normal,
                    std::time::Instant::now(),
                )
                .unwrap();
        }
        assert!(self.drain().await.is_none());
    }

    /// Routes datagrams to actual endpoint/session owners until an application event arrives.
    async fn next(&mut self) -> Input {
        loop {
            if let Some(input) = self.drain().await {
                return input;
            }
            let mut buffer = vec![0; usize::from(u16::MAX) + 1];
            let (length, from) = self.socket.recv_from(&mut buffer).await.unwrap();
            self.server
                .handle(RakServerInput::Datagram(
                    buffer[..length].into(),
                    from,
                    std::time::Instant::now(),
                ))
                .unwrap();
        }
    }

    /// Emits only original core datagrams and never fabricates an established owner.
    async fn drain(&mut self) -> Option<Input> {
        while let Some(output) = self.server.poll() {
            match output {
                RakServerOutput::SocketDatagram(bytes, to) => {
                    assert_eq!(self.socket.send_to(&bytes, to).await.unwrap(), bytes.len());
                }
                RakServerOutput::SessionConnected(session) => {
                    assert!(self.session.replace(session).is_none())
                }
                RakServerOutput::SessionDatagram(bytes, id) => {
                    let session = self
                        .session
                        .as_mut()
                        .expect("actual negotiated core session");
                    assert_eq!(session.id(), id);
                    session
                        .handle(RakSessionInput::Datagram(bytes, std::time::Instant::now()))
                        .unwrap();
                }
                RakServerOutput::SessionClosing(_) => {
                    panic!("peer admission expired before its genuine session")
                }
            }
        }
        if let Some(session) = &mut self.session {
            session.tick(std::time::Instant::now()).unwrap();
            while let Some(output) = session.poll() {
                match output {
                    RakSessionOutput::Datagram(bytes, to) => {
                        assert_eq!(self.socket.send_to(&bytes, to).await.unwrap(), bytes.len())
                    }
                    RakSessionOutput::Packet(bytes) => {
                        return Some(Input::Message(Bytes::from(bytes)));
                    }
                    RakSessionOutput::Disconnected(..) => return Some(Input::Closed),
                    RakSessionOutput::Wait(_) => {}
                }
            }
        }
        None
    }
}
