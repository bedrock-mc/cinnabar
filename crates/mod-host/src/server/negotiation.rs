//! Holds callbacks at the host boundary until registry-confirmed initialization succeeds.

use crate::helper::Event;
use server_experience::{policy::*, runtime::Command};

/// A terminal failure: retaining another callback would exceed the reliable queue budget.
#[derive(Debug)]
pub(crate) struct Overflow;

impl std::fmt::Display for Overflow {
    /// Gives the helper and the client a diagnostic for the stopped part.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("registry negotiation callback queue overflow; client part stopped")
    }
}

impl std::error::Error for Overflow {}

pub(super) struct Negotiation {
    pub confirmation: String,
    pub request: Command,
    held: Vec<Event>,
    bytes: usize,
}

impl Negotiation {
    /// Starts an empty callback queue for one component instance's registry request.
    pub fn new(confirmation: String, request: Command) -> Self {
        Self {
            confirmation,
            request,
            held: Vec::new(),
            bytes: 0,
        }
    }

    /// Retains every event in order, replacing only observations of the same state.
    pub fn hold(&mut self, event: &Event) -> anyhow::Result<()> {
        let replaced = state_key(event).and_then(|key| {
            self.held
                .iter()
                .position(|pending| state_key(pending) == Some(key))
        });
        let previous = replaced.map_or(Ok(0), |index| {
            serde_json::to_vec(&self.held[index]).map(|bytes| bytes.len())
        })?;
        let bytes = self.bytes - previous + serde_json::to_vec(event)?.len();
        if bytes > MAX_QUEUE_BYTES || (replaced.is_none() && self.held.len() >= MAX_QUEUE_MESSAGES)
        {
            return Err(Overflow.into());
        }
        if let Some(index) = replaced {
            self.held[index] = event.clone();
        } else {
            self.held.push(event.clone());
        }
        self.bytes = bytes;
        Ok(())
    }

    /// Replays the latest epoch and other state before the ordered event backlog.
    pub fn into_callbacks(self) -> impl Iterator<Item = Event> {
        let (mut state, events): (Vec<_>, Vec<_>) = self
            .held
            .into_iter()
            .partition(|event| state_key(event).is_some());
        state.sort_by_key(|event| !matches!(event, Event::Epoch));
        state.into_iter().chain(events)
    }
}

/// Identifies coalescible state; every event variant must explicitly choose its delivery rule.
fn state_key(event: &Event) -> Option<(&'static str, &str)> {
    match event {
        Event::Epoch | Event::Resized { .. } | Event::Shift { .. } => Some((event.callback(), "")),
        Event::Text { control, .. } => Some((event.callback(), control)),
        Event::Scrolled { view, .. } => Some((event.callback(), view)),
        Event::Message { .. } | Event::Action { .. } | Event::SecondaryAction { .. } => None,
    }
}

#[cfg(test)]
mod tests;
