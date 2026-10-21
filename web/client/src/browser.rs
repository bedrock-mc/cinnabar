//! Browser-local control driver and verified asset download admission.
use crate::relay::{self, AccountCommand, Host, Intent, Refresh};
use assets::carriers::startup::delivery::{DeliveryManifest, MemoryCarriers, startup_carriers};
use futures::channel::{mpsc, oneshot};
use js_sys::Function;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};
use wasm_bindgen::prelude::{JsError, JsValue, wasm_bindgen};
use wasm_bindgen_futures::spawn_local;
use web_transport::{CloseReason, browser::BrowserTransport, session::AccountOperation};

mod audio;
mod game;
mod timer;
pub use audio::SoundSource;
pub use game::Game;

/// Samples the browser's monotonic clock only at the platform edge.
fn now() -> Duration {
    Duration::from_secs_f64(timer::milliseconds().max(0.0) / 1000.0)
}

/// Supplies browser services to the one shared page driver.
struct BrowserHost {
    origin: String,
    observe: Function,
    account: Rc<RefCell<crate::launcher::RelayAccount>>,
}
impl Host for BrowserHost {
    type Transport = BrowserTransport;
    fn now(&self) -> Duration {
        now()
    }
    fn jitter(&self) -> Duration {
        Duration::from_secs_f64(js_sys::Math::random() * 0.5)
    }
    fn sleep(&self, duration: Duration) -> impl std::future::Future<Output = ()> {
        timer::sleep(duration)
    }
    fn transports(&self) -> Result<(Option<BrowserTransport>, BrowserTransport), CloseReason> {
        Ok((
            BrowserTransport::new(&self.origin, true).ok(),
            BrowserTransport::new(&self.origin, false)?,
        ))
    }
    fn observe(&self, value: serde_json::Value) {
        emit(&self.observe, value);
    }
    fn account(&self) -> &RefCell<crate::launcher::RelayAccount> {
        &self.account
    }
}

/// Owns bounded account commands; dropping the page cancels its controller.
#[wasm_bindgen]
pub struct Page {
    ready: Rc<Cell<bool>>,
    account: Rc<RefCell<crate::launcher::RelayAccount>>,
    observe: Function,
    commands: mpsc::Sender<AccountCommand>,
    joins: mpsc::Sender<crate::play::Request<web_transport::browser::BrowserStream>>,
    lifecycle: mpsc::Sender<()>,
    suspended: Rc<Cell<bool>>,
    intent: Rc<Intent>,
    _cancel: oneshot::Sender<()>,
}

#[wasm_bindgen]
impl Page {
    /// Opens QUIC first and falls back through the existing transport selector.
    #[wasm_bindgen(constructor)]
    pub fn new(origin: &str, observe: Function) -> Result<Page, JsError> {
        let intent = Rc::new(Intent::default());
        let ready = Rc::clone(&intent.ready);
        let account = Rc::new(RefCell::new(crate::launcher::RelayAccount::default()));
        let (commands, pending) = mpsc::channel(4);
        let (joins, requests) = mpsc::channel(1);
        let (lifecycle, changed) = mpsc::channel(1);
        let suspended = Rc::new(Cell::new(false));
        let (cancel, cancelled) = oneshot::channel();
        spawn_local(relay::run(
            BrowserHost {
                origin: origin.to_owned(),
                observe: observe.clone(),
                account: Rc::clone(&account),
            },
            pending,
            requests,
            changed,
            Rc::clone(&suspended),
            Rc::clone(&intent),
            cancelled,
        ));
        Ok(Page {
            ready,
            account,
            observe,
            commands,
            joins,
            lifecycle,
            suspended,
            intent,
            _cancel: cancel,
        })
    }
    /// Retries control with this page's retained account capability.
    pub fn reconnect(&mut self) {
        self.intent.request(Refresh::Reconnect);
        let _ = self.lifecycle.try_send(());
    }
    /// Starts an explicitly fresh account session after resume was refused or expired.
    pub fn fresh_session(&mut self) {
        self.account.borrow_mut().fresh_session();
        self.intent.request(Refresh::Fresh);
        let _ = self.lifecycle.try_send(());
    }
    /// Coalesces sign-in intent in the shared controller after bounded command admission.
    pub fn sign_in(&mut self) -> Result<(), JsError> {
        self.account_intent(::launcher::menu::auth::AccountIntent::SignIn)
    }
    /// Signs out through the same relay control channel.
    pub fn sign_out(&mut self) -> Result<(), JsError> {
        self.account_intent(::launcher::menu::auth::AccountIntent::SignOut)
    }

    /// Cancels pending sign-in even if its device code has not arrived yet.
    pub fn cancel_sign_in(&mut self) -> Result<(), JsError> {
        self.account_intent(::launcher::menu::auth::AccountIntent::Cancel)
    }

    /// Retains resume capability while cached navigation closes the live relay transport.
    pub fn suspend(&mut self) {
        self.set_suspended(true);
    }

    /// Reopens relay control after restoration without freeing this page object.
    pub fn resume(&mut self) {
        self.set_suspended(false);
    }
}
impl Page {
    /// Requests ordered play I/O from the page's one account and reconnect driver.
    pub async fn open_play(
        &mut self,
    ) -> Result<crate::play::Lease<web_transport::browser::BrowserStream>, CloseReason> {
        let (reply, response) = oneshot::channel();
        self.joins
            .try_send(crate::play::Request { reply })
            .map_err(|_| CloseReason::Transport)?;
        response.await.map_err(|_| CloseReason::Cancelled)?
    }

    /// Admits a menu intent before updating presentation; full queues remain visible failures.
    pub fn account_intent(
        &mut self,
        intent: ::launcher::menu::auth::AccountIntent,
    ) -> Result<(), JsError> {
        let mut next = self.account.borrow().clone();
        next.request(intent);
        if let Some(operation) = next.take_operation() {
            self.command(operation)?;
        }
        *self.account.borrow_mut() = next;
        Ok(())
    }

    /// Supplies plain account data to the game host without putting JS handles in resources.
    pub fn menu_account(
        &self,
    ) -> (
        ::launcher::menu::auth::AuthState,
        ::launcher::menu::view::MenuProfile,
        Option<::launcher::menu::view::JoinStage>,
    ) {
        let account = self.account.borrow();
        (account.state(), account.profile(), account.join_stage())
    }

    /// Reports operation failures without changing a confirmed signed-in identity.
    pub fn menu_account_error(&self) -> Option<String> {
        self.account.borrow().operation_error().map(str::to_owned)
    }

    /// Refuses additional UI actions while the controller's command slot is full.
    fn command(&mut self, operation: AccountOperation) -> Result<(), JsError> {
        self.commands
            .try_send(self.intent.account(operation))
            .map_err(|_| JsError::new("relay busy or disconnected"))
    }

    /// Coalesces lifecycle changes even when the bounded wake slot is already occupied.
    fn set_suspended(&mut self, suspended: bool) {
        if suspended {
            self.ready.set(false);
        }
        self.suspended.set(suspended);
        let _ = self.lifecycle.try_send(());
    }
}
/// Emits bounded presentation data without exposing transport exception details.
fn emit(observe: &Function, value: serde_json::Value) {
    let _ = observe.call1(&JsValue::NULL, &JsValue::from_str(&value.to_string()));
}

/// Carrier downloads, verified in Rust before admission to startup decoding.
#[wasm_bindgen]
pub struct Carriers {
    source: MemoryCarriers,
}
#[wasm_bindgen]
impl Carriers {
    /// Checks the complete manifest before the page makes any carrier requests.
    #[wasm_bindgen(constructor)]
    pub fn new(manifest: &str) -> Result<Carriers, JsError> {
        if manifest.len() > assets::carriers::startup::delivery::MAX_DELIVERY_MANIFEST_BYTES {
            return Err(JsError::new("delivery manifest too large"));
        }
        let manifest: DeliveryManifest = serde_json::from_str(manifest)
            .map_err(|_| JsError::new("invalid delivery manifest"))?;
        MemoryCarriers::new(manifest)
            .map(|source| Self { source })
            .map_err(error)
    }
    /// Supplies canonical filenames and delivery requirements from the startup carrier set.
    pub fn entries(&self) -> String {
        serde_json::to_string(
            &self
                .source
                .entries()
                .map(|carrier| {
                    (
                        carrier.name,
                        carrier.output,
                        startup_carriers().any(|required| required.name == carrier.name),
                    )
                })
                .collect::<Vec<_>>(),
        )
        .expect("carrier strings")
    }
    /// Admits only bytes with the manifest's length and digest.
    pub fn insert(&mut self, name: &str, bytes: Vec<u8>) -> Result<(), JsError> {
        self.source.insert(name, bytes).map_err(error)
    }
    /// Runs all authoritative carrier and cross-carrier startup validators.
    pub fn validate(&self) -> Result<(), JsError> {
        self.source.decode_startup().map(|_| ()).map_err(error)
    }

    /// Transfers verified startup carriers to the game host and starts the existing canvas.
    pub fn boot(
        self,
        canvas: String,
        input: &crate::BrowserInput,
        identity: i64,
        audio: Option<SoundSource>,
    ) -> Result<Game, JsError> {
        console_error_panic_hook::set_once();
        let startup = self.into_startup().map_err(error)?;
        let identity = protocol::ClientRandomId::new(identity)
            .ok_or_else(|| JsError::new("invalid client identity"))?;
        let mut input = input.clone();
        let (bank, audio_task) = match audio {
            Some(audio) => {
                let (bank, task) = audio.start(startup.audio.clone());
                (Some(bank), Some(task))
            }
            None => (None, None),
        };
        let output = if bank.is_some() {
            client_audio::named_audio::AudioDevice::open_default_once()
        } else {
            client_audio::named_audio::AudioDevice::disabled()
        };
        let audio = output.browser_audio();
        let host = cinnabar::browser::run(
            startup,
            canvas,
            identity,
            Box::new(move || input.take_device()),
            bank,
            output,
        )
        .map_err(|error| JsError::new(&error.to_string()))?;
        Ok(Game::new(host, audio, audio_task))
    }
}
/// Presents a carrier validation failure with the canonical rebuild hint.
fn error(error: assets::carriers::startup::delivery::DeliveryError) -> JsError {
    JsError::new(&error.to_string())
}

/// Bounds the delivery index before JSON parsing.
#[wasm_bindgen]
pub fn manifest_limit() -> usize {
    assets::carriers::startup::delivery::MAX_DELIVERY_MANIFEST_BYTES
}

impl Carriers {
    /// Transfers decoded domain data to the client host without filesystem emulation.
    pub fn into_startup(
        self,
    ) -> Result<
        assets::carriers::startup::delivery::StartupCarriers,
        assets::carriers::startup::delivery::DeliveryError,
    > {
        self.source.decode_startup()
    }
}
