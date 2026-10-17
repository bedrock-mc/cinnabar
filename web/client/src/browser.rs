//! Browser-local control driver and verified asset download admission.
use crate::relay::{self, Host, Refresh};
use assets::carriers::startup::delivery::{DeliveryManifest, MemoryCarriers, startup_carriers};
use futures::channel::{mpsc, oneshot};
use js_sys::Function;
use std::{cell::Cell, rc::Rc, time::Duration};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_transport::{CloseReason, browser::BrowserTransport, session::AccountOperation};

mod timer;

/// Samples the browser's monotonic clock only at the platform edge.
fn now() -> Duration {
    Duration::from_secs_f64(timer::milliseconds().max(0.0) / 1000.0)
}

/// Supplies browser services to the one shared page driver.
struct BrowserHost {
    origin: String,
    observe: Function,
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
}

/// Owns bounded account commands; dropping the page cancels its controller.
#[wasm_bindgen]
pub struct Page {
    commands: mpsc::Sender<AccountOperation>,
    lifecycle: mpsc::Sender<()>,
    suspended: Rc<Cell<bool>>,
    refresh: Rc<Cell<Refresh>>,
    _cancel: oneshot::Sender<()>,
}

#[wasm_bindgen]
impl Page {
    /// Opens QUIC first and falls back through the existing transport selector.
    #[wasm_bindgen(constructor)]
    pub fn new(origin: &str, observe: Function) -> Result<Page, JsError> {
        let (commands, pending) = mpsc::channel(4);
        let (lifecycle, changed) = mpsc::channel(1);
        let suspended = Rc::new(Cell::new(false));
        let refresh = Rc::new(Cell::new(Refresh::None));
        let (cancel, cancelled) = oneshot::channel();
        spawn_local(relay::run(
            BrowserHost {
                origin: origin.to_owned(),
                observe,
            },
            pending,
            changed,
            Rc::clone(&suspended),
            Rc::clone(&refresh),
            cancelled,
        ));
        Ok(Page {
            commands,
            lifecycle,
            suspended,
            refresh,
            _cancel: cancel,
        })
    }
    /// Retries control with this page's retained account capability.
    pub fn reconnect(&mut self) {
        self.refresh.set(Refresh::Reconnect);
        let _ = self.lifecycle.try_send(());
    }
    /// Starts an explicitly fresh account session after resume was refused or expired.
    pub fn fresh_session(&mut self) {
        self.refresh.set(Refresh::Fresh);
        let _ = self.lifecycle.try_send(());
    }
    /// Coalesces sign-in intent in the shared controller after bounded command admission.
    pub fn sign_in(&mut self) -> Result<(), JsError> {
        self.command(AccountOperation::SignIn)
    }
    /// Signs out through the same relay control channel.
    pub fn sign_out(&mut self) -> Result<(), JsError> {
        self.command(AccountOperation::SignOut)
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
    /// Refuses additional UI actions while the controller's command slot is full.
    fn command(&mut self, operation: AccountOperation) -> Result<(), JsError> {
        self.commands
            .try_send(operation)
            .map_err(|_| JsError::new("relay busy or disconnected"))
    }

    /// Coalesces lifecycle changes even when the bounded wake slot is already occupied.
    fn set_suspended(&mut self, suspended: bool) {
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
    /// Supplies canonical filenames; JS never maintains a carrier list.
    pub fn entries(&self) -> String {
        serde_json::to_string(
            &startup_carriers()
                .map(|carrier| (carrier.name, carrier.output))
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
