//! Deterministic helper responses for client-part lifecycle and drawing tests.

use super::*;

pub(super) struct FakeWorker {
    pub(super) response: Option<Transaction>,
    /// Ordered snapshot pieces delivered alongside successful callbacks.
    pub(super) presentation: VecDeque<mod_host::presentation::Chunk>,
    /// A failure reply that the next poll delivers before any response.
    pub(super) failure: Option<CallFailure>,
    /// Makes each dispatch fail its callback instead of completing it.
    pub(super) fail: bool,
    pub(super) transport_error: bool,
    pub(super) refuse_dispatch: bool,
    /// The confirmation channel blocking callbacks, and the channel used after a restart.
    pub(super) confirmation: Option<String>,
    pub(super) registry: Option<String>,
    pub(super) dispatched: Vec<Event>,
    /// Makes epoch callbacks publish the actor references they observe.
    pub(super) observe_epoch: bool,
    pub(super) contexts: Vec<mod_host::presentation::Context>,
    pub(super) references: Vec<mod_render::retained::EntityKey>,
    /// The modal size each dispatch carried.
    pub(super) sizes: Vec<Option<GuiSize>>,
    /// The revision of the session items each dispatch carried.
    pub(super) items: Vec<Option<u64>>,
    pub(super) owner: Principal,
}

impl Worker for FakeWorker {
    /// Leaves initialization pending until the test supplies its completion.
    fn spawn(
        _: &Path,
        bytes: Vec<u8>,
        owner: Principal,
        _: Capabilities,
        drawing: mod_host::presentation::Context,
        _: BTreeMap<String, Vec<u8>>,
    ) -> Result<Self> {
        ensure!(bytes != [255], "fixture launch failed");
        Ok(Self {
            response: None,
            presentation: VecDeque::new(),
            failure: None,
            fail: false,
            transport_error: false,
            refuse_dispatch: false,
            confirmation: None,
            registry: None,
            dispatched: Vec::new(),
            observe_epoch: false,
            contexts: vec![drawing],
            references: Vec::new(),
            sizes: Vec::new(),
            items: Vec::new(),
            owner,
        })
    }
    /// Delivers only explicitly completed transactions and failures.
    fn poll(&mut self) -> Option<Result<Reply>> {
        if self.transport_error {
            return Some(Err(anyhow::anyhow!("fixture reply decode failed")));
        }
        if let Some(failure) = self.failure.take() {
            return Some(Ok(match self.response.take() {
                Some(transaction) => Reply::Recovered {
                    failure,
                    transaction,
                    confirmation: self.confirmation.clone(),
                    references: self.references.clone(),
                },
                None => Reply::Failed {
                    failure,
                    references: self.references.clone(),
                },
            }));
        }
        self.response.take().map(|transaction| {
            Ok(match &self.confirmation {
                Some(confirmation) => Reply::NotReady {
                    transaction,
                    confirmation: confirmation.clone(),
                },
                None => Reply::Committed {
                    transaction,
                    fuel: 1,
                    references: self.references.clone(),
                    presentation: self.presentation.pop_front().map(Box::new),
                },
            })
        })
    }
    /// Records delivered events and completes them on the next poll, in the dispatch's epoch,
    /// or fails them when the test asks.
    fn dispatch(&mut self, request: Dispatch) -> Result<()> {
        ensure!(!self.refuse_dispatch, "fixture dispatch failed");
        let context = request.drawing.clone();
        self.contexts.push(context.clone());
        if self.observe_epoch && matches!(request.event, Event::Epoch) {
            self.references = context.candidates.clone();
            let mut output = mod_host::presentation::Output::default();
            output.drawing.entities = context
                .candidates
                .iter()
                .enumerate()
                .map(|(slot, &key)| {
                    (
                        mod_api::drawing::DrawingHandle {
                            owner: 1,
                            generation: 1,
                            slot: slot as u64 + 1,
                        },
                        key,
                    )
                })
                .collect();
            let bytes = serde_json::to_vec(&output)?;
            self.presentation.push_back(mod_host::presentation::Chunk {
                revision: 1,
                offset: 0,
                total: bytes.len() as u32,
                bytes,
            });
        }
        self.sizes.push(request.gui);
        self.items
            .push(request.items.as_ref().map(|items| items.revision));
        if let Some(confirmation) = &self.confirmation {
            if matches!(&request.event, Event::Message { channel, .. } if channel == confirmation) {
                self.confirmation = None;
            } else {
                self.response = Some(Transaction {
                    owner: self.owner.clone(),
                    epoch: request.epoch,
                    commands: Vec::new(),
                });
                return Ok(());
            }
        }
        if self.fail {
            self.confirmation.clone_from(&self.registry);
            self.failure = Some(CallFailure {
                bundle: self.owner.bundle.clone(),
                callback: "dispatch".into(),
                kind: FailureKind::Panic,
                reason: "wasm trap: wasm `unreachable` instruction executed".into(),
                fuel: Some(1),
            });
            self.dispatched.push(request.event);
            if self.confirmation.is_some() {
                self.response = Some(Transaction {
                    owner: self.owner.clone(),
                    epoch: request.epoch,
                    commands: Vec::new(),
                });
            }
            return Ok(());
        }
        self.dispatched.push(request.event);
        self.response = Some(Transaction {
            owner: self.owner.clone(),
            epoch: request.epoch,
            commands: Vec::new(),
        });
        Ok(())
    }
}
