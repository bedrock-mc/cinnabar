//! One retained codec across the private driver's actual settings handoff.

use bedrock_net::{BatchCodec, BatchError, DecodeOutcome, Negotiated};
use bytes::Bytes;

/// Dynamic storage for the same codec before and after its consuming configuration.
pub(super) enum Codec {
    Plain(BatchCodec),
    Negotiated(BatchCodec<Negotiated>),
}

impl Codec {
    /// Encodes on the actual current phase without copying or resetting its settings.
    pub(super) fn encode(&self, packets: &[Bytes]) -> Result<Vec<Bytes>, BatchError> {
        match self {
            Self::Plain(codec) => codec.encode(packets),
            Self::Negotiated(codec) => codec.encode(packets),
        }
    }

    /// Decodes using the original envelope and the settings installed by the handshake.
    pub(super) fn decode(&self, message: Bytes) -> Result<DecodeOutcome, BatchError> {
        match self {
            Self::Plain(codec) => codec.decode(message),
            Self::Negotiated(codec) => codec.decode(message),
        }
    }
}
