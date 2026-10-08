use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use std::task::{Context, Poll};

use bytes::{Bytes, BytesMut};
use futures::{Sink, SinkExt, Stream};
use tokio::sync::{mpsc, oneshot};
use tokio_util::codec::{Decoder, Encoder, Framed, FramedRead, FramedWrite, LengthDelimitedCodec};

use crate::BridgeError;
use crate::endpoint::{PlatformReadHalf, PlatformStream, PlatformWriteHalf};

pub(crate) struct BridgeCodec {
    inner: LengthDelimitedCodec,
    expected_payload: Option<usize>,
    maximum: usize,
}

impl BridgeCodec {
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::with_max(crate::MAX_FRAME_LEN)
    }

    pub(crate) fn with_max(maximum: usize) -> Self {
        let inner = LengthDelimitedCodec::builder()
            .big_endian()
            .length_field_offset(0)
            .length_field_type::<u32>()
            .length_adjustment(0)
            .num_skip(4)
            .max_frame_length(maximum)
            .new_codec();
        Self {
            inner,
            expected_payload: None,
            maximum,
        }
    }
}

impl Decoder for BridgeCodec {
    type Item = Bytes;
    type Error = BridgeError;

    fn decode(&mut self, source: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if self.expected_payload.is_none() {
            if source.len() < 4 {
                return Ok(None);
            }
            let length = u32::from_be_bytes(source[..4].try_into().expect("four-byte header"));
            let length = length as usize;
            validate_frame_length(length, self.maximum)?;
            self.expected_payload = Some(length);
        }

        match self.inner.decode(source).map_err(BridgeError::Io)? {
            Some(frame) => {
                self.expected_payload = None;
                Ok(Some(frame.freeze()))
            }
            None => Ok(None),
        }
    }

    fn decode_eof(&mut self, source: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if let Some(frame) = self.decode(source)? {
            return Ok(Some(frame));
        }
        if let Some(expected) = self.expected_payload {
            return Err(BridgeError::TruncatedFrame {
                expected,
                received: source.len(),
            });
        }
        if !source.is_empty() {
            return Err(BridgeError::TruncatedFrame {
                expected: 4,
                received: source.len(),
            });
        }
        Ok(None)
    }
}

impl Encoder<Bytes> for BridgeCodec {
    type Error = BridgeError;

    fn encode(&mut self, item: Bytes, destination: &mut BytesMut) -> Result<(), Self::Error> {
        validate_frame_length(item.len(), self.maximum)?;
        self.inner
            .encode(item, destination)
            .map_err(BridgeError::Io)
    }
}

fn validate_frame_length(length: usize, maximum: usize) -> Result<(), BridgeError> {
    if length == 0 {
        return Err(BridgeError::ZeroLengthFrame);
    }
    if length > maximum {
        return Err(BridgeError::FrameTooLarge { length, maximum });
    }
    Ok(())
}

/// A local byte stream framed as unsigned 32-bit big-endian payloads.
pub struct FramedStream {
    inner: Framed<PlatformStream, BridgeCodec>,
}

impl FramedStream {
    pub(crate) fn with_max(stream: PlatformStream, maximum: usize) -> Self {
        Self {
            inner: Framed::new(stream, BridgeCodec::with_max(maximum)),
        }
    }
}

/// Frames accepted but not yet written; a stalled reader backs up into the senders, not memory.
const QUEUED_FRAMES: usize = 16;

/// Splits a fresh stream so a spawned writer task owns its write half.
///
/// Requires a Tokio runtime. The writer exits once every [`FrameQueue`] clone is dropped.
pub(crate) fn queued(stream: PlatformStream, maximum: usize) -> (FramedReader, FrameQueue) {
    let (read, write) = stream.into_split();
    let (frames, queued) = mpsc::channel(QUEUED_FRAMES);
    let failure = Arc::new(OnceLock::new());
    tokio::spawn(write_queued_frames(
        FramedWrite::new(write, BridgeCodec::with_max(maximum)),
        queued,
        Arc::clone(&failure),
    ));
    (
        FramedReader {
            inner: FramedRead::new(read, BridgeCodec::with_max(maximum)),
        },
        FrameQueue {
            frames,
            failure,
            maximum,
        },
    )
}

/// The read half of a queued game connection.
pub struct FramedReader {
    inner: FramedRead<PlatformReadHalf, BridgeCodec>,
}

impl Stream for FramedReader {
    type Item = Result<Bytes, BridgeError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.get_mut().inner).poll_next(cx)
    }
}

struct QueuedFrame {
    frame: Bytes,
    written: oneshot::Sender<Result<(), String>>,
}

/// Queues whole frames for one connection's writer task; every clone shares one bounded FIFO.
#[derive(Clone)]
pub struct FrameQueue {
    frames: mpsc::Sender<QueuedFrame>,
    failure: Arc<OnceLock<String>>,
    maximum: usize,
}

/// Resolves once an accepted frame has been flushed to the socket.
pub struct FrameWritten {
    written: oneshot::Receiver<Result<(), String>>,
    failure: Arc<OnceLock<String>>,
}

impl Future for FrameWritten {
    type Output = Result<(), BridgeError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let failure = Arc::clone(&self.failure);
        Pin::new(&mut self.written)
            .poll(cx)
            .map(|result| match result {
                Ok(Ok(())) => Ok(()),
                Ok(Err(reason)) => Err(broken_pipe(&reason)),
                Err(_) => Err(writer_stopped(&failure)),
            })
    }
}

impl FrameQueue {
    /// Writes `frame` after every frame accepted before it and resolves once it is flushed.
    pub async fn send(&self, frame: Bytes) -> Result<(), BridgeError> {
        self.accept(frame).await?.await
    }

    /// Waits for queue capacity, then accepts `frame`; the returned future tracks its write.
    pub async fn accept(&self, frame: Bytes) -> Result<FrameWritten, BridgeError> {
        validate_frame_length(frame.len(), self.maximum)?;
        let permit = self
            .frames
            .reserve()
            .await
            .map_err(|_| writer_stopped(&self.failure))?;
        let (written, receiver) = oneshot::channel();
        permit.send(QueuedFrame { frame, written });
        Ok(FrameWritten {
            written: receiver,
            failure: Arc::clone(&self.failure),
        })
    }
}

fn writer_stopped(failure: &OnceLock<String>) -> BridgeError {
    broken_pipe(
        failure
            .get()
            .map_or("bridge writer stopped", String::as_str),
    )
}

fn broken_pipe(reason: &str) -> BridgeError {
    BridgeError::Io(io::Error::new(io::ErrorKind::BrokenPipe, reason.to_owned()))
}

/// Writes queued frames in order, coalescing whatever is already queued into one flush, and
/// acknowledges each frame only after that flush.
async fn write_queued_frames(
    mut writer: FramedWrite<PlatformWriteHalf, BridgeCodec>,
    mut frames: mpsc::Receiver<QueuedFrame>,
    failure: Arc<OnceLock<String>>,
) {
    let mut pending = Vec::with_capacity(QUEUED_FRAMES);
    while let Some(first) = frames.recv().await {
        let mut written = writer.feed(first.frame).await;
        pending.push(first.written);
        while written.is_ok()
            && let Ok(next) = frames.try_recv()
        {
            written = writer.feed(next.frame).await;
            pending.push(next.written);
        }
        let flushed = match written {
            Ok(()) => writer.flush().await,
            Err(error) => Err(error),
        };
        let result = flushed.map_err(|error| error.to_string());
        if let Err(reason) = &result {
            let _ = failure.set(reason.clone());
        }
        for written in pending.drain(..) {
            let _ = written.send(result.clone());
        }
        if result.is_err() {
            return;
        }
    }
}

impl Stream for FramedStream {
    type Item = Result<Bytes, BridgeError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.get_mut().inner).poll_next(cx)
    }
}

impl Sink<Bytes> for FramedStream {
    type Error = BridgeError;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.get_mut().inner).poll_ready(cx)
    }

    fn start_send(self: Pin<&mut Self>, item: Bytes) -> Result<(), Self::Error> {
        Pin::new(&mut self.get_mut().inner).start_send(item)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.get_mut().inner).poll_close(cx)
    }
}

#[cfg(test)]
mod tests {
    use bytes::{BufMut, Bytes, BytesMut};
    use tokio_util::codec::{Decoder, Encoder};

    use super::{BridgeCodec, validate_frame_length};
    use crate::{BridgeError, MAX_FRAME_LEN};

    /// Clones of the queue share one writer that keeps the order frames were accepted in,
    /// while the read half keeps receiving.
    #[cfg(unix)]
    #[tokio::test]
    async fn queued_connection_writes_in_acceptance_order_while_reading() {
        use futures::StreamExt;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let (local, mut peer) = tokio::net::UnixStream::pair().unwrap();
        let (mut reader, queue) =
            super::queued(crate::endpoint::PlatformStream::Unix(local), MAX_FRAME_LEN);
        peer.write_all(&wire_frame(&[2])).await.unwrap();
        let session = queue.clone();
        let first = queue.accept(Bytes::from_static(&[3])).await.unwrap();
        let second = session.accept(Bytes::from_static(&[4])).await.unwrap();
        let third = queue.accept(Bytes::from_static(&[5])).await.unwrap();

        assert_eq!(&reader.next().await.unwrap().unwrap()[..], [2]);
        let mut written = [0; 15];
        peer.read_exact(&mut written).await.unwrap();
        let mut expected = wire_frame(&[3]);
        expected.extend_from_slice(&wire_frame(&[4]));
        expected.extend_from_slice(&wire_frame(&[5]));
        assert_eq!(&written[..], &expected[..]);
        for written in [first, second, third] {
            written.await.unwrap();
        }
    }

    /// A send completes only once its frame is flushed, and fails when the write does.
    #[cfg(unix)]
    #[tokio::test]
    async fn send_resolves_only_after_its_frame_is_flushed() {
        use futures::FutureExt;
        use tokio::io::AsyncReadExt;

        let (local, mut peer) = tokio::net::UnixStream::pair().unwrap();
        let (_reader, queue) =
            super::queued(crate::endpoint::PlatformStream::Unix(local), MAX_FRAME_LEN);
        let payload = Bytes::from(vec![7; 4 * 1024 * 1024]);
        let mut send = Box::pin(queue.send(payload.clone()));
        for _ in 0..64 {
            assert!(
                (&mut send).now_or_never().is_none(),
                "a frame the peer has not drained must not report written"
            );
            tokio::task::yield_now().await;
        }

        let mut drained = vec![0; 4 + payload.len()];
        let (read, sent) = tokio::join!(peer.read_exact(&mut drained), send);
        read.unwrap();
        sent.unwrap();

        drop(peer);
        let mut failed = Ok(());
        for _ in 0..8 {
            failed = queue.send(Bytes::from_static(&[1])).await;
            if failed.is_err() {
                break;
            }
        }
        assert!(failed.is_err(), "a write to a closed peer must fail");
    }

    /// A reader that stops draining backs up into the senders instead of an unbounded backlog.
    #[cfg(unix)]
    #[tokio::test]
    async fn stalled_reader_bounds_accepted_frames() {
        use futures::FutureExt;

        let (local, _peer) = tokio::net::UnixStream::pair().unwrap();
        let (_reader, queue) =
            super::queued(crate::endpoint::PlatformStream::Unix(local), MAX_FRAME_LEN);
        let frame = Bytes::from(vec![7; 64 * 1024]);
        let mut accepted = Vec::new();
        let mut backed_up = false;
        // 4096 frames of 64 KiB is 256 MiB, far beyond any socket buffer.
        for _ in 0..4096 {
            match queue.accept(frame.clone()).now_or_never() {
                Some(written) => accepted.push(written.unwrap()),
                None => {
                    backed_up = true;
                    break;
                }
            }
            for _ in 0..4 {
                tokio::task::yield_now().await;
            }
        }

        assert!(
            backed_up,
            "accepted {} frames for a stalled reader without backing up",
            accepted.len()
        );
    }

    fn wire_frame(payload: &[u8]) -> BytesMut {
        let mut wire = BytesMut::with_capacity(4 + payload.len());
        wire.put_u32(payload.len() as u32);
        wire.extend_from_slice(payload);
        wire
    }

    #[test]
    fn encode_writes_u32_big_endian_payload_length() {
        let mut codec = BridgeCodec::new();
        let mut wire = BytesMut::new();

        codec
            .encode(Bytes::from_static(&[0xfe, 0x01]), &mut wire)
            .expect("encode frame");

        assert_eq!(&wire[..], &[0x00, 0x00, 0x00, 0x02, 0xfe, 0x01]);
    }

    #[test]
    fn decode_preserves_fifo_order_and_returns_immutable_bytes() {
        let mut codec = BridgeCodec::new();
        let mut wire = wire_frame(&[0xfe, 0x01]);
        wire.extend_from_slice(&wire_frame(&[0xfe, 0x02]));

        let first: Bytes = codec
            .decode(&mut wire)
            .expect("decode first frame")
            .expect("first frame");
        let second: Bytes = codec
            .decode(&mut wire)
            .expect("decode second frame")
            .expect("second frame");

        assert_eq!(&first[..], &[0xfe, 0x01]);
        assert_eq!(&second[..], &[0xfe, 0x02]);
        assert!(
            codec
                .decode(&mut wire)
                .expect("decode empty buffer")
                .is_none()
        );
    }

    #[test]
    fn decode_rejects_zero_length_frame() {
        let mut codec = BridgeCodec::new();
        let mut wire = BytesMut::from(&[0, 0, 0, 0][..]);

        let error = codec.decode(&mut wire).expect_err("zero frame must fail");

        assert!(matches!(error, BridgeError::ZeroLengthFrame));
    }

    #[test]
    fn encode_rejects_zero_length_frame() {
        let mut codec = BridgeCodec::new();
        let mut wire = BytesMut::new();

        let error = codec
            .encode(Bytes::new(), &mut wire)
            .expect_err("zero frame must fail");

        assert!(matches!(error, BridgeError::ZeroLengthFrame));
        assert!(wire.is_empty());
    }

    #[test]
    fn decode_rejects_oversized_length_before_payload_arrives() {
        let mut codec = BridgeCodec::new();
        let mut wire = BytesMut::new();
        wire.put_u32((MAX_FRAME_LEN + 1) as u32);

        let error = codec
            .decode(&mut wire)
            .expect_err("oversized frame must fail");

        assert!(matches!(
            error,
            BridgeError::FrameTooLarge {
                length,
                maximum
            } if length == MAX_FRAME_LEN + 1 && maximum == MAX_FRAME_LEN
        ));
        assert_eq!(wire.len(), 4);
    }

    #[test]
    fn encode_rejects_oversized_payload() {
        let mut codec = BridgeCodec::new();
        let mut wire = BytesMut::new();
        let payload = Bytes::from(vec![0; MAX_FRAME_LEN + 1]);

        let error = codec
            .encode(payload, &mut wire)
            .expect_err("oversized frame must fail");

        assert!(matches!(
            error,
            BridgeError::FrameTooLarge {
                length,
                maximum
            } if length == MAX_FRAME_LEN + 1 && maximum == MAX_FRAME_LEN
        ));
        assert!(wire.is_empty());
    }

    #[test]
    fn maximum_frame_length_is_accepted() {
        assert!(validate_frame_length(MAX_FRAME_LEN, MAX_FRAME_LEN).is_ok());
    }

    #[test]
    fn endpoint_specific_limit_rejects_a_larger_frame() {
        let mut codec = BridgeCodec::with_max(64 * 1024);
        let mut wire = BytesMut::new();
        wire.put_u32(64 * 1024 + 1);

        let error = codec.decode(&mut wire).expect_err("frame must fail");
        assert!(matches!(
            error,
            BridgeError::FrameTooLarge {
                length: 65_537,
                maximum: 65_536
            }
        ));
    }

    #[test]
    fn decode_eof_rejects_partial_header() {
        let mut codec = BridgeCodec::new();
        let mut wire = BytesMut::from(&[0, 0][..]);

        let error = codec
            .decode_eof(&mut wire)
            .expect_err("partial header must fail");

        assert!(matches!(
            error,
            BridgeError::TruncatedFrame {
                expected: 4,
                received: 2
            }
        ));
    }

    #[test]
    fn decode_eof_rejects_header_without_payload() {
        let mut codec = BridgeCodec::new();
        let mut wire = BytesMut::from(&[0, 0, 0, 3][..]);

        assert!(codec.decode(&mut wire).expect("decode header").is_none());
        assert!(wire.is_empty());
        let error = codec
            .decode_eof(&mut wire)
            .expect_err("missing payload must fail");

        assert!(matches!(
            error,
            BridgeError::TruncatedFrame {
                expected: 3,
                received: 0
            }
        ));
    }

    #[test]
    fn decode_eof_rejects_partial_payload() {
        let mut codec = BridgeCodec::new();
        let mut wire = BytesMut::from(&[0, 0, 0, 3, 0xfe][..]);

        assert!(
            codec
                .decode(&mut wire)
                .expect("decode partial frame")
                .is_none()
        );
        let error = codec
            .decode_eof(&mut wire)
            .expect_err("partial payload must fail");

        assert!(matches!(
            error,
            BridgeError::TruncatedFrame {
                expected: 3,
                received: 1
            }
        ));
    }

    #[test]
    fn decode_eof_between_frames_is_clean() {
        let mut codec = BridgeCodec::new();
        let mut wire = BytesMut::new();

        assert!(codec.decode_eof(&mut wire).expect("clean EOF").is_none());
    }
}
