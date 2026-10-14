use crate::{Channel, CloseReason, RelayTransport};
use bridge::contract::{
    RELAY_CHANNEL_CONTROL, RELAY_CONTROL_BYTES, RELAY_CONTROL_PATH, RELAY_PLAY_MESSAGE_BYTES,
    RELAY_PLAY_PATH, RELAY_SOCKET_PROTOCOL, RELAY_TRANSPORT_PATH,
};
use js_sys::{Promise, Uint8Array};
use std::{
    future::Future,
    io,
    pin::Pin,
    task::{Context, Poll, ready},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen(module = "/src/adapters.mjs")]
extern "C" {
    fn available() -> bool;
    #[wasm_bindgen(catch)]
    fn create(origin: &str, quic: bool, config: &str) -> Result<JsValue, JsValue>;
    fn open(transport: &JsValue, name: &str) -> Promise;
    fn read(channel: &JsValue) -> Promise;
    fn write(channel: &JsValue, bytes: &Uint8Array) -> Promise;
    fn flush(channel: &JsValue) -> Promise;
    fn shutdown(channel: &JsValue) -> Promise;
    #[wasm_bindgen(js_name = closeChannel)]
    fn close_channel(channel: &JsValue);
    fn close(transport: &JsValue);
    fn maximum(transport: &JsValue) -> usize;
    #[wasm_bindgen(js_name = sendDatagram)]
    fn send_datagram(transport: &JsValue, bytes: &Uint8Array) -> Promise;
    #[wasm_bindgen(js_name = recvDatagram)]
    fn recv_datagram(transport: &JsValue) -> Promise;
}

/// Event-loop-owned QUIC candidate or WebSocket fallback; no thread-mobility wrapper.
pub struct BrowserTransport(JsValue);
impl BrowserTransport {
    /// Reports constructor availability; successful Hello still decides whether QUIC works.
    pub fn quic_available() -> bool {
        available()
    }
    /// Creates a cancellable candidate with wire values supplied by the generated contract.
    pub fn new(origin: &str, quic: bool) -> Result<Self, CloseReason> {
        let config = serde_json::json!({"transport": RELAY_TRANSPORT_PATH,
            "controlPath": RELAY_CONTROL_PATH, "playPath": RELAY_PLAY_PATH,
            "control": RELAY_CHANNEL_CONTROL, "protocol": RELAY_SOCKET_PROTOCOL,
            "controlBytes": RELAY_CONTROL_BYTES, "playBytes": RELAY_PLAY_MESSAGE_BYTES, "chunks": 16});
        create(origin, quic, &config.to_string())
            .map(Self)
            .map_err(|_| CloseReason::Transport)
    }
}
impl RelayTransport for BrowserTransport {
    type Stream = BrowserStream;
    async fn open(&mut self, channel: Channel) -> Result<BrowserStream, CloseReason> {
        let handle = JsFuture::from(open(&self.0, channel.name()))
            .await
            .map_err(|_| CloseReason::Transport)?;
        Ok(BrowserStream {
            handle,
            buffer: Uint8Array::new_with_length(0),
            offset: 0,
            reading: None,
            writing: None,
            flushing: None,
            eof: false,
            shutting_down: None,
            finished: false,
            shutdown_started: false,
        })
    }
    fn max_datagram_size(&self) -> Option<usize> {
        let size = maximum(&self.0);
        (size > 0).then_some(size)
    }
    async fn send_datagram(&mut self, bytes: &[u8]) -> Result<(), CloseReason> {
        if self.max_datagram_size().is_none_or(|max| bytes.len() > max) {
            return Err(CloseReason::Protocol);
        }
        JsFuture::from(send_datagram(&self.0, &Uint8Array::from(bytes)))
            .await
            .map(|_| ())
            .map_err(|_| CloseReason::Transport)
    }
    async fn recv_datagram(&mut self) -> Result<Option<Vec<u8>>, CloseReason> {
        let value = JsFuture::from(recv_datagram(&self.0))
            .await
            .map_err(|_| CloseReason::Transport)?;
        if value.is_null() {
            return Ok(None);
        }
        let bytes = value
            .dyn_into::<Uint8Array>()
            .map_err(|_| CloseReason::Protocol)?;
        if self
            .max_datagram_size()
            .is_none_or(|max| bytes.length() as usize > max)
        {
            return Err(CloseReason::Protocol);
        }
        Ok(Some(bytes.to_vec()))
    }
    fn close(&mut self, _reason: CloseReason) {
        close(&self.0);
    }
}
impl Drop for BrowserTransport {
    fn drop(&mut self) {
        close(&self.0);
    }
}

/// Reliable byte stream with retained promises and one size-checked JS receive chunk.
pub struct BrowserStream {
    handle: JsValue,
    buffer: Uint8Array,
    offset: u32,
    eof: bool,
    reading: Option<JsFuture>,
    writing: Option<JsFuture>,
    flushing: Option<JsFuture>,
    shutting_down: Option<JsFuture>,
    finished: bool,
    shutdown_started: bool,
}
/// Hides browser exception details from public errors.
fn io_error(_: JsValue) -> io::Error {
    io::Error::other("relay I/O failed")
}
/// Clears one retained I/O promise only after completion, preserving its registered wake.
fn poll_io(future: &mut Option<JsFuture>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
    let result = if let Some(pending) = future {
        ready!(Pin::new(pending).poll(cx))
            .map(|_| ())
            .map_err(io_error)
    } else {
        Ok(())
    };
    *future = None;
    Poll::Ready(result)
}
impl AsyncRead for BrowserStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        out: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if out.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        while this.offset == this.buffer.length() && !this.eof {
            let reading = this
                .reading
                .get_or_insert_with(|| JsFuture::from(read(&this.handle)));
            let result = ready!(Pin::new(reading).poll(cx));
            this.reading = None;
            let value = result.map_err(io_error)?;
            if value.is_null() {
                this.eof = true;
                break;
            }
            let array = value.dyn_into::<Uint8Array>().map_err(io_error)?;
            if array.length() as usize > RELAY_PLAY_MESSAGE_BYTES {
                return Poll::Ready(Err(io::Error::other("relay chunk limit")));
            }
            this.buffer = array;
            this.offset = 0;
        }
        let count = out
            .remaining()
            .min((this.buffer.length() - this.offset) as usize);
        this.buffer
            .subarray(this.offset, this.offset + count as u32)
            .copy_to(out.initialize_unfilled_to(count));
        out.advance(count);
        this.offset += count as u32;
        Poll::Ready(Ok(()))
    }
}
impl AsyncWrite for BrowserStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if this.shutdown_started {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "relay stream shut down",
            )));
        }
        ready!(poll_io(&mut this.writing, cx))?;
        ready!(poll_io(&mut this.flushing, cx))?;
        let count = bytes.len().min(64 * 1024);
        if count != 0 {
            this.writing = Some(JsFuture::from(write(
                &this.handle,
                &Uint8Array::from(&bytes[..count]),
            )));
        }
        // Report local acceptance once; a later poll never reports an earlier buffer's length.
        Poll::Ready(Ok(count))
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.finished {
            return Poll::Ready(Ok(()));
        }
        ready!(poll_io(&mut this.writing, cx))?;
        this.flushing
            .get_or_insert_with(|| JsFuture::from(flush(&this.handle)));
        poll_io(&mut this.flushing, cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.finished {
            return Poll::Ready(Ok(()));
        }
        this.shutdown_started = true;
        ready!(poll_io(&mut this.writing, cx))?;
        ready!(poll_io(&mut this.flushing, cx))?;
        this.shutting_down
            .get_or_insert_with(|| JsFuture::from(shutdown(&this.handle)));
        ready!(poll_io(&mut this.shutting_down, cx))?;
        this.finished = true;
        Poll::Ready(Ok(()))
    }
}
impl Drop for BrowserStream {
    fn drop(&mut self) {
        close_channel(&self.handle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{ManuallyDrop, MaybeUninit};

    #[test]
    fn small_receive_initializes_only_delivered_bytes() {
        let mut stream = ManuallyDrop::new(BrowserStream {
            handle: JsValue::NULL,
            buffer: Uint8Array::from(&[1, 2][..]),
            offset: 0,
            eof: false,
            reading: None,
            writing: None,
            flushing: None,
            shutting_down: None,
            finished: false,
            shutdown_started: false,
        });
        let mut storage = [MaybeUninit::uninit(); 4096];
        let mut out = ReadBuf::uninit(&mut storage);
        let mut cx = Context::from_waker(futures::task::noop_waker_ref());
        assert!(matches!(
            Pin::new(&mut *stream).poll_read(&mut cx, &mut out),
            Poll::Ready(Ok(()))
        ));
        assert_eq!(out.filled(), &[1, 2]);
        assert_eq!(out.initialized().len(), 2);
    }
}
