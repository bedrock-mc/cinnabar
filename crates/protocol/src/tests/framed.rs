//! Concrete framed peers for browser/gateway protocol controls.

pub(crate) type Reader = bridge::FramedReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>;

/// Keeps the original writer owned until its test finishes or cancels it.
pub(crate) struct Writer(tokio::task::JoinHandle<()>);

impl Drop for Writer {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Prepares the same bounded codec and writer against a concrete in-memory byte stream.
pub(crate) fn pair() -> (Reader, bridge::FrameQueue, tokio::io::DuplexStream, Writer) {
    let (client, peer) = tokio::io::duplex(64 * 1024);
    let (read, write) = tokio::io::split(client);
    let (reader, frames, writer) =
        bridge::tasks::prepare_connection(read, write, bridge::MAX_FRAME_LEN);
    (reader, frames, peer, Writer(tokio::spawn(writer)))
}
