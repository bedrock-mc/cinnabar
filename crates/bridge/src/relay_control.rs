//! A browser's control socket to its web relay (`/v1/control`), which creates or resumes the
//! relay session, runs Microsoft sign-in and reports the join the relay prepares. The page keeps
//! the session ID in its session storage, so a reload resumes the same signed-in session.

use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::time::Duration;

use send_wrapper::SendWrapper;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{CloseEvent, Event, MessageEvent, WebSocket};

use crate::relay_events::{ControlEvent, ControlOp, parse_event};
use crate::{Account, ConnectProgress, RelaySession, RelaySessions};

const CONTROL_PATH: &str = "/v1/control";
const CONTROL_PROTOCOL: &str = "cinnabar.control.v1";
const SESSION_ID_PREFIX: &str = "sid.";
/// Session storage key of the relay session ID, which the page used before the client owned it.
const SESSION_KEY: &str = "cinnabar.relay-session";
/// Wait before reconnecting a closed control socket.
const RECONNECT_DELAY: Duration = Duration::from_secs(2);

/// The relay's control link, shared by the menu's account screens and the joins that open
/// relay sessions. Clones share one socket. Browsers run it on one thread; the [`SendWrapper`]
/// satisfies resources that must be `Send`.
#[derive(Clone)]
pub struct RelayControl(SendWrapper<Rc<Shared>>);

struct Shared {
    url: String,
    state: RefCell<State>,
    socket: RefCell<Option<ControlSocket>>,
}

#[derive(Default)]
struct State {
    open: bool,
    session_id: Option<String>,
    account: Option<Account>,
    connect: Option<ConnectProgress>,
    refusal: Option<String>,
    queued: Vec<ControlOp>,
}

/// One control socket with its event handlers; dropping it detaches them and closes it.
struct ControlSocket {
    socket: WebSocket,
    _on_open: Closure<dyn FnMut(Event)>,
    _on_message: Closure<dyn FnMut(MessageEvent)>,
    _on_close: Closure<dyn FnMut(CloseEvent)>,
}

impl Drop for ControlSocket {
    fn drop(&mut self) {
        self.socket.set_onopen(None);
        self.socket.set_onmessage(None);
        self.socket.set_onclose(None);
        let _ = self.socket.close();
    }
}

impl RelayControl {
    /// Connects to the relay at `url` (`wss://host`), resuming this tab's session when the
    /// relay still holds it, and keeps reconnecting after the socket closes.
    #[must_use]
    pub fn connect(url: impl Into<String>) -> Self {
        let shared = Rc::new(Shared {
            url: url.into().trim_end_matches('/').to_owned(),
            state: RefCell::new(State {
                session_id: stored_session(),
                ..State::default()
            }),
            socket: RefCell::new(None),
        });
        open(&shared);
        Self(SendWrapper::new(shared))
    }

    /// The sign-in state the relay last reported, once it has.
    #[must_use]
    pub fn account(&self) -> Option<Account> {
        self.0.state.borrow().account.clone()
    }

    /// The join the relay is preparing, until it hands the session over.
    #[must_use]
    pub fn connect_progress(&self) -> Option<ConnectProgress> {
        self.0.state.borrow().connect
    }

    /// Why the relay last refused a request or the socket, once.
    pub fn take_refusal(&self) -> Option<String> {
        self.0.state.borrow_mut().refusal.take()
    }

    /// Starts Microsoft sign-in, sent once the socket is open.
    pub fn sign_in(&self) {
        self.request(ControlOp::SignIn);
    }

    /// Signs out, or cancels a sign-in waiting for its code.
    pub fn sign_out(&self) {
        self.request(ControlOp::SignOut);
    }

    fn request(&self, op: ControlOp) {
        let sent = {
            let state = self.0.state.borrow();
            state.open
                && self
                    .0
                    .socket
                    .borrow()
                    .as_ref()
                    .is_some_and(|socket| socket.socket.send_with_str(op.frame()).is_ok())
        };
        if !sent {
            self.0.state.borrow_mut().queued.push(op);
        }
    }
}

impl RelaySessions for RelayControl {
    fn session(&self) -> Option<RelaySession> {
        self.0
            .state
            .borrow()
            .session_id
            .clone()
            .map(|session_id| RelaySession {
                url: self.0.url.clone(),
                session_id,
            })
    }
}

/// Opens a control socket for `shared`, resuming its session ID if it has one.
fn open(shared: &Rc<Shared>) {
    let protocols = js_sys::Array::of1(&CONTROL_PROTOCOL.into());
    if let Some(id) = &shared.state.borrow().session_id {
        protocols.push(&format!("{SESSION_ID_PREFIX}{id}").into());
    }
    let Ok(socket) =
        WebSocket::new_with_str_sequence(&format!("{}{CONTROL_PATH}", shared.url), &protocols)
    else {
        shared.state.borrow_mut().refusal = Some("the relay URL is invalid".to_owned());
        return;
    };
    let weak = Rc::downgrade(shared);
    let on_open = {
        let weak = weak.clone();
        Closure::<dyn FnMut(Event)>::new(move |_: Event| {
            if let Some(shared) = weak.upgrade() {
                opened(&shared);
            }
        })
    };
    let on_message = {
        let weak = weak.clone();
        Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
            if let (Some(shared), Some(text)) = (weak.upgrade(), event.data().as_string())
                && let Some(event) = parse_event(&text)
            {
                received(&shared, event);
            }
        })
    };
    let on_close = Closure::<dyn FnMut(CloseEvent)>::new(move |event: CloseEvent| {
        if let Some(shared) = weak.upgrade() {
            closed(&shared, &event);
        }
    });
    socket.set_onopen(Some(on_open.as_ref().unchecked_ref()));
    socket.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    socket.set_onclose(Some(on_close.as_ref().unchecked_ref()));
    *shared.socket.borrow_mut() = Some(ControlSocket {
        socket,
        _on_open: on_open,
        _on_message: on_message,
        _on_close: on_close,
    });
}

/// Sends the operations requested while the socket was closed.
fn opened(shared: &Shared) {
    let queued = {
        let mut state = shared.state.borrow_mut();
        state.open = true;
        std::mem::take(&mut state.queued)
    };
    if let Some(socket) = shared.socket.borrow().as_ref() {
        for op in queued {
            let _ = socket.socket.send_with_str(op.frame());
        }
    }
}

fn received(shared: &Shared, event: ControlEvent) {
    let mut state = shared.state.borrow_mut();
    match event {
        ControlEvent::Session(id) => {
            store_session(&id);
            state.session_id = Some(id);
        }
        ControlEvent::Account(account) => state.account = Some(account),
        ControlEvent::Connect(connect) => state.connect = connect,
        ControlEvent::Refused(reason) => state.refusal = Some(reason),
    }
}

/// Keeps the relay's reason for closing, then reconnects after [`RECONNECT_DELAY`]; the
/// relay may answer with a new session if it forgot the old one.
fn closed(shared: &Rc<Shared>, event: &CloseEvent) {
    {
        let mut state = shared.state.borrow_mut();
        state.open = false;
        state.connect = None;
        if !event.reason().is_empty() {
            state.refusal = Some(event.reason());
        }
    }
    let weak: Weak<Shared> = Rc::downgrade(shared);
    let reconnect = async move {
        async_platform::sleep(RECONNECT_DELAY).await;
        if let Some(shared) = weak.upgrade() {
            open(&shared);
        }
    };
    async_platform::spawn(reconnect);
}

fn session_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.session_storage().ok().flatten()
}

fn stored_session() -> Option<String> {
    session_storage()?.get_item(SESSION_KEY).ok().flatten()
}

/// Keeps `id` for this tab; without storage a reload starts a new session.
fn store_session(id: &str) {
    if let Some(storage) = session_storage() {
        let _ = storage.set_item(SESSION_KEY, id);
    }
}
