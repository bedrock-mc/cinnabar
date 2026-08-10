use std::io;
use std::path::PathBuf;

/// Errors produced by the local Rust-to-Go bridge.
#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    /// Reading endpoint metadata or a published endpoint file failed.
    #[error("failed to read bridge endpoint {path}: {source}")]
    EndpointRead {
        /// Endpoint path that could not be read.
        path: PathBuf,
        /// Underlying filesystem error.
        #[source]
        source: io::Error,
    },

    /// An endpoint did not match the local bridge security contract.
    #[error("invalid bridge endpoint {path}: {reason}")]
    InvalidEndpoint {
        /// Endpoint path that failed validation.
        path: PathBuf,
        /// Human-readable validation failure.
        reason: String,
    },

    /// The connected byte stream failed.
    #[error("bridge I/O error: {0}")]
    Io(#[from] io::Error),

    /// A control request or response was not valid JSON.
    #[error("invalid control JSON: {0}")]
    ControlJson(#[from] serde_json::Error),

    /// A control response violated the Status v1 or JSON-RPC contract.
    #[error("invalid control response: {reason}")]
    InvalidControlResponse {
        /// Contract violation detected by the client.
        reason: &'static str,
    },

    /// The control endpoint closed before returning a response.
    #[error("control endpoint closed before returning a response")]
    ControlClosed,

    /// The control endpoint returned a JSON-RPC error.
    #[error("control request failed with JSON-RPC error {code}: {message}")]
    ControlRpc {
        /// Standard or server-defined JSON-RPC error code.
        code: i64,
        /// Human-readable JSON-RPC error message.
        message: String,
    },

    /// Empty frames are not part of the bridge protocol.
    #[error("bridge frame length must be positive")]
    ZeroLengthFrame,

    /// A frame exceeded the configured payload limit.
    #[error("bridge frame is {length} bytes; maximum is {maximum}")]
    FrameTooLarge {
        /// Rejected payload length.
        length: usize,
        /// Configured maximum payload length.
        maximum: usize,
    },

    /// EOF arrived after only part of a frame.
    #[error("truncated bridge frame: received {received} of {expected} bytes")]
    TruncatedFrame {
        /// Number of bytes required for the current header or payload.
        expected: usize,
        /// Number of bytes received before EOF.
        received: usize,
    },
}
