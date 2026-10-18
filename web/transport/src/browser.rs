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
use wasm_bindgen::{JsCast, JsValue, prelude::wasm_bindgen};
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen(module = "/src/adapters.mjs")]
extern "C" {
    #[wasm_bindgen(catch)]
    fn create(origin: &str, quic: bool, config: &str) -> Result<JsValue, JsValue>;
    fn open(transport: &JsValue, name: &str, join: &str) -> Promise;
    #[wasm_bindgen(js_name = packChannel)]
    fn pack_channel(transport: &JsValue) -> JsValue;
    #[wasm_bindgen(js_name = acceptPacks)]
    fn accept_packs(transport: &JsValue, channel: &JsValue) -> Promise;
    #[wasm_bindgen(js_name = finishPlay)]
    fn finish_play(transport: &JsValue);
    fn read(channel: &JsValue) -> Promise;
    fn write(channel: &JsValue, bytes: &Uint8Array) -> Promise;
    fn flush(channel: &JsValue) -> Promise;
    fn shutdown(channel: &JsValue) -> Promise;
    #[wasm_bindgen(js_name = closeChannel)]
    fn close_channel(channel: &JsValue);
    fn close(transport: &JsValue);
}

/// Event-loop-owned QUIC candidate or WebSocket fallback; no thread-mobility wrapper.
pub struct BrowserTransport(JsValue);
impl BrowserTransport {
    /// Creates a cancellable candidate with wire values supplied by the generated contract.
    pub fn new(origin: &str, quic: bool) -> Result<Self, CloseReason> {
        let config = serde_json::json!({"transport": RELAY_TRANSPORT_PATH,
            "controlPath": RELAY_CONTROL_PATH, "playPath": RELAY_PLAY_PATH,
            "control": RELAY_CHANNEL_CONTROL, "protocol": RELAY_SOCKET_PROTOCOL,
            "controlBytes": RELAY_CONTROL_BYTES, "playBytes": RELAY_PLAY_MESSAGE_BYTES, "chunks": bridge::contract::QUEUED_FRAMES});
        create(origin, quic, &config.to_string())
            .map(Self)
            .map_err(|_| CloseReason::Transport)
    }
}
impl RelayTransport for BrowserTransport {
    type Stream = BrowserStream;
    async fn open(&mut self, channel: Channel) -> Result<BrowserStream, CloseReason> {
        let join = match channel {
            Channel::Control => String::new(),
            Channel::Play { join } => join.to_string(),
        };
        let handle = JsFuture::from(open(&self.0, channel.name(), &join))
            .await
            .map_err(|_| CloseReason::Transport)?;
        Ok(BrowserStream::new(handle))
    }
    fn accept_packs(
        &self,
    ) -> impl Future<Output = Result<Option<BrowserStream>, CloseReason>> + 'static + use<> {
        let transport = self.0.clone();
        let mut acceptance = PackAcceptance {
            channel: Some(pack_channel(&transport)),
        };
        async move {
            let handle = JsFuture::from(accept_packs(
                &transport,
                acceptance.channel.as_ref().expect("pending pack channel"),
            ))
            .await
            .map_err(|_| CloseReason::Transport)?;
            let _ = acceptance.channel.take();
            Ok((!handle.is_null()).then(|| BrowserStream::new(handle)))
        }
    }
    fn finish_play(&mut self) {
        finish_play(&self.0);
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

/// Cancels a pending archive wait even when its Rust future is dropped before first poll.
struct PackAcceptance {
    channel: Option<JsValue>,
}
impl Drop for PackAcceptance {
    fn drop(&mut self) {
        if let Some(channel) = &self.channel
            && !channel.is_null()
        {
            close_channel(channel);
        }
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
impl BrowserStream {
    /// Retains one newly locked browser byte reader and optional writer.
    fn new(handle: JsValue) -> Self {
        Self {
            handle,
            buffer: Uint8Array::new_with_length(0),
            offset: 0,
            eof: false,
            reading: None,
            writing: None,
            flushing: None,
            shutting_down: None,
            finished: false,
            shutdown_started: false,
        }
    }
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
        let count = crate::receive::copy_chunk(
            out,
            (this.buffer.length() - this.offset) as usize,
            |destination| {
                this.buffer
                    .subarray(this.offset, this.offset + destination.len() as u32)
                    .copy_to(destination);
            },
        );
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
