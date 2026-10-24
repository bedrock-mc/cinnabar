//! Holds callbacks at the host boundary until registry-confirmed initialization succeeds.

use crate::helper::Event;
use server_experience::{policy::*, runtime::Command, wire::is_current_epoch};

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
    held: Vec<Held>,
    bytes: usize,
}

struct Held {
    event: Event,
    epoch: u64,
    bytes: usize,
}

impl Held {
    /// Keeps state and input callbacks, but delivers messages only in their originating world.
    fn deliver_in(&self, epoch: u64) -> bool {
        !matches!(self.event, Event::Message { .. }) || is_current_epoch(self.epoch, epoch)
    }
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

    /// Drops messages from other worlds, then retains the event or replaces its latest state.
    pub fn hold(&mut self, event: &Event, epoch: u64) -> anyhow::Result<()> {
        self.held.retain(|pending| {
            if pending.deliver_in(epoch) {
                true
            } else {
                self.bytes -= pending.bytes;
                false
            }
        });
        let replaced = state_key(event).and_then(|key| {
            self.held
                .iter()
                .position(|pending| state_key(&pending.event) == Some(key))
        });
        let previous = replaced.map_or(0, |index| self.held[index].bytes);
        let pending = Held {
            event: event.clone(),
            epoch,
            bytes: serde_json::to_vec(event)?.len(),
        };
        let bytes = self.bytes - previous + pending.bytes;
        if bytes > MAX_QUEUE_BYTES || (replaced.is_none() && self.held.len() >= MAX_QUEUE_MESSAGES)
        {
            return Err(Overflow.into());
        }
        if let Some(index) = replaced {
            self.held[index] = pending;
        } else {
            self.held.push(pending);
        }
        self.bytes = bytes;
        Ok(())
    }

    /// Replays latest state before ordered events, excluding messages from other worlds.
    pub fn into_callbacks(self, epoch: u64) -> impl Iterator<Item = Event> {
        let (mut state, events): (Vec<_>, Vec<_>) = self
            .held
            .into_iter()
            .filter(|pending| pending.deliver_in(epoch))
            .map(|pending| pending.event)
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
