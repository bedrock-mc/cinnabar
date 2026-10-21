//! One page-owned game task over the relay's prepared play lease.
use super::{Page, emit, timer};
use futures::{
    channel::{mpsc, oneshot},
    future::{AbortHandle, Abortable},
};
use std::{cell::RefCell, future::Future, pin::Pin, rc::Rc, time::Duration};
use wasm_bindgen::prelude::{JsError, wasm_bindgen};
use wasm_bindgen_futures::spawn_local;

/// Cancels every local task spawned under this owner when it is replaced or dropped.
pub(super) struct Task(AbortHandle);
impl Drop for Task {
    fn drop(&mut self) {
        self.0.abort();
    }
}
impl Task {
    /// Runs browser-local work without requiring Send or a Tokio runtime.
    pub(super) fn spawn(work: impl Future<Output = ()> + 'static) -> Self {
        let (abort, registration) = AbortHandle::new_pair();
        spawn_local(async move {
            let _ = Abortable::new(work, registration).await;
        });
        Self(abort)
    }
}

/// Browser execution for the existing protocol pump and its finite startup jobs.
#[derive(Clone)]
struct Executor;
impl client_session::NetworkExecutor for Executor {
    type OutboundTask = Task;
    fn spawn_outbound(&self, work: impl Future<Output = ()> + Send + 'static) -> Task {
        Task::spawn(work)
    }
    fn timers(&self) -> bridge::tasks::Timers {
        timers()
    }
}
impl protocol::JoinPreparation for Executor {
    type Task<T: Send + 'static> = Pin<Box<dyn Future<Output = T>>>;
    fn submit<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Self::Task<T> {
        Box::pin(async move {
            timer::sleep(Duration::ZERO).await;
            work()
        })
    }
}

/// Supplies cancellation-owned browser deadlines to the shared protocol implementation.
fn timers() -> bridge::tasks::Timers {
    bridge::tasks::Timers(|duration| Box::pin(timer::sleep(duration)))
}

/// Retains the running game and its current session until page teardown.
#[wasm_bindgen]
pub struct Game {
    host: Rc<RefCell<cinnabar::browser::Host>>,
    play: Option<Task>,
    _jobs: Task,
    _audio_task: Option<Task>,
    audio: Option<client_audio::named_audio::BrowserAudio>,
}
impl Game {
    /// Adopts the game host returned after canvas startup.
    pub(super) fn new(
        host: cinnabar::browser::Host,
        audio: Option<client_audio::named_audio::BrowserAudio>,
        audio_task: Option<Task>,
    ) -> Self {
        let jobs = Task::spawn(async {
            let mut jobs = cinnabar::browser::Jobs::default();
            let mut delay = Duration::from_millis(16);
            loop {
                timer::sleep(delay).await;
                delay = if jobs.run_one(super::now()) {
                    Duration::ZERO
                } else {
                    Duration::from_millis(16)
                };
            }
        });
        Self {
            audio,
            _audio_task: audio_task,
            _jobs: jobs,
            host: Rc::new(RefCell::new(host)),
            play: None,
        }
    }
}
#[wasm_bindgen]
impl Game {
    /// Resumes the real game mixer during the canvas's user gesture.
    pub fn resume_audio(&self) -> Result<js_sys::Promise, wasm_bindgen::JsValue> {
        self.audio.as_ref().map_or_else(
            || Ok(js_sys::Promise::resolve(&wasm_bindgen::JsValue::UNDEFINED)),
            |audio| audio.context().resume(),
        )
    }

    /// Suspends the retained mixer while the page lives in the browser's navigation cache.
    pub fn suspend_audio(&self) -> Result<js_sys::Promise, wasm_bindgen::JsValue> {
        self.audio.as_ref().map_or_else(
            || Ok(js_sys::Promise::resolve(&wasm_bindgen::JsValue::UNDEFINED)),
            |audio| audio.context().suspend(),
        )
    }

    /// Starts a fresh manual join, cancelling the previous play task and its writer.
    pub fn join(&mut self, page: &Page, address: &str) -> Result<(), JsError> {
        let bridge::ConnectTarget::RakNet(address) =
            crate::launcher::addressed_target(address).map_err(JsError::new)?
        else {
            unreachable!("addressed_target accepts only RakNet servers");
        };
        page.account.borrow_mut().begin_join();
        self.disconnect();
        self.play = Some(Task::spawn(drive(
            Rc::clone(&self.host),
            page.joins.clone(),
            Rc::clone(&page.ready),
            page.observe.clone(),
            address.to_owned(),
        )));
        Ok(())
    }

    /// Cancels the play lease, both I/O tasks and game authority together.
    pub fn disconnect(&mut self) {
        self.play = None;
        self.host.borrow_mut().disconnect();
    }
}
impl Drop for Game {
    fn drop(&mut self) {
        self.disconnect();
    }
}

/// Opens one relay generation at a time and follows only validated, bounded server transfers.
async fn drive(
    host: Rc<RefCell<cinnabar::browser::Host>>,
    mut joins: mpsc::Sender<crate::play::Request<web_transport::browser::BrowserStream>>,
    ready: Rc<std::cell::Cell<bool>>,
    observe: js_sys::Function,
    mut address: String,
) {
    let mut transfers = client_session::MAX_TRANSFER_CHAIN_HOPS;
    loop {
        let started = super::now();
        // Control recovery belongs to the relay driver; play waits without replaying login bytes.
        while !ready.replace(false) {
            if super::now().saturating_sub(started) >= Duration::from_secs(60) {
                emit(
                    &observe,
                    serde_json::json!({"error":"Relay is not ready. Connect the relay, then join again."}),
                );
                return;
            }
            timer::sleep(Duration::from_millis(16)).await;
        }
        let (reply, response) = oneshot::channel();
        if joins.try_send(crate::play::Request { reply }).is_err() {
            emit(
                &observe,
                serde_json::json!({"error":"Relay is busy or disconnected"}),
            );
            return;
        }
        let lease = match response.await {
            Ok(Ok(lease)) => lease,
            _ => {
                emit(
                    &observe,
                    serde_json::json!({"error":"Could not open a relay play session"}),
                );
                return;
            }
        };
        let (io, mut guard) = lease.into_parts();
        let work = host.borrow_mut().connect(
            protocol::SessionIo {
                reader: io.reader,
                frames: io.frames,
                timers: timers(),
            },
            io.join,
            async move {
                io.packs.await.map_err(|_| {
                    protocol::ProtocolError::Bridge(
                        bridge::BridgeError::InvalidSessionMessage {
                            reason: "pack stream unavailable",
                        }
                        .into(),
                    )
                })
            },
            address,
            Executor,
            Executor,
        );
        let Ok(work) = work else {
            emit(
                &observe,
                serde_json::json!({"error":"Could not start the game session"}),
            );
            return;
        };
        emit(&observe, serde_json::json!({"state":"joining"}));
        let completed = guard.run(work, io.writer).await;
        drop(guard);
        if !completed {
            host.borrow_mut().disconnect();
            emit(&observe, serde_json::json!({"state":"disconnected"}));
            return;
        }
        let event = loop {
            if let Some(event) = host.borrow_mut().take_event() {
                break event;
            }
            timer::sleep(Duration::from_millis(16)).await;
        };
        match event {
            cinnabar::browser::Event::Transfer { host, port } if transfers > 0 => {
                transfers -= 1;
                address = cinnabar::browser::transfer_address(&host, port);
                emit(&observe, serde_json::json!({"state":"transferring"}));
            }
            cinnabar::browser::Event::Transfer { .. } => {
                emit(
                    &observe,
                    serde_json::json!({"error":"Server transfer limit reached"}),
                );
                return;
            }
            cinnabar::browser::Event::Failed(error) => {
                emit(&observe, serde_json::json!({"error":error}));
                return;
            }
            cinnabar::browser::Event::Disconnected => return,
        }
    }
}
