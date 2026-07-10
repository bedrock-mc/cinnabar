use std::path::Path;

use jolyne::error::JolyneError;
use jolyne::stream::client::ClientHandshakeConfig;
use jolyne::stream::transport::{BedrockTransport, Transport};
use jolyne::stream::{BedrockStream, Client, Handshake, Play};

use crate::socket_transport::SocketTransport;
use crate::{GameData, Packet, ProtocolError};

const MAX_DECOMPRESSED_BATCH_SIZE: usize = 16 * 1024 * 1024;

/// Entry point for the offline local-core login sequence.
pub struct LoginSequence;

impl LoginSequence {
    /// Connects to the Go core and completes the encrypted Bedrock spawn sequence.
    pub async fn connect(
        socket_dir: &Path,
        display_name: &str,
    ) -> Result<(PlaySession, GameData), ProtocolError> {
        let transport = SocketTransport::connect(socket_dir)
            .await
            .map_err(ProtocolError::Bridge)?;
        Self::connect_transport(transport, display_name).await
    }

    /// Generic transport seam used by deterministic protocol state tests.
    #[doc(hidden)]
    pub async fn connect_transport<T: Transport>(
        transport: T,
        display_name: &str,
    ) -> Result<(PlaySession<T>, GameData), ProtocolError> {
        let peer_addr = transport.peer_addr();
        let mut transport = BedrockTransport::new(transport);
        transport.set_max_decompressed_batch_size(Some(MAX_DECOMPRESSED_BATCH_SIZE));
        let stream: BedrockStream<Handshake, Client, T> = BedrockStream::from_transport(transport);
        let config = ClientHandshakeConfig::random(peer_addr, display_name);
        let (stream, game_data) = stream.join(config).await?;
        Ok((PlaySession::new(stream), game_data))
    }
}

/// An authenticated, spawned Bedrock session.
pub struct PlaySession<T: Transport = SocketTransport> {
    stream: BedrockStream<Play, Client, T>,
    decode_errors: u64,
}

impl<T: Transport> PlaySession<T> {
    fn new(stream: BedrockStream<Play, Client, T>) -> Self {
        Self {
            stream,
            decode_errors: 0,
        }
    }

    /// Receives one packet, counting malformed/decompression failures.
    pub async fn recv(&mut self) -> Result<Packet, ProtocolError> {
        match self.stream.recv_packet().await {
            Ok(packet) => Ok(packet),
            Err(error) => {
                if is_decode_error(&error) {
                    self.decode_errors = self.decode_errors.saturating_add(1);
                }
                Err(error.into())
            }
        }
    }

    /// Sends one packet through the encrypted play session.
    pub async fn send(&mut self, packet: Packet) -> Result<(), ProtocolError> {
        crate::codec::validate_packet(&packet)?;
        self.stream.send_packet(packet).await?;
        Ok(())
    }

    /// Number of receive-side decode/decompression failures observed in play.
    pub fn decode_error_count(&self) -> u64 {
        self.decode_errors
    }
}

fn is_decode_error(error: &JolyneError) -> bool {
    matches!(
        error,
        JolyneError::Decode(_)
            | JolyneError::PacketDecode { .. }
            | JolyneError::PacketTrailingBytes { .. }
            | JolyneError::Io(_)
            | JolyneError::Protocol(_)
    )
}
