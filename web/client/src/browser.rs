//! Browser-local control driver and verified asset download admission.
use assets::carriers::startup::delivery::{DeliveryManifest, MemoryCarriers, startup_carriers};
use futures::{
    FutureExt, StreamExt,
    channel::{mpsc, oneshot},
};
use js_sys::{Function, Promise};
use std::time::Duration;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_transport::{AccountOperation, BrowserTransport, CloseReason, ControlEvent, Session};

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
    _cancel: oneshot::Sender<()>,
}
#[wasm_bindgen]
impl Page {
    /// Opens QUIC first and falls back through the existing transport selector.
    #[wasm_bindgen(constructor)]
    pub fn new(origin: &str, observe: Function) -> Result<Page, JsError> {
        let quic = BrowserTransport::new(origin, true).ok();
        let socket =
            BrowserTransport::new(origin, false).map_err(|_| JsError::new("relay unavailable"))?;
        let (commands, mut pending) = mpsc::channel(4);
        let (cancel, cancelled) = oneshot::channel();
        spawn_local(async move {
            let mut session = Session::new();
            let run = {
                let drive = async {
                    web_transport::select(&mut session, quic, || socket, now, deadline).await?;
                    loop {
                        let command = {
                            let event = session.next_event(now).fuse();
                            let command = pending.next().fuse();
                            futures::pin_mut!(event, command);
                            futures::select! {
                                command = command => command,
                                event = event => {
                                    match event? {
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
                                session.desire_account(operation);
                                session.flush_account().await?;
                            }
                            None => return Ok::<(), CloseReason>(()),
                        }
                    }
                };
                futures::pin_mut!(drive);
                futures::select! { result = drive.fuse() => Some(result), _ = cancelled.fuse() => None }
            };
            session.close(CloseReason::Cancelled);
            if run.is_some_and(|result| result.is_err()) {
                emit(&observe, serde_json::json!({"state": "disconnected"}));
            }
        });
        Ok(Page {
            commands,
            _cancel: cancel,
        })
    }
    /// Coalesces sign-in intent in the shared controller after bounded command admission.
    pub fn sign_in(&mut self) -> Result<(), JsError> {
        self.command(AccountOperation::SignIn)
    }
    /// Signs out through the same relay control channel.
    pub fn sign_out(&mut self) -> Result<(), JsError> {
        self.command(AccountOperation::SignOut)
    }
}
impl Page {
    /// Refuses additional UI actions while the controller's command slot is full.
    fn command(&mut self, operation: AccountOperation) -> Result<(), JsError> {
        self.commands
            .try_send(operation)
            .map_err(|_| JsError::new("relay busy or disconnected"))
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
