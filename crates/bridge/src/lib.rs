//! Local stream bridge between the Rust client and Go core.

mod endpoint;
mod error;
mod framed;

use std::path::Path;

pub use error::BridgeError;
pub use framed::FramedStream;

/// Largest payload accepted by the local bridge framing protocol.
pub const MAX_FRAME_LEN: usize = 64 * 1024 * 1024;

/// Connects to the local Go core endpoint published in `socket_dir`.
pub async fn connect(socket_dir: &Path) -> anyhow::Result<FramedStream> {
    let stream = endpoint::connect(socket_dir).await?;
    Ok(FramedStream::new(stream))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use bytes::Bytes;
    use futures::{Sink, Stream};

    use super::{BridgeError, FramedStream, connect};

    fn assert_transport<T>()
    where
        T: Stream<Item = Result<Bytes, BridgeError>>
            + Sink<Bytes, Error = BridgeError>
            + Unpin
            + Send,
    {
    }

    #[test]
    fn public_transport_contract_is_stable() {
        assert_transport::<FramedStream>();
        let _ = connect;
    }

    #[tokio::test]
    async fn connect_preserves_bridge_error_in_anyhow_result() {
        let error = match connect(Path::new("")).await {
            Ok(_) => panic!("empty socket directory must fail"),
            Err(error) => error,
        };

        assert!(matches!(
            error.downcast_ref::<BridgeError>(),
            Some(BridgeError::InvalidEndpoint { .. })
        ));
    }
}
