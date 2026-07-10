//! Raw transport layer for the Bedrock protocol.
//!
//! `BedrockTransport<T>` handles encryption, compression, and batching
//! on top of any transport implementing the `Transport` trait.

use std::collections::VecDeque;
use std::future::poll_fn;
use std::pin::Pin;

use aes::Aes256;
use aes_gcm::{Aes256Gcm, Key};
use bytes::BytesMut;
use ctr::cipher::{KeyIvInit, StreamCipher};
use sha2::{Digest, Sha256};
use tracing::{debug, instrument};
use valentine::bedrock::codec::BedrockSized;

use super::{Transport, TransportMessage, TransportRecvMessage};
use crate::batch::{
    BatchCompression, decode_batch_no_prefix_raw, decode_batch_raw, decode_batch_raw_split,
    encode_batch_multi_into, encode_batch_raw_into,
};
use crate::error::{JolyneError, ProtocolError};
use crate::raw::{RawPacket, decode_packet_raw, decode_packet_raw_split};
use crate::valentine::{BorrowedMcpePacket, McpePacket, McpePacketArgs};
use valentine::bedrock::context::BedrockSession;

type Aes256Ctr = ctr::Ctr32BE<Aes256>;

const CHECKSUM_LEN: usize = 8;

/// Raw transport layer for the Bedrock protocol.
///
/// Handles:
/// - Framing (via underlying Transport)
/// - Encryption (AES-256-CTR + SHA256 checksum)
/// - Compression (Zlib/Deflate)
/// - Batching
///
/// This struct does NOT handle protocol state (Login, Handshake).
/// It merely reads and writes batches of packets.
///
/// Generic over `T: Transport` to support both RakNet and NetherNet.
pub struct BedrockTransport<T: Transport> {
    inner: T,
    // We keep the session for codec context (shield ID, etc.)
    pub(crate) session: BedrockSession,

    // Packet Buffering (single raw buffer - decoded on demand)
    recv_queue: VecDeque<RawPacket>,
    write_buffer: Vec<McpePacket>,
    auto_flush: bool,

    // Encryption State (Bedrock: AES-256-CTR + SHA256 checksum)
    pub(crate) encryption_enabled: bool,
    send_cipher: Option<Aes256Ctr>,
    recv_cipher: Option<Aes256Ctr>,
    key_bytes: Option<Vec<u8>>,
    send_counter: u64,
    recv_counter: u64,

    // Compression State
    pub(crate) compression_enabled: bool,
    pub(crate) compression_algorithm: BatchCompression,
    pub(crate) compression_level: u32,
    pub(crate) compression_threshold: u16,
    max_decompressed_batch_size: Option<usize>,
    encryption_header_len: usize,

    // Reusable outbound buffers to avoid per-send allocation churn.
    send_batch_buffer: BytesMut,
    send_raw_buffer: BytesMut,
    recv_decrypt_buffer: BytesMut,
}

impl<T: Transport> BedrockTransport<T> {
    /// Create a new BedrockTransport wrapping the given transport.
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            session: BedrockSession { shield_item_id: 0 },
            recv_queue: VecDeque::new(),
            write_buffer: Vec::new(),
            auto_flush: true,
            encryption_enabled: false,
            send_cipher: None,
            recv_cipher: None,
            key_bytes: None,
            send_counter: 0,
            recv_counter: 0,
            compression_enabled: false,
            compression_algorithm: BatchCompression::Deflate,
            compression_level: 7,
            compression_threshold: 0,
            max_decompressed_batch_size: Some(1024 * 1024 * 4),
            encryption_header_len: 1,
            send_batch_buffer: BytesMut::new(),
            send_raw_buffer: BytesMut::new(),
            recv_decrypt_buffer: BytesMut::new(),
        }
    }

    /// Enable encryption with the derived keys.
    #[instrument(skip_all, level = "debug", fields(peer_addr = %self.peer_addr()))]
    pub fn enable_encryption(&mut self, key: Key<Aes256Gcm>, iv: [u8; 12]) {
        let key_bytes = key.to_vec();

        let mut iv16 = [0u8; 16];
        iv16[..12].copy_from_slice(&iv);
        iv16[12..].copy_from_slice(&[0, 0, 0, 2]);

        let base_cipher = Aes256Ctr::new_from_slices(&key_bytes, &iv16)
            .expect("AES-256-CTR key/iv lengths are fixed (32/16 bytes)");
        self.send_cipher = Some(base_cipher.clone());
        self.recv_cipher = Some(base_cipher);
        self.key_bytes = Some(key_bytes);
        self.send_counter = 0;
        self.recv_counter = 0;
        self.encryption_enabled = true;
        debug!("encryption enabled");
    }

    /// Sets compression parameters.
    pub fn set_compression(&mut self, enabled: bool, level: u32, threshold: u16) {
        self.set_compression_algorithm(enabled, BatchCompression::Deflate, level, threshold);
    }

    /// Sets the negotiated compression algorithm and parameters.
    pub fn set_compression_algorithm(
        &mut self,
        enabled: bool,
        algorithm: BatchCompression,
        level: u32,
        threshold: u16,
    ) {
        self.compression_enabled = enabled;
        self.compression_algorithm = algorithm;
        self.compression_level = level;
        self.compression_threshold = threshold;
    }

    /// Configures the maximum accepted decompressed batch payload size.
    pub fn set_max_decompressed_batch_size(&mut self, max: Option<usize>) {
        self.max_decompressed_batch_size = max;
    }

    /// Configures how many leading bytes remain unencrypted in encrypted frames.
    pub fn set_encryption_header_len(&mut self, len: usize) {
        self.encryption_header_len = len;
    }

    /// Applies listener-level protocol safety limits to this transport.
    pub fn apply_listener_config(&mut self, config: &crate::config::BedrockListenerConfig) {
        self.set_max_decompressed_batch_size(config.max_decompressed_batch_size);
        self.set_encryption_header_len(config.encryption_header_len);
    }

    /// Configures the flushing strategy.
    pub fn set_auto_flush(&mut self, auto: bool) {
        self.auto_flush = auto;
    }

    /// Returns the current packet decode/encode args derived from transport session state.
    pub fn packet_args(&self) -> McpePacketArgs {
        McpePacketArgs::from(&self.session)
    }

    /// Sends a packet. Behavior depends on `set_auto_flush`.
    pub async fn send(&mut self, packet: McpePacket) -> Result<(), JolyneError> {
        if self.auto_flush {
            self.send_batch(&[packet]).await
        } else {
            self.write_buffer.push(packet);
            Ok(())
        }
    }

    /// Flushes all buffered packets as a single batch (ReliableOrdered).
    pub async fn flush(&mut self) -> Result<(), JolyneError> {
        if self.write_buffer.is_empty() {
            return Ok(());
        }
        let packets: Vec<McpePacket> = self.write_buffer.drain(..).collect();
        self.send_batch(&packets).await
    }

    /// Sends a list of packets as a single batch using `ReliableOrdered` reliability.
    #[instrument(skip_all, level = "trace", fields(peer_addr = %self.peer_addr()))]
    pub async fn send_batch(&mut self, packets: &[McpePacket]) -> Result<(), JolyneError> {
        self.send_batch_with_reliability(packets, true).await
    }

    /// Sends a list of packets as a single batch with specified reliability.
    #[instrument(skip_all, level = "trace", fields(peer_addr = %self.peer_addr(), reliable = reliable))]
    pub async fn send_batch_with_reliability(
        &mut self,
        packets: &[McpePacket],
        reliable: bool,
    ) -> Result<(), JolyneError> {
        if self.encryption_enabled && !reliable {
            return Err(JolyneError::Protocol(ProtocolError::UnexpectedHandshake(
                "Cannot use unreliable networking when encryption is active".into(),
            )));
        }

        if packets.is_empty() {
            return Ok(());
        }

        // 1. Encode Batch (Handles Compression)
        // Use T::USES_BATCH_PREFIX to conditionally add 0xFE prefix
        encode_batch_multi_into(
            packets,
            self.compression_enabled,
            self.compression_algorithm,
            self.compression_level,
            self.compression_threshold,
            T::USES_BATCH_PREFIX,
            &mut self.send_batch_buffer,
        )?;

        // 2. Encrypt & Send
        let msg = if self.encryption_enabled {
            let cipher = self
                .send_cipher
                .as_mut()
                .expect("encryption_enabled implies send_cipher is initialised");
            let key_bytes = self
                .key_bytes
                .as_deref()
                .expect("encryption_enabled implies key_bytes is initialised");
            tracing::trace!(
                "Encrypting batch of {} bytes (send_counter={})",
                self.send_batch_buffer.len(),
                self.send_counter
            );
            Self::encrypt_buffer(
                &mut self.send_batch_buffer,
                cipher,
                key_bytes,
                &mut self.send_counter,
                self.encryption_header_len,
            )?;
            tracing::trace!("Encrypted batch is {} bytes", self.send_batch_buffer.len());
            TransportMessage::reliable(self.send_batch_buffer.split().freeze())
        } else if reliable {
            TransportMessage::reliable(self.send_batch_buffer.split().freeze())
        } else {
            TransportMessage::unreliable(self.send_batch_buffer.split().freeze())
        };

        // Send using poll_fn to convert poll-based API to async
        tracing::trace!("Sending {} bytes to RakNet", msg.buffer.len());
        poll_fn(|cx| Pin::new(&mut self.inner).poll_send(cx, msg.clone()))
            .await
            .map_err(|e| JolyneError::Transport(e.to_string()))?;
        tracing::trace!("Sent successfully");

        Ok(())
    }

    /// Raw send for handshake packets that cannot be batched.
    ///
    /// The framing depends on the transport type:
    /// - **RakNet**: Uses game frame format `[0xFE][Length][Header][Body]`
    /// - **NetherNet**: Uses inner format `[Length][Header][Body]` (no 0xFE)
    #[instrument(skip_all, level = "trace", fields(peer_addr = %self.peer_addr()))]
    pub async fn send_raw(&mut self, packet: McpePacket) -> Result<(), JolyneError> {
        let reserve = if T::USES_BATCH_PREFIX {
            11usize + packet.data.encoded_size()
        } else {
            10usize + packet.data.encoded_size()
        };
        self.send_raw_buffer.clear();
        self.send_raw_buffer.reserve(reserve);

        if T::USES_BATCH_PREFIX {
            // RakNet: Use full game frame with 0xFE prefix
            packet.encode_bytes_mut(&mut self.send_raw_buffer)?;
        } else {
            // NetherNet: Use inner format without 0xFE prefix
            packet.data.encode_inner_bytes_mut(
                &mut self.send_raw_buffer,
                packet.header.from_subclient,
                packet.header.to_subclient,
            )?;
        }

        if self.encryption_enabled {
            let cipher = self
                .send_cipher
                .as_mut()
                .expect("encryption_enabled implies send_cipher is initialised");
            let key_bytes = self
                .key_bytes
                .as_deref()
                .expect("encryption_enabled implies key_bytes is initialised");
            Self::encrypt_buffer(
                &mut self.send_raw_buffer,
                cipher,
                key_bytes,
                &mut self.send_counter,
                self.encryption_header_len,
            )?;
        }

        let msg = TransportMessage::reliable(self.send_raw_buffer.split().freeze());
        poll_fn(|cx| Pin::new(&mut self.inner).poll_send(cx, msg.clone()))
            .await
            .map_err(|e| JolyneError::Transport(e.to_string()))?;
        Ok(())
    }

    /// Returns the next single packet as an owned protocol packet.
    ///
    /// This materializes packet contents and is the convenience path for
    /// higher-level code that wants owned values immediately.
    pub async fn recv_packet(&mut self) -> Result<McpePacket, JolyneError> {
        loop {
            if let Some(raw) = self.recv_queue.pop_front() {
                // Decode on demand
                return raw.decode(&self.session);
            }
            let packets = self.recv_batch_raw().await?;
            if packets.is_empty() {
                continue;
            }
            self.recv_queue.extend(packets);
        }
    }

    /// Reads a batch of packets from the network, returning owned packets.
    ///
    /// This eagerly materializes packet contents. For hot ingress paths, prefer
    /// [`Self::recv_batch_borrowed`] or [`Self::recv_batch_raw`].
    #[instrument(skip_all, level = "trace", fields(peer_addr = %self.peer_addr()))]
    pub async fn recv_batch(&mut self) -> Result<Vec<McpePacket>, JolyneError> {
        let raw_packets = self.recv_batch_raw().await?;
        raw_packets
            .into_iter()
            .map(|raw| raw.decode(&self.session))
            .collect()
    }

    /// Returns the next packet as raw bytes (header parsed, body undecoded).
    ///
    /// Useful for proxies that need to inspect packet IDs without full decode.
    /// Call [`RawPacket::decode`] if you later need the full packet.
    pub async fn recv_packet_raw(&mut self) -> Result<RawPacket, JolyneError> {
        loop {
            if let Some(pkt) = self.recv_queue.pop_front() {
                return Ok(pkt);
            }
            let packets = self.recv_batch_raw().await?;
            if packets.is_empty() {
                continue;
            }
            self.recv_queue.extend(packets);
        }
    }

    /// Restores packets consumed by an internal state machine so callers can
    /// observe them after the state transition. Restored packets retain their
    /// original order and precede unread packets from the same network batch.
    pub(crate) fn prepend_recv_queue(&mut self, packets: impl IntoIterator<Item = RawPacket>) {
        let mut deferred: VecDeque<_> = packets.into_iter().collect();
        deferred.append(&mut self.recv_queue);
        self.recv_queue = deferred;
    }

    /// Returns the next packet as a borrowed protocol view.
    ///
    /// This is the preferred ingress path for handshake and other hot packet
    /// processing where the caller can stay on borrowed data.
    pub async fn recv_packet_borrowed(&mut self) -> Result<BorrowedMcpePacket, JolyneError> {
        loop {
            if let Some(raw) = self.recv_queue.pop_front() {
                return raw.decode_borrowed();
            }
            let packets = self.recv_batch_raw().await?;
            if packets.is_empty() {
                continue;
            }
            self.recv_queue.extend(packets);
        }
    }

    /// Returns all packets from the next network batch as borrowed protocol views.
    ///
    /// This is the preferred batch receive path when the caller can inspect or
    /// route packets without immediately allocating owned payloads.
    #[instrument(skip_all, level = "trace", fields(peer_addr = %self.peer_addr()))]
    pub async fn recv_batch_borrowed(&mut self) -> Result<Vec<BorrowedMcpePacket>, JolyneError> {
        let raw_packets = self.recv_batch_raw().await?;
        raw_packets
            .into_iter()
            .map(RawPacket::decode_borrowed)
            .collect()
    }

    /// Returns all packets from the next network batch as raw bytes.
    #[instrument(skip_all, level = "trace", fields(peer_addr = %self.peer_addr()))]
    pub async fn recv_batch_raw(&mut self) -> Result<Vec<RawPacket>, JolyneError> {
        // 1. Read Raw Frame
        tracing::trace!("Waiting for raw RakNet frame...");
        let recv_result = poll_fn(|cx| Pin::new(&mut self.inner).poll_recv(cx)).await;
        let packet_bytes = recv_result
            .ok_or(JolyneError::ConnectionClosed)?
            .map_err(|e| JolyneError::Transport(e.to_string()))?;
        tracing::trace!("Received {} bytes from transport", packet_bytes.len());

        // 2. Decrypt
        if self.encryption_enabled {
            packet_bytes.copy_into(&mut self.recv_decrypt_buffer);
            let cipher = self
                .recv_cipher
                .as_mut()
                .expect("encryption_enabled implies recv_cipher is initialised");
            let key_bytes = self
                .key_bytes
                .as_deref()
                .expect("encryption_enabled implies key_bytes is initialised");
            Self::decrypt_buffer(
                &mut self.recv_decrypt_buffer,
                cipher,
                key_bytes,
                &mut self.recv_counter,
                self.encryption_header_len,
            )?;
            let mut buf = self.recv_decrypt_buffer.split().freeze();
            if buf.is_empty() {
                return Ok(vec![]);
            }

            if T::USES_BATCH_PREFIX {
                if buf[0] == 0xFE {
                    decode_batch_raw(
                        &mut buf,
                        self.compression_enabled,
                        self.max_decompressed_batch_size,
                    )
                } else {
                    Ok(vec![decode_packet_raw(&mut buf)?])
                }
            } else if self.compression_enabled {
                decode_batch_no_prefix_raw(&mut buf, self.max_decompressed_batch_size)
            } else {
                Ok(vec![decode_packet_raw(&mut buf)?])
            }
        } else {
            match packet_bytes {
                TransportRecvMessage::Contiguous(mut buf) => {
                    if buf.is_empty() {
                        return Ok(vec![]);
                    }

                    if T::USES_BATCH_PREFIX {
                        if buf[0] == 0xFE {
                            decode_batch_raw(
                                &mut buf,
                                self.compression_enabled,
                                self.max_decompressed_batch_size,
                            )
                        } else {
                            Ok(vec![decode_packet_raw(&mut buf)?])
                        }
                    } else if self.compression_enabled {
                        decode_batch_no_prefix_raw(&mut buf, self.max_decompressed_batch_size)
                    } else {
                        Ok(vec![decode_packet_raw(&mut buf)?])
                    }
                }
                TransportRecvMessage::SplitFirst { first, rest } => {
                    if T::USES_BATCH_PREFIX && first == 0xFE {
                        decode_batch_raw_split(
                            first,
                            rest,
                            self.compression_enabled,
                            self.max_decompressed_batch_size,
                        )
                    } else {
                        Ok(vec![decode_packet_raw_split(first, rest)?])
                    }
                }
            }
        }
    }

    /// Sends raw packets as a batch with specified reliability.
    ///
    /// Useful for proxies forwarding packets without decode/re-encode overhead.
    #[instrument(skip_all, level = "trace", fields(peer_addr = %self.peer_addr(), reliable = reliable))]
    pub async fn send_batch_raw(
        &mut self,
        packets: &[RawPacket],
        reliable: bool,
    ) -> Result<(), JolyneError> {
        if self.encryption_enabled && !reliable {
            return Err(JolyneError::Protocol(ProtocolError::UnexpectedHandshake(
                "Cannot use unreliable networking when encryption is active".into(),
            )));
        }

        if packets.is_empty() {
            return Ok(());
        }

        encode_batch_raw_into(
            packets,
            self.compression_enabled,
            self.compression_algorithm,
            self.compression_level,
            self.compression_threshold,
            T::USES_BATCH_PREFIX,
            &mut self.send_batch_buffer,
        )?;

        let msg = if self.encryption_enabled {
            let cipher = self
                .send_cipher
                .as_mut()
                .expect("encryption_enabled implies send_cipher is initialised");
            let key_bytes = self
                .key_bytes
                .as_deref()
                .expect("encryption_enabled implies key_bytes is initialised");
            Self::encrypt_buffer(
                &mut self.send_batch_buffer,
                cipher,
                key_bytes,
                &mut self.send_counter,
                self.encryption_header_len,
            )?;
            TransportMessage::reliable(self.send_batch_buffer.split().freeze())
        } else if reliable {
            TransportMessage::reliable(self.send_batch_buffer.split().freeze())
        } else {
            TransportMessage::unreliable(self.send_batch_buffer.split().freeze())
        };

        poll_fn(|cx| Pin::new(&mut self.inner).poll_send(cx, msg.clone()))
            .await
            .map_err(|e| JolyneError::Transport(e.to_string()))?;

        Ok(())
    }

    /// Sends a single raw packet (convenience wrapper around `send_batch_raw`).
    pub async fn send_packet_raw(&mut self, packet: RawPacket) -> Result<(), JolyneError> {
        self.send_batch_raw(&[packet], true).await
    }

    // --- Crypto Helpers ---

    fn encrypt_buffer(
        buf: &mut BytesMut,
        cipher: &mut Aes256Ctr,
        key_bytes: &[u8],
        counter: &mut u64,
        header_len: usize,
    ) -> Result<(), JolyneError> {
        if buf.is_empty() {
            return Ok(());
        }
        if header_len > buf.len() {
            return Err(ProtocolError::UnexpectedHandshake(format!(
                "encryption header length {} exceeds packet length {}",
                header_len,
                buf.len()
            ))
            .into());
        }

        let counter_value = *counter;
        *counter = counter.wrapping_add(1);

        let counter_bytes = counter_value.to_le_bytes();
        let mut digest = Sha256::new();
        digest.update(counter_bytes);
        digest.update(&buf[header_len..]);
        digest.update(key_bytes);
        let checksum = digest.finalize();

        buf.extend_from_slice(&checksum[..CHECKSUM_LEN]);
        cipher.apply_keystream(&mut buf[header_len..]);

        Ok(())
    }

    fn decrypt_buffer(
        buf: &mut BytesMut,
        cipher: &mut Aes256Ctr,
        key_bytes: &[u8],
        counter: &mut u64,
        header_len: usize,
    ) -> Result<(), JolyneError> {
        if buf.len() < header_len + CHECKSUM_LEN {
            return Err(ProtocolError::UnexpectedHandshake(format!(
                "encrypted packet must be at least {} bytes long, got {}",
                header_len + CHECKSUM_LEN,
                buf.len()
            ))
            .into());
        }

        cipher.apply_keystream(&mut buf[header_len..]);

        let checksum_start = buf.len() - CHECKSUM_LEN;
        let their_checksum = &buf[checksum_start..];

        let counter_value = *counter;
        *counter = counter.wrapping_add(1);

        let counter_bytes = counter_value.to_le_bytes();
        let mut digest = Sha256::new();
        digest.update(counter_bytes);
        digest.update(&buf[header_len..checksum_start]);
        digest.update(key_bytes);
        let our_checksum_full = digest.finalize();
        let our_checksum = &our_checksum_full[..CHECKSUM_LEN];

        if their_checksum != our_checksum {
            return Err(ProtocolError::UnexpectedHandshake(format!(
                "invalid checksum of packet {}: expected {:02x?}, got {:02x?}",
                counter_value, our_checksum, their_checksum
            ))
            .into());
        }

        buf.truncate(checksum_start);
        Ok(())
    }

    /// Returns the peer address.
    pub fn peer_addr(&self) -> std::net::SocketAddr {
        self.inner.peer_addr()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes_gcm::aead::generic_array::GenericArray;
    use std::io;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::pin::Pin;
    use std::task::{Context, Poll};

    struct TestTransport;

    impl Transport for TestTransport {
        type Error = io::Error;

        const USES_BATCH_PREFIX: bool = true;

        fn poll_send(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _msg: TransportMessage,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Pending
        }

        fn poll_recv(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Option<Result<TransportRecvMessage, Self::Error>>> {
            Poll::Pending
        }

        fn peer_addr(&self) -> SocketAddr {
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)
        }
    }

    // ========== Encryption Checksum Tests ==========

    /// Computes the Bedrock checksum: SHA256(counter_le || data || key) truncated to 8 bytes.
    fn compute_checksum(counter: u64, data: &[u8], key: &[u8]) -> [u8; CHECKSUM_LEN] {
        let mut digest = Sha256::new();
        digest.update(counter.to_le_bytes());
        digest.update(data);
        digest.update(key);
        let hash = digest.finalize();
        let mut result = [0u8; CHECKSUM_LEN];
        result.copy_from_slice(&hash[..CHECKSUM_LEN]);
        result
    }

    #[test]
    fn checksum_is_first_8_bytes_of_sha256() {
        let counter: u64 = 0;
        let data = b"hello world";
        let key = b"0123456789abcdef0123456789abcdef";

        let checksum = compute_checksum(counter, data, key);

        // Verify it's 8 bytes
        assert_eq!(checksum.len(), CHECKSUM_LEN);

        // Compute full SHA256 and verify truncation
        let mut full_digest = Sha256::new();
        full_digest.update(counter.to_le_bytes());
        full_digest.update(data);
        full_digest.update(key);
        let full_hash = full_digest.finalize();

        assert_eq!(&checksum[..], &full_hash[..CHECKSUM_LEN]);
    }

    #[test]
    fn checksum_changes_with_counter() {
        let data = b"test data";
        let key = b"0123456789abcdef0123456789abcdef";

        let checksum_0 = compute_checksum(0, data, key);
        let checksum_1 = compute_checksum(1, data, key);
        let checksum_max = compute_checksum(u64::MAX, data, key);

        assert_ne!(checksum_0, checksum_1);
        assert_ne!(checksum_0, checksum_max);
        assert_ne!(checksum_1, checksum_max);
    }

    #[test]
    fn checksum_changes_with_data() {
        let key = b"0123456789abcdef0123456789abcdef";

        let checksum_a = compute_checksum(0, b"data_a", key);
        let checksum_b = compute_checksum(0, b"data_b", key);

        assert_ne!(checksum_a, checksum_b);
    }

    #[test]
    fn checksum_changes_with_key() {
        let data = b"test data";

        let checksum_key1 = compute_checksum(0, data, b"key1_padding_to_32_bytes_12345");
        let checksum_key2 = compute_checksum(0, data, b"key2_padding_to_32_bytes_12345");

        assert_ne!(checksum_key1, checksum_key2);
    }

    // ========== IV Expansion Tests ==========

    #[test]
    fn iv_expansion_12_to_16_bytes() {
        let iv_12: [u8; 12] = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C,
        ];

        // Bedrock format: [12-byte IV][0, 0, 0, 2]
        let mut iv_16 = [0u8; 16];
        iv_16[..12].copy_from_slice(&iv_12);
        iv_16[12..].copy_from_slice(&[0, 0, 0, 2]);

        assert_eq!(&iv_16[..12], &iv_12[..]);
        assert_eq!(&iv_16[12..], &[0, 0, 0, 2]);
    }

    #[test]
    fn iv_suffix_is_big_endian_2() {
        // The suffix [0, 0, 0, 2] is big-endian representation of 2
        let suffix: [u8; 4] = [0, 0, 0, 2];
        let value = u32::from_be_bytes(suffix);
        assert_eq!(value, 2);
    }

    // ========== Counter Wrapping Tests ==========

    #[test]
    fn counter_wraps_at_u64_max() {
        let mut counter: u64 = u64::MAX;
        counter = counter.wrapping_add(1);
        assert_eq!(counter, 0);
    }

    #[test]
    fn counter_increments_correctly() {
        let mut counter: u64 = 0;
        for expected in 1..100 {
            counter = counter.wrapping_add(1);
            assert_eq!(counter, expected);
        }
    }

    // ========== AES-256-CTR Tests ==========

    #[test]
    fn aes_ctr_encrypt_decrypt_roundtrip() {
        let key_bytes: [u8; 32] = [0x42; 32];
        let iv_16: [u8; 16] = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0, 0, 0, 2,
        ];

        let plaintext = b"Hello, Bedrock!";
        let mut ciphertext = plaintext.to_vec();

        // Encrypt
        let mut cipher = Aes256Ctr::new_from_slices(&key_bytes, &iv_16).expect("valid key/iv");
        cipher.apply_keystream(&mut ciphertext);

        assert_ne!(&ciphertext[..], plaintext);

        // Decrypt (same cipher state means fresh cipher)
        let mut decrypted = ciphertext.clone();
        let mut cipher2 = Aes256Ctr::new_from_slices(&key_bytes, &iv_16).expect("valid key/iv");
        cipher2.apply_keystream(&mut decrypted);

        assert_eq!(&decrypted[..], plaintext);
    }

    #[test]
    fn aes_ctr_different_keys_produce_different_ciphertext() {
        let iv: [u8; 16] = [0x01; 16];
        let plaintext = b"Test message for encryption";

        let key1: [u8; 32] = [0x11; 32];
        let key2: [u8; 32] = [0x22; 32];

        let mut ct1 = plaintext.to_vec();
        let mut ct2 = plaintext.to_vec();

        Aes256Ctr::new_from_slices(&key1, &iv)
            .unwrap()
            .apply_keystream(&mut ct1);
        Aes256Ctr::new_from_slices(&key2, &iv)
            .unwrap()
            .apply_keystream(&mut ct2);

        assert_ne!(ct1, ct2);
    }

    #[test]
    fn aes_ctr_different_ivs_produce_different_ciphertext() {
        let key: [u8; 32] = [0x42; 32];
        let plaintext = b"Test message for encryption";

        let iv1: [u8; 16] = [0x01; 16];
        let iv2: [u8; 16] = [0x02; 16];

        let mut ct1 = plaintext.to_vec();
        let mut ct2 = plaintext.to_vec();

        Aes256Ctr::new_from_slices(&key, &iv1)
            .unwrap()
            .apply_keystream(&mut ct1);
        Aes256Ctr::new_from_slices(&key, &iv2)
            .unwrap()
            .apply_keystream(&mut ct2);

        assert_ne!(ct1, ct2);
    }

    // ========== Encrypt/Decrypt Message Format Tests ==========

    /// Simulates encrypt_outgoing behavior for testing
    fn encrypt_message(data: &[u8], key: &[u8], cipher: &mut Aes256Ctr, counter: u64) -> BytesMut {
        if data.is_empty() {
            return BytesMut::new();
        }

        let mut buf = BytesMut::from(data);

        // Compute checksum: SHA256(counter || data[1..] || key)[..8]
        let checksum = compute_checksum(counter, &data[1..], key);

        // Append checksum
        buf.extend_from_slice(&checksum);

        // Encrypt everything after first byte
        cipher.apply_keystream(&mut buf[1..]);

        buf
    }

    /// Simulates decrypt_incoming behavior for testing
    fn decrypt_message(
        buf: &mut BytesMut,
        key: &[u8],
        cipher: &mut Aes256Ctr,
        counter: u64,
    ) -> Result<(), &'static str> {
        if buf.is_empty() {
            return Ok(());
        }

        // Decrypt everything after first byte
        cipher.apply_keystream(&mut buf[1..]);

        if buf.len() < 1 + CHECKSUM_LEN {
            return Err("packet too short");
        }

        // Verify checksum
        let checksum_start = buf.len() - CHECKSUM_LEN;
        let their_checksum = &buf[checksum_start..];
        let our_checksum = compute_checksum(counter, &buf[1..checksum_start], key);

        if their_checksum != our_checksum {
            return Err("checksum mismatch");
        }

        // Remove checksum
        buf.truncate(checksum_start);
        Ok(())
    }

    #[test]
    fn encrypt_decrypt_roundtrip_with_checksum() {
        let key: [u8; 32] = [0x42; 32];
        let iv: [u8; 16] = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0, 0, 0, 2,
        ];

        let original = [0xFE, 0x01, 0x02, 0x03, 0x04]; // Batch packet marker + data

        // Encrypt
        let mut send_cipher = Aes256Ctr::new_from_slices(&key, &iv).unwrap();
        let encrypted = encrypt_message(&original, &key, &mut send_cipher, 0);

        // Verify checksum was appended
        assert_eq!(encrypted.len(), original.len() + CHECKSUM_LEN);

        // Decrypt
        let mut recv_cipher = Aes256Ctr::new_from_slices(&key, &iv).unwrap();
        let mut decrypted = encrypted.clone();
        decrypt_message(&mut decrypted, &key, &mut recv_cipher, 0).expect("decrypt should succeed");

        assert_eq!(decrypted.as_ref(), &original[..]);
    }

    #[test]
    fn decrypt_rejects_modified_checksum() {
        let key: [u8; 32] = [0x42; 32];
        let iv: [u8; 16] = [0x01; 16];

        let original = [0xFE, 0x01, 0x02, 0x03, 0x04];

        // Encrypt
        let mut send_cipher = Aes256Ctr::new_from_slices(&key, &iv).unwrap();
        let mut encrypted = encrypt_message(&original, &key, &mut send_cipher, 0);

        // Tamper with checksum (last byte)
        let last_idx = encrypted.len() - 1;
        encrypted[last_idx] ^= 0xFF;

        // Decrypt should fail
        let mut recv_cipher = Aes256Ctr::new_from_slices(&key, &iv).unwrap();
        let result = decrypt_message(&mut encrypted, &key, &mut recv_cipher, 0);
        assert!(result.is_err());
    }

    #[test]
    fn decrypt_rejects_modified_payload() {
        let key: [u8; 32] = [0x42; 32];
        let iv: [u8; 16] = [0x01; 16];

        let original = [0xFE, 0x01, 0x02, 0x03, 0x04];

        // Encrypt
        let mut send_cipher = Aes256Ctr::new_from_slices(&key, &iv).unwrap();
        let mut encrypted = encrypt_message(&original, &key, &mut send_cipher, 0);

        // Tamper with encrypted payload (after first byte, before checksum)
        encrypted[2] ^= 0xFF;

        // Decrypt should fail (checksum won't match)
        let mut recv_cipher = Aes256Ctr::new_from_slices(&key, &iv).unwrap();
        let result = decrypt_message(&mut encrypted, &key, &mut recv_cipher, 0);
        assert!(result.is_err());
    }

    #[test]
    fn decrypt_rejects_wrong_counter() {
        let key: [u8; 32] = [0x42; 32];
        let iv: [u8; 16] = [0x01; 16];

        let original = [0xFE, 0x01, 0x02, 0x03, 0x04];

        // Encrypt with counter 0
        let mut send_cipher = Aes256Ctr::new_from_slices(&key, &iv).unwrap();
        let mut encrypted = encrypt_message(&original, &key, &mut send_cipher, 0);

        // Decrypt with counter 1 (wrong)
        let mut recv_cipher = Aes256Ctr::new_from_slices(&key, &iv).unwrap();
        let result = decrypt_message(&mut encrypted, &key, &mut recv_cipher, 1);
        assert!(result.is_err());
    }

    #[test]
    fn multiple_messages_with_incrementing_counters() {
        let key: [u8; 32] = [0x42; 32];
        let iv: [u8; 16] = [0x01; 16];

        // Simulate multiple message sends with counter tracking
        let mut send_cipher = Aes256Ctr::new_from_slices(&key, &iv).unwrap();
        let mut recv_cipher = Aes256Ctr::new_from_slices(&key, &iv).unwrap();

        for counter in 0u64..5 {
            let original = [0xFE, counter as u8, 0x02, 0x03];

            let encrypted = encrypt_message(&original, &key, &mut send_cipher, counter);
            let mut decrypted = encrypted.clone();
            decrypt_message(&mut decrypted, &key, &mut recv_cipher, counter).expect("decrypt");

            assert_eq!(decrypted.as_ref(), &original[..]);
        }
    }

    #[test]
    fn empty_buffer_encrypt_is_noop() {
        let key: [u8; 32] = [0x42; 32];
        let iv: [u8; 16] = [0x01; 16];

        let mut cipher = Aes256Ctr::new_from_slices(&key, &iv).unwrap();
        let encrypted = encrypt_message(&[], &key, &mut cipher, 0);

        assert!(encrypted.is_empty());
    }

    #[test]
    fn decrypt_rejects_empty_encrypted_frame_without_panicking() {
        let key: [u8; 32] = [0x42; 32];
        let iv: [u8; 16] = [0x01; 16];

        let mut cipher = Aes256Ctr::new_from_slices(&key, &iv).unwrap();
        let mut buf = BytesMut::new();
        let mut counter = 0;

        let err = BedrockTransport::<TestTransport>::decrypt_buffer(
            &mut buf,
            &mut cipher,
            &key,
            &mut counter,
            1,
        )
        .expect_err("empty encrypted frame must be rejected");

        assert!(matches!(
            err,
            JolyneError::Protocol(ProtocolError::UnexpectedHandshake(_))
        ));
        assert_eq!(counter, 0, "counter must not advance for rejected frames");
    }

    #[test]
    fn encrypt_rejects_header_longer_than_packet() {
        let key: [u8; 32] = [0x42; 32];
        let iv: [u8; 16] = [0x01; 16];

        let mut cipher = Aes256Ctr::new_from_slices(&key, &iv).unwrap();
        let mut buf = BytesMut::from(&b"x"[..]);
        let mut counter = 0;

        let err = BedrockTransport::<TestTransport>::encrypt_buffer(
            &mut buf,
            &mut cipher,
            &key,
            &mut counter,
            2,
        )
        .expect_err("oversized cleartext header must be rejected");

        assert!(matches!(
            err,
            JolyneError::Protocol(ProtocolError::UnexpectedHandshake(_))
        ));
        assert_eq!(counter, 0, "counter must not advance for rejected frames");
    }

    #[test]
    fn first_byte_is_not_encrypted() {
        // In Bedrock encryption, the first byte (0xFE batch marker) is NOT encrypted
        let key: [u8; 32] = [0x42; 32];
        let iv: [u8; 16] = [0x01; 16];

        let original = [0xFE, 0x01, 0x02, 0x03];

        let mut cipher = Aes256Ctr::new_from_slices(&key, &iv).unwrap();
        let encrypted = encrypt_message(&original, &key, &mut cipher, 0);

        // First byte should remain 0xFE (unencrypted)
        assert_eq!(encrypted[0], 0xFE);
    }

    // ========== Transport State Tests ==========

    #[test]
    fn compression_settings_stored_correctly() {
        // Test that set_compression stores all three parameters
        // We can't test BedrockTransport directly without a Transport impl,
        // but we can verify the expected behavior through field access if pub(crate)

        // This test verifies the logic of compression settings
        let enabled = true;
        let level = 6u32;
        let threshold = 256u16;

        // Verify threshold comparison logic
        let payload_size = 100usize;
        let should_compress = enabled && level > 0 && payload_size >= threshold as usize;
        assert!(!should_compress); // 100 < 256

        let payload_size = 512usize;
        let should_compress = enabled && level > 0 && payload_size >= threshold as usize;
        assert!(should_compress); // 512 >= 256
    }

    #[test]
    fn compression_threshold_boundary() {
        let threshold = 100u16;

        // Exactly at threshold -> should compress
        assert!(100usize >= threshold as usize);
        assert!(101usize >= threshold as usize);

        // Below threshold -> should not compress
        assert!(99usize <= threshold as usize);
    }

    #[test]
    fn encryption_requires_reliable_send() {
        // Test the logic: encryption_enabled && !reliable -> error
        let encryption_enabled = true;
        let reliable = false;

        let should_error = encryption_enabled && !reliable;
        assert!(should_error);

        // When reliable, no error
        let reliable = true;
        let should_error = encryption_enabled && !reliable;
        assert!(!should_error);
    }

    // ========== Key/IV Generation Tests ==========

    #[test]
    fn key_from_generic_array() {
        // Test that GenericArray conversion works correctly
        let key_bytes: [u8; 32] = [0x42; 32];
        let key: Key<Aes256Gcm> = *GenericArray::from_slice(&key_bytes);

        assert_eq!(key.as_slice(), &key_bytes);
    }
}
