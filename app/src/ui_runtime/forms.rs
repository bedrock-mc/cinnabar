//! Bounded retention and the modal answer lifecycle for server forms.
//!
//! Admitted `UiEvent::Form` traffic lands here keyed by form id: a reissued id
//! replaces its retained dialog, the oldest dialog is evicted at capacity with
//! accounting, and a session or dimension reset clears everything. Answering a
//! retained form builds the protocol-2168 `ModalFormResponse` outbound packet
//! into one latest-wins pending slot that [`flush_form_response`] drains
//! through an injected transport.

use std::collections::VecDeque;
use std::sync::Arc;

use protocol::{
    FormKind, FormRequestEvent, ModalFormResponseSelection, Packet, modal_form_cancel_response,
    modal_form_submit_response,
};

use super::UiRuntime;

pub const MAX_RETAINED_SERVER_FORMS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerFormEntry {
    pub form_id: u32,
    pub kind: FormKind,
    pub title: Option<Arc<str>>,
    pub json: Arc<str>,
    fifo_sequence: u64,
}

impl ServerFormEntry {
    pub const fn fifo_sequence(&self) -> u64 {
        self.fifo_sequence
    }
}

/// A locally authored answer to one retained server form. Custom-form element
/// state has no capture surface yet; [`LocalFormAction::CustomElements`] names
/// that unwired family and fails closed instead of fabricating a payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalFormAction {
    /// Submit the zero-based button index of a menu form. Modal and custom
    /// forms answer with different payload shapes that have no capture
    /// surface yet, so this action fails closed against their kinds.
    SubmitButton(u32),
    /// Dismiss the form as a user close.
    Dismiss,
    /// Custom forms need per-element state capture that does not exist yet.
    CustomElements,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormRespondError {
    UnknownForm { form_id: u32 },
    ButtonAnswerUnsupportedForKind { form_id: u32, kind: FormKind },
    CustomElementsUnsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RetainedAnswer {
    ButtonIndex(u32),
    Dismissed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingFormResponse {
    form_id: u32,
    answer: RetainedAnswer,
}

#[derive(Debug, Clone, Default)]
pub struct ServerFormStore {
    entries: VecDeque<ServerFormEntry>,
    replaced_by_reissue: u64,
    dropped_over_capacity: u64,
    superseded_responses: u64,
    pending: Option<PendingFormResponse>,
    watched_dimension: Option<i32>,
}

impl ServerFormStore {
    pub fn admit(&mut self, event: FormRequestEvent, fifo_sequence: u64) {
        if let Some(existing) = self
            .entries
            .iter()
            .position(|entry| entry.form_id == event.form_id)
        {
            self.entries.remove(existing);
            self.replaced_by_reissue = self.replaced_by_reissue.saturating_add(1);
        }
        if self.entries.len() >= MAX_RETAINED_SERVER_FORMS {
            self.entries.pop_front();
            self.dropped_over_capacity = self.dropped_over_capacity.saturating_add(1);
        }
        self.entries.push_back(ServerFormEntry {
            form_id: event.form_id,
            kind: event.kind,
            title: event.title,
            json: event.json,
            fifo_sequence,
        });
    }

    pub fn get(&self, form_id: u32) -> Option<&ServerFormEntry> {
        self.entries.iter().find(|entry| entry.form_id == form_id)
    }

    pub fn entries(&self) -> impl Iterator<Item = &ServerFormEntry> + '_ {
        self.entries.iter()
    }

    pub fn respond(
        &mut self,
        form_id: u32,
        action: LocalFormAction,
    ) -> Result<(), FormRespondError> {
        let kind = self
            .get(form_id)
            .ok_or(FormRespondError::UnknownForm { form_id })?
            .kind;
        let answer = match action {
            LocalFormAction::CustomElements => {
                return Err(FormRespondError::CustomElementsUnsupported);
            }
            LocalFormAction::SubmitButton(index) => match kind {
                FormKind::Menu => RetainedAnswer::ButtonIndex(index),
                _ => {
                    return Err(FormRespondError::ButtonAnswerUnsupportedForKind { form_id, kind });
                }
            },
            LocalFormAction::Dismiss => RetainedAnswer::Dismissed,
        };
        self.entries.retain(|entry| entry.form_id != form_id);
        if self
            .pending
            .replace(PendingFormResponse { form_id, answer })
            .is_some()
        {
            self.superseded_responses = self.superseded_responses.saturating_add(1);
        }
        Ok(())
    }

    /// Clears every retained dialog and pending response when the stream moves
    /// to another dimension; the first observation only arms the watch.
    ///
    /// Bounded accepted window: the dimension is sampled once per frame from
    /// the stream's current value rather than per packet. A form committed
    /// into the same poll batch as a dimension switch may therefore be
    /// admitted under the old dimension and survive into the new one until
    /// the next observation clears it; correcting that requires cross-packet
    /// reordering this surface deliberately does not perform, mirroring the
    /// accepted camera-instruction identity window.
    pub fn note_stream_dimension(&mut self, dimension: i32) {
        match self.watched_dimension {
            Some(previous) if previous != dimension => self.clear(),
            _ => {}
        }
        self.watched_dimension = Some(dimension);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.pending = None;
        self.watched_dimension = None;
    }

    pub const fn replaced_by_reissue(&self) -> u64 {
        self.replaced_by_reissue
    }

    pub const fn dropped_over_capacity(&self) -> u64 {
        self.dropped_over_capacity
    }

    pub const fn superseded_responses(&self) -> u64 {
        self.superseded_responses
    }

    fn take_pending(&mut self) -> Option<PendingFormResponse> {
        self.pending.take()
    }

    fn restore_pending(&mut self, pending: PendingFormResponse) {
        self.pending = Some(pending);
    }
}

fn pending_packet(pending: PendingFormResponse) -> Packet {
    match pending.answer {
        RetainedAnswer::ButtonIndex(index) => modal_form_submit_response(
            pending.form_id,
            ModalFormResponseSelection::ButtonIndex(index),
        ),
        RetainedAnswer::Dismissed => modal_form_cancel_response(pending.form_id),
    }
}

/// Drains one pending form response through the injected transport. A failed
/// send restores the pending response so a later frame can retry it unchanged.
pub fn flush_form_response<E>(
    runtime: &mut UiRuntime,
    mut send: impl FnMut(Packet) -> Result<(), E>,
) -> Result<bool, E> {
    let Some(pending) = runtime.server_forms_mut().take_pending() else {
        return Ok(false);
    };
    match send(pending_packet(pending)) {
        Ok(()) => Ok(true),
        Err(error) => {
            runtime.server_forms_mut().restore_pending(pending);
            Err(error)
        }
    }
}
