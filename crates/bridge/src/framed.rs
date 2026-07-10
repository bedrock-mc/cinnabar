use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::{Bytes, BytesMut};
use futures::{Sink, Stream};
use tokio_util::codec::{Decoder, Encoder, Framed, LengthDelimitedCodec};

use crate::endpoint::PlatformStream;
use crate::{BridgeError, MAX_FRAME_LEN};

pub(crate) struct BridgeCodec {
    inner: LengthDelimitedCodec,
    expected_payload: Option<usize>,
}

impl BridgeCodec {
    pub(crate) fn new() -> Self {
        let inner = LengthDelimitedCodec::builder()
            .big_endian()
            .length_field_offset(0)
            .length_field_type::<u32>()
            .length_adjustment(0)
            .num_skip(4)
            .max_frame_length(MAX_FRAME_LEN)
            .new_codec();
        Self {
            inner,
            expected_payload: None,
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
            validate_frame_length(length)?;
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
        validate_frame_length(item.len())?;
        self.inner
            .encode(item, destination)
            .map_err(BridgeError::Io)
    }
}

fn validate_frame_length(length: usize) -> Result<(), BridgeError> {
    if length == 0 {
        return Err(BridgeError::ZeroLengthFrame);
    }
    if length > MAX_FRAME_LEN {
        return Err(BridgeError::FrameTooLarge {
            length,
            maximum: MAX_FRAME_LEN,
        });
    }
    Ok(())
}

/// A local byte stream framed as unsigned 32-bit big-endian payloads.
pub struct FramedStream {
    inner: Framed<PlatformStream, BridgeCodec>,
}

impl FramedStream {
    pub(crate) fn new(stream: PlatformStream) -> Self {
        Self {
            inner: Framed::new(stream, BridgeCodec::new()),
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
        assert!(validate_frame_length(MAX_FRAME_LEN).is_ok());
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
