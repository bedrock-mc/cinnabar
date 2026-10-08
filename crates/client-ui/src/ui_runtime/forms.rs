//! Session-bound form authority and bounded, single-enqueue responses.
pub mod engine_focus;
pub mod engine_input;
pub mod engine_scroll;
pub mod shape_probe;
#[cfg(test)]
pub mod store_tests;
pub mod values;
use super::UiRuntime;
use protocol::{
    CustomFormValue, FormKind, FormRequestEvent, ModalFormResponseSelection, NPC_DIALOGUE_FORM_ID,
    NpcRequestKind, Packet, ServerFormModel, custom_form_submit_response, modal_form_busy_response,
    modal_form_cancel_response, modal_form_submit_response, npc_request_packet,
};
use std::{collections::VecDeque, sync::Arc};
pub use values::{EditText, EngineFrame, FormEngineState, FormValue};

/// One displayed form; at most eight pending busy cancellations.
pub const MAX_RETAINED_SERVER_FORMS: usize = 8;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerFormIdentity {
    pub session: u64,
    pub form_id: u32,
    pub revision: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerFormEntry {
    pub identity: ServerFormIdentity,
    pub form_id: u32,
    pub kind: FormKind,
    pub title: Option<Arc<str>>,
    pub model: ServerFormModel,
    fifo_sequence: u64,
}
impl ServerFormEntry {
    pub const fn fifo_sequence(&self) -> u64 {
        self.fifo_sequence
    }
    /// Answerable buttons: menu buttons, or a modal's two.
    pub fn button_count(&self) -> usize {
        match &self.model {
            ServerFormModel::TextMenu(menu) => menu.buttons.len(),
            ServerFormModel::ElementMenu(menu) => menu.button_count(),
            ServerFormModel::Modal(_) => 2,
            ServerFormModel::NpcDialogue(npc) => npc.buttons.len(),
            _ => 0,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalFormAction {
    SubmitButton(u32),
    Dismiss,
    CustomElements,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormRespondError {
    StaleIdentity,
    PendingResponse,
    InvalidButton,
    UnsupportedControls,
    CustomElementsUnsupported,
}
#[derive(Debug, Clone, PartialEq)]
enum RetainedAnswer {
    ButtonIndex(u32),
    Modal(bool),
    Custom(Arc<[CustomFormValue]>),
    Npc {
        npc_runtime_id: u64,
        scene_name: Arc<str>,
        request: NpcRequestKind,
    },
    Dismissed,
    Busy,
}
#[derive(Debug, Clone, PartialEq)]
struct PendingFormResponse {
    identity: ServerFormIdentity,
    answer: RetainedAnswer,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormTransportError {
    /// The queue definitely did not accept the packet: safe to retry.
    Full,
    /// Closed is not a delivery acknowledgement and must not retry.
    Closed,
}
#[derive(Debug, Clone, Default)]
pub struct ServerFormStore {
    active: Option<ServerFormEntry>,
    pending: Option<PendingFormResponse>,
    busy: VecDeque<PendingFormResponse>,
    next_revision: u64,
    replaced_by_reissue: u64,
    dropped_over_capacity: u64,
    watched_dimension: Option<i32>,
    watched_epoch: Option<(u64, u64)>,
    focus: usize,
    scroll: usize,
    engine: FormEngineState,
    /// The settings screen already asked the server for its settings form.
    settings_requested: bool,
}
impl ServerFormStore {
    pub fn admit(
        &mut self,
        event: FormRequestEvent,
        fifo_sequence: u64,
        session: u64,
        other_ui: bool,
    ) {
        shape_probe::observe(&event);
        self.next_revision = self.next_revision.saturating_add(1);
        let identity = ServerFormIdentity {
            session,
            form_id: event.form_id,
            revision: self.next_revision,
        };
        // Same-ID reissue is new authority, even while an old answer is Full.
        // Never send an unsent old cancellation against the new revision.
        let replaces = self
            .active
            .as_ref()
            .is_some_and(|entry| entry.form_id == event.form_id)
            || self
                .pending
                .as_ref()
                .is_some_and(|response| response.identity.form_id == event.form_id);
        self.busy
            .retain(|response| response.identity.form_id != event.form_id);
        if replaces {
            self.active = None;
            self.pending = None;
            self.replaced_by_reissue = self.replaced_by_reissue.saturating_add(1);
        }
        // The server closing an NPC dialogue only takes it down.
        if matches!(&event.model, ServerFormModel::NpcDialogue(npc) if !npc.open) {
            return;
        }
        // Server settings answer the settings screen the player has open.
        let other_ui = other_ui && event.kind != FormKind::ServerSettings;
        if other_ui || self.active.is_some() || self.pending.is_some() {
            // An NPC dialogue has no busy answer; it is simply not shown.
            if event.form_id == NPC_DIALOGUE_FORM_ID {
                return;
            }
            if self.busy.len() < MAX_RETAINED_SERVER_FORMS {
                self.busy.push_back(PendingFormResponse {
                    identity,
                    answer: RetainedAnswer::Busy,
                });
            } else {
                self.dropped_over_capacity = self.dropped_over_capacity.saturating_add(1);
            }
            return;
        }
        self.focus = 0;
        self.scroll = 0;
        self.engine = FormEngineState::for_model(&event.model);
        self.active = Some(ServerFormEntry {
            identity,
            form_id: event.form_id,
            kind: event.kind,
            title: event.title,
            model: event.model,
            fifo_sequence,
        });
    }
    pub fn active(&self) -> Option<&ServerFormEntry> {
        self.active.as_ref()
    }
    pub fn entries(&self) -> impl Iterator<Item = &ServerFormEntry> {
        self.active.iter()
    }
    pub fn get(&self, form_id: u32) -> Option<&ServerFormEntry> {
        self.active
            .as_ref()
            .filter(|entry| entry.form_id == form_id)
    }
    /// A server settings form is up; it draws and takes input over the menu.
    pub fn settings_form_active(&self) -> bool {
        self.active
            .as_ref()
            .is_some_and(|entry| entry.kind == FormKind::ServerSettings)
    }
    pub fn owns_input(&self) -> bool {
        self.active.is_some() || self.pending.is_some()
    }
    pub const fn focus(&self) -> usize {
        self.focus
    }
    pub const fn scroll(&self) -> usize {
        self.scroll
    }
    pub fn move_focus(&mut self, delta: i32) {
        let Some(entry) = self.active.as_ref() else {
            return;
        };
        let count = entry.button_count() + 1;
        self.focus = (self.focus as i64 + i64::from(delta)).rem_euclid(count as i64) as usize;
    }
    pub fn scroll_rows(&mut self, delta: i32) {
        self.scroll = (self.scroll as i64 + i64::from(delta)).clamp(0, 1 << 20) as usize;
    }
    pub fn set_scroll(&mut self, offset: usize) {
        self.scroll = offset.min(1 << 20);
    }
    /// The JSON-UI path's live interaction state and custom-form values.
    pub const fn engine(&self) -> &FormEngineState {
        &self.engine
    }
    pub fn engine_mut(&mut self) -> &mut FormEngineState {
        &mut self.engine
    }
    pub fn reject_active_busy(&mut self) {
        if let Some(entry) = self.active.take() {
            if self.busy.len() < MAX_RETAINED_SERVER_FORMS {
                self.busy.push_back(PendingFormResponse {
                    identity: entry.identity,
                    answer: RetainedAnswer::Busy,
                });
            } else {
                self.dropped_over_capacity = self.dropped_over_capacity.saturating_add(1);
            }
        }
    }
    pub fn respond(
        &mut self,
        identity: ServerFormIdentity,
        action: LocalFormAction,
    ) -> Result<(), FormRespondError> {
        if self.pending.is_some() {
            return Err(FormRespondError::PendingResponse);
        }
        let entry = self
            .active
            .as_ref()
            .filter(|entry| entry.identity == identity)
            .ok_or(FormRespondError::StaleIdentity)?;
        let answer = match action {
            LocalFormAction::CustomElements => match &entry.model {
                ServerFormModel::Custom(_) => RetainedAnswer::Custom(self.engine.submission()),
                _ => return Err(FormRespondError::CustomElementsUnsupported),
            },
            LocalFormAction::Dismiss => match &entry.model {
                ServerFormModel::NpcDialogue(npc) => RetainedAnswer::Npc {
                    npc_runtime_id: npc.npc_runtime_id,
                    scene_name: Arc::clone(&npc.scene_name),
                    request: NpcRequestKind::ExecuteClosingCommands,
                },
                _ => RetainedAnswer::Dismissed,
            },
            LocalFormAction::SubmitButton(index) => match &entry.model {
                ServerFormModel::NpcDialogue(npc) => {
                    let button = npc
                        .buttons
                        .get(index as usize)
                        .ok_or(FormRespondError::InvalidButton)?;
                    RetainedAnswer::Npc {
                        npc_runtime_id: npc.npc_runtime_id,
                        scene_name: Arc::clone(&npc.scene_name),
                        request: NpcRequestKind::ExecuteAction(button.action_index),
                    }
                }
                ServerFormModel::TextMenu(_)
                | ServerFormModel::ElementMenu(_)
                | ServerFormModel::Modal(_) => {
                    if index as usize >= entry.button_count() {
                        return Err(FormRespondError::InvalidButton);
                    }
                    match entry.model {
                        // button1 answers true, button2 false.
                        ServerFormModel::Modal(_) => RetainedAnswer::Modal(index == 0),
                        _ => RetainedAnswer::ButtonIndex(index),
                    }
                }
                _ => return Err(FormRespondError::UnsupportedControls),
            },
        };
        self.pending = Some(PendingFormResponse { identity, answer });
        self.active = None;
        self.engine = FormEngineState::default();
        Ok(())
    }
    pub fn note_stream_dimension(&mut self, dimension: i32) {
        if self
            .watched_dimension
            .is_some_and(|previous| previous != dimension)
        {
            self.clear();
        }
        self.watched_dimension = Some(dimension);
    }
    pub fn synchronize_epoch(&mut self, session: u64, dimension_epoch: u64) {
        let identity = (session, dimension_epoch);
        if self.watched_epoch != Some(identity) {
            self.clear();
            self.watched_epoch = Some(identity);
        }
    }
    pub fn clear(&mut self) {
        self.active = None;
        self.pending = None;
        self.busy.clear();
        self.watched_dimension = None;
        self.watched_epoch = None;
        self.focus = 0;
        self.scroll = 0;
        self.engine = FormEngineState::default();
    }
    pub const fn replaced_by_reissue(&self) -> u64 {
        self.replaced_by_reissue
    }
    pub const fn dropped_over_capacity(&self) -> u64 {
        self.dropped_over_capacity
    }
    pub fn queued_busy_count(&self) -> usize {
        self.busy.len()
    }
}
fn pending_packet(pending: &PendingFormResponse) -> Packet {
    match &pending.answer {
        RetainedAnswer::ButtonIndex(index) => modal_form_submit_response(
            pending.identity.form_id,
            ModalFormResponseSelection::ButtonIndex(*index),
        ),
        RetainedAnswer::Modal(choice) => modal_form_submit_response(
            pending.identity.form_id,
            ModalFormResponseSelection::ModalButton(*choice),
        ),
        RetainedAnswer::Npc {
            npc_runtime_id,
            scene_name,
            request,
        } => npc_request_packet(*npc_runtime_id, scene_name, *request),
        RetainedAnswer::Custom(values) => {
            custom_form_submit_response(pending.identity.form_id, values)
        }
        RetainedAnswer::Dismissed => modal_form_cancel_response(pending.identity.form_id),
        RetainedAnswer::Busy => modal_form_busy_response(pending.identity.form_id),
    }
}
/// Drains every due response, the local answer first, and returns whether any was sent.
/// Accepted enqueue is consumed once, never retried on an uncertain later delivery failure.
pub fn flush_form_response(
    runtime: &mut UiRuntime,
    mut send: impl FnMut(Packet) -> Result<(), FormTransportError>,
) -> Result<bool, FormTransportError> {
    let session = runtime.session_id();
    let store = runtime.server_forms_mut();
    let mut sent = false;
    loop {
        let local = store.pending.is_some();
        let Some(pending) = store.pending.take().or_else(|| store.busy.pop_front()) else {
            return Ok(sent);
        };
        if pending.identity.session != session {
            continue;
        }
        match send(pending_packet(&pending)) {
            Ok(()) => sent = true,
            Err(FormTransportError::Full) => {
                if local {
                    store.pending = Some(pending);
                } else {
                    store.busy.push_front(pending);
                }
                return Err(FormTransportError::Full);
            }
            Err(FormTransportError::Closed) => {
                store.clear();
                return Err(FormTransportError::Closed);
            }
        }
    }
}

impl ServerFormStore {
    /// Requests server settings once per visit, retrying until the transport accepts it.
    pub fn flush_settings_request(&mut self, in_settings: bool, send: impl FnOnce() -> bool) {
        if !in_settings {
            self.settings_requested = false;
        } else if !self.settings_requested && send() {
            self.settings_requested = true;
        }
    }
}
