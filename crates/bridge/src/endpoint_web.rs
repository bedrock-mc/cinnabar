//! The browser's endpoints. A browser has no local core: its sessions run on a web relay
//! (`core/webrelay`), reached over a WebSocket that carries the same u32-framed session stream as a
//! local socket. The local endpoints do not exist here, so connecting to one fails.

use std::cell::RefCell;
use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};
use std::time::Duration;

use futures::Stream;
use futures::channel::{mpsc, oneshot};
use send_wrapper::SendWrapper;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{BinaryType, CloseEvent, Event, MessageEvent, WebSocket};

use crate::{BridgeError, RelaySession};

/// The relay's session path and WebSocket subprotocols, as `core/webrelay` defines them. The
/// session ID travels as a second subprotocol so it never appears in a URL.
const RELAY_SESSION_PATH: &str = "/v1/session";
const RELAY_SESSION_PROTOCOL: &str = "cinnabar.session.v1";
const RELAY_SESSION_ID_PREFIX: &str = "sid.";
/// Bytes the browser may hold unsent before writes wait for the socket to drain.
const MAX_BUFFERED_BYTES: u32 = 8 * 1024 * 1024;
/// How often a write waiting on a full send buffer checks it again; browsers raise no drain event.
const DRAIN_POLL: Duration = Duration::from_millis(4);

#[derive(Clone, Copy)]
pub(crate) enum EndpointKind {
    Control,
    Session,
}

impl EndpointKind {
    const fn name(self) -> &'static str {
        match self {
            Self::Control => "control",
            Self::Session => "session",
        }
    }
}

/// Names the endpoint a native core would publish; browsers have none.
pub(crate) fn endpoint_path(socket_dir: &Path, kind: EndpointKind) -> PathBuf {
    socket_dir.join(kind.name())
}

/// Browsers cannot reach a local core.
pub(crate) async fn connect(
    socket_dir: &Path,
    _kind: EndpointKind,
) -> Result<PlatformStream, BridgeError> {
    Err(BridgeError::InvalidEndpoint {
        path: socket_dir.to_path_buf(),
        reason: "local core endpoints are unavailable in a browser".to_owned(),
    })
}

enum Incoming {
    Data(Vec<u8>),
    Closed,
}

/// Owns the socket's event handlers; dropping the last owner detaches them and closes it. Halves
/// hold it in a [`SendWrapper`]: the browser runs this module on one thread, and the stream
/// traits shared with native transports require `Send`.
struct Socket {
    socket: WebSocket,
    _on_message: Closure<dyn FnMut(MessageEvent)>,
    _on_close: Closure<dyn FnMut(CloseEvent)>,
    _on_error: Closure<dyn FnMut(Event)>,
    _on_open: Closure<dyn FnMut(Event)>,
}

impl Drop for Socket {
    fn drop(&mut self) {
        self.socket.set_onmessage(None);
        self.socket.set_onclose(None);
        self.socket.set_onerror(None);
        self.socket.set_onopen(None);
        let _ = self.socket.close();
    }
}

/// Opens `relay`'s session WebSocket and waits until it is open.
pub(crate) async fn connect_relay(relay: &RelaySession) -> Result<PlatformStream, BridgeError> {
    let url = format!("{}{RELAY_SESSION_PATH}", relay.url.trim_end_matches('/'));
    let protocols = js_sys::Array::of2(
        &RELAY_SESSION_PROTOCOL.into(),
        &format!("{RELAY_SESSION_ID_PREFIX}{}", relay.session_id).into(),
    );
    let socket = WebSocket::new_with_str_sequence(&url, &protocols)
        .map_err(|_| refused("the relay URL is invalid"))?;
    socket.set_binary_type(BinaryType::Arraybuffer);

    let (incoming, received) = mpsc::unbounded();
    let (opened, open) = oneshot::channel::<bool>();
    let opened = Rc::new(RefCell::new(Some(opened)));
    let settle = {
        let opened = Rc::clone(&opened);
        move |open: bool| {
            if let Some(sender) = opened.borrow_mut().take() {
                let _ = sender.send(open);
            }
        }
    };

    let on_message = {
        let incoming = incoming.clone();
        Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
            if let Ok(buffer) = event.data().dyn_into::<js_sys::ArrayBuffer>() {
                let _ = incoming
                    .unbounded_send(Incoming::Data(js_sys::Uint8Array::new(&buffer).to_vec()));
            }
        })
    };
    let on_close = {
        let incoming = incoming.clone();
        let settle = settle.clone();
        Closure::<dyn FnMut(CloseEvent)>::new(move |_: CloseEvent| {
            settle(false);
            let _ = incoming.unbounded_send(Incoming::Closed);
        })
    };
    let on_error = {
        let settle = settle.clone();
        Closure::<dyn FnMut(Event)>::new(move |_: Event| {
            settle(false);
            let _ = incoming.unbounded_send(Incoming::Closed);
        })
    };
    let on_open = Closure::<dyn FnMut(Event)>::new(move |_: Event| settle(true));
    socket.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    socket.set_onclose(Some(on_close.as_ref().unchecked_ref()));
    socket.set_onerror(Some(on_error.as_ref().unchecked_ref()));
    socket.set_onopen(Some(on_open.as_ref().unchecked_ref()));
    let socket = SendWrapper::new(Rc::new(Socket {
        socket,
        _on_message: on_message,
        _on_close: on_close,
        _on_error: on_error,
        _on_open: on_open,
    }));

    if !open.await.unwrap_or(false) {
        return Err(refused("the relay refused the session"));
    }
    Ok(PlatformStream {
        read: PlatformReadHalf {
            received,
            pending: Vec::new(),
            offset: 0,
            _socket: SendWrapper::new(Rc::clone(&socket)),
        },
        write: PlatformWriteHalf {
            socket,
            draining: false,
        },
    })
}

fn refused(reason: &'static str) -> BridgeError {
    BridgeError::Io(io::Error::new(io::ErrorKind::ConnectionRefused, reason))
}

/// One relay session's byte stream.
pub(crate) struct PlatformStream {
    read: PlatformReadHalf,
    write: PlatformWriteHalf,
}

/// Bytes received from the relay, in order; the socket closing ends the stream.
pub(crate) struct PlatformReadHalf {
    received: mpsc::UnboundedReceiver<Incoming>,
    pending: Vec<u8>,
    offset: usize,
    _socket: SendWrapper<Rc<Socket>>,
}

/// Sends bytes to the relay, waiting while the browser's send buffer is full.
pub(crate) struct PlatformWriteHalf {
    socket: SendWrapper<Rc<Socket>>,
    draining: bool,
}

impl PlatformStream {
    pub(crate) fn into_split(self) -> (PlatformReadHalf, PlatformWriteHalf) {
        (self.read, self.write)
    }
}

impl AsyncRead for PlatformReadHalf {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        while this.offset == this.pending.len() {
            match Pin::new(&mut this.received).poll_next(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Some(Incoming::Data(data))) => {
                    this.pending = data;
                    this.offset = 0;
                }
                // The end of the stream: a read of no bytes.
                Poll::Ready(Some(Incoming::Closed) | None) => return Poll::Ready(Ok(())),
            }
        }
        let available = &this.pending[this.offset..];
        let count = available.len().min(buffer.remaining());
        buffer.put_slice(&available[..count]);
        this.offset += count;
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for PlatformWriteHalf {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        let this = self.get_mut();
        let socket = &this.socket.socket;
        if socket.ready_state() != WebSocket::OPEN {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "the relay session closed",
            )));
        }
        if socket.buffered_amount() > MAX_BUFFERED_BYTES {
            if !this.draining {
                this.draining = true;
                let waker = cx.waker().clone();
                async_platform::spawn(async move {
                    async_platform::sleep(DRAIN_POLL).await;
                    waker.wake();
                });
            }
            return Poll::Pending;
        }
        this.draining = false;
        socket
            .send_with_u8_array(buffer)
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "relay send failed"))?;
        Poll::Ready(Ok(buffer.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        let _ = self.socket.socket.close();
        Poll::Ready(Ok(()))
    }
}

impl AsyncRead for PlatformStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().read).poll_read(cx, buffer)
    }
}

impl AsyncWrite for PlatformStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        Pin::new(&mut self.get_mut().write).poll_write(cx, buffer)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.get_mut().write).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.get_mut().write).poll_shutdown(cx)
    }
}
