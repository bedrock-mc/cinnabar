//! Browser-local control driver and verified asset download admission.
use assets::carriers::startup::delivery::{DeliveryManifest, MemoryCarriers, startup_carriers};
use futures::{
    FutureExt, StreamExt,
    channel::{mpsc, oneshot},
};
use js_sys::{Function, Promise};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_transport::{
    CloseReason,
    browser::BrowserTransport,
    session::{AccountOperation, ControlEvent, Session},
};

#[wasm_bindgen(module = "/../site/edges.mjs")]
extern "C" {
    fn milliseconds() -> f64;
    fn delay(ms: u32) -> Promise;
}
/// Samples the browser's monotonic clock only at the platform edge.
fn now() -> Duration {
    Duration::from_secs_f64(milliseconds().max(0.0) / 1000.0)
}
/// Supplies a browser deadline to the shared transport selector.
async fn deadline() {
    let _ = JsFuture::from(delay(10_000)).await;
}

/// Owns bounded account commands; dropping the page cancels its controller.
#[wasm_bindgen]
pub struct Page {
    commands: mpsc::Sender<AccountOperation>,
    lifecycle: mpsc::Sender<()>,
    suspended: Rc<Cell<bool>>,
    account: Rc<RefCell<crate::launcher::RelayAccount>>,
    _cancel: oneshot::Sender<()>,
}

#[wasm_bindgen]
impl Page {
    /// Opens QUIC first and falls back through the existing transport selector.
    #[wasm_bindgen(constructor)]
    pub fn new(origin: &str, observe: Function) -> Result<Page, JsError> {
        let origin = origin.to_owned();
        let (commands, mut pending) = mpsc::channel(4);
        let (lifecycle, mut changed) = mpsc::channel(1);
        let suspended = Rc::new(Cell::new(false));
        let paused = Rc::clone(&suspended);
        let (cancel, cancelled) = oneshot::channel();
        let account = Rc::new(RefCell::new(crate::launcher::RelayAccount::default()));
        let observed_account = Rc::clone(&account);
        spawn_local(async move {
            let mut session = Session::new();
            let mut cancelled = cancelled.fuse();
            let mut parked = false;
            loop {
                if paused.get() || parked {
                    futures::select_biased! {
                        _ = cancelled => break,
                        signal = changed.next().fuse() => {
                            if signal.is_none() { break; }
                            while changed.try_recv().is_ok() {}
                            parked = false;
                            continue;
                        }
                    }
                }
                let run = {
                    let drive = async {
                        let quic = BrowserTransport::new(&origin, true).ok();
                        let socket = BrowserTransport::new(&origin, false)?;
                        web_transport::select(&mut session, quic, || socket, now, deadline).await?;
                        if !session.resumed_account() {
                            let mut account = observed_account.borrow_mut();
                            account.fresh_session();
                            if let Some(operation) = session.pending_account() {
                                account.restore_operation(operation);
                            }
                        }
                        while let Ok(operation) = pending.try_recv() {
                            observed_account.borrow_mut().restore_operation(operation);
                            session.desire_account(operation);
                        }
                        session.flush_account().await?;
                        loop {
                            let command = {
                                let event = session.next_event(now).fuse();
                                let command = pending.next().fuse();
                                futures::pin_mut!(event, command);
                                futures::select! {
                                    command = command => command,
                                    event = event => {
                                        let event = event?;
                                        observed_account.borrow_mut().observe(&event).map_err(|_| CloseReason::Protocol)?;
                                        match event {
                                            ControlEvent::Account(value) => {
                                                let account: bridge::contract::AuthState = serde_json::from_value(value["state"].clone()).map_err(|_| CloseReason::Protocol)?;
                                                emit(&observe, serde_json::json!({"state": account, "account": value}));
                                            }
                                            ControlEvent::Connect(value) => emit(&observe, serde_json::json!({"connect": value})),
                                            ControlEvent::Error(_) => emit(&observe, serde_json::json!({"error": "relay refused request"})),
                                            ControlEvent::Keepalive => {}
                                        }
                                        continue;
                                    }
                                }
                            };
                            match command {
                                Some(mut operation) => {
                                    while let Ok(next) = pending.try_recv() {
                                        operation = next;
                                    }
                                    observed_account.borrow_mut().restore_operation(operation);
                                    session.desire_account(operation);
                                    session.flush_account().await?;
                                }
                                None => return Ok::<(), CloseReason>(()),
                            }
                        }
                    };
                    futures::pin_mut!(drive);
                    futures::select_biased! {
                        _ = cancelled => break,
                        signal = changed.next().fuse() => {
                            if signal.is_none() { break; }
                            while changed.try_recv().is_ok() {}
                            None
                        },
                        result = drive.fuse() => Some(result),
                    }
                };
                session.close(CloseReason::Transport);
                if run.is_some() {
                    parked = true;
                    observed_account.borrow_mut().disconnected();
                    emit(&observe, serde_json::json!({"state": "disconnected"}));
                }
            }
            session.close(CloseReason::Cancelled);
        });
        Ok(Page {
            commands,
            lifecycle,
            suspended,
            account,
            _cancel: cancel,
        })
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

/// Supplies the storage ceiling from the same owner that validates the document.
#[wasm_bindgen]
pub fn preference_limit() -> usize {
    crate::launcher::preferences::MAX_PREFERENCE_BYTES
}

/// Returns a canonical validated preference document for the browser storage edge.
#[wasm_bindgen]
pub fn validate_preferences(json: &str) -> Result<String, JsError> {
    crate::launcher::preferences::Preferences::decode(json)
        .and_then(|preferences| preferences.encode())
        .map_err(JsError::new)
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
