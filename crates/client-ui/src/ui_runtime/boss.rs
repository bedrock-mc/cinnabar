use std::collections::VecDeque;

use super::{FormTransportError, UiApplyOutcome, UiRuntime, UiRuntimeError, scoreboard_adapter};

const MAX_PENDING_RESPONSES: usize = 2 * ui::MAX_BOSS_BARS;

#[derive(Clone, Debug, Default)]
pub(super) struct Responses {
    pending: VecDeque<protocol::BossEvent>,
    coalesced_pairs: u64,
    dropped: u64,
}

impl Responses {
    fn push(&mut self, event: protocol::BossEvent) {
        if self.pending.len() == MAX_PENDING_RESPONSES {
            // An entirely unsent registration/removal pair has no final
            // server membership effect. Compact only when the queue is full.
            let pair = self.pending.iter().enumerate().find_map(|(first, queued)| {
                self.pending
                    .iter()
                    .enumerate()
                    .skip(first + 1)
                    .find_map(|(last, later)| {
                        (queued.target_entity_id == later.target_entity_id
                            && queued.action != later.action)
                            .then_some((first, last))
                    })
            });
            if let Some((first, last)) = pair {
                self.pending.remove(last);
                self.pending.remove(first);
                self.coalesced_pairs = self.coalesced_pairs.saturating_add(1);
            } else if let Some(index) = self.pending.iter().position(|queued| {
                queued.target_entity_id == event.target_entity_id && queued.action != event.action
            }) {
                self.pending.remove(index);
                self.coalesced_pairs = self.coalesced_pairs.saturating_add(1);
                return;
            } else {
                self.dropped = self.dropped.saturating_add(1);
                bevy::log::warn!(
                    dropped = self.dropped,
                    "boss subscription response capacity exceeded"
                );
                return;
            }
        }
        self.pending.push_back(event);
    }
}

impl UiRuntime {
    /// Local actor loss hides a bar without synthesizing a wire removal reply.
    pub fn retain_boss_actors(&mut self, has_actor: impl FnMut(i64) -> bool) {
        let retired = self.boss_bars.retain_actors(has_actor);
        if retired != 0 {
            bevy::log::info!(
                session = self.session_id(),
                retired,
                "boss bars retired after actor lifetime ended"
            );
        }
    }

    pub(super) fn apply_boss(
        &mut self,
        sequence: u64,
        event: protocol::BossEvent,
    ) -> Result<UiApplyOutcome, UiRuntimeError> {
        let tracked = self
            .boss_bars
            .stacked_iter()
            .any(|bar| bar.target_entity_id == event.target_entity_id);
        let changed = matches!(event.action, protocol::BossAction::Show) && !tracked
            || matches!(event.action, protocol::BossAction::Hide) && tracked;
        let lifecycle = changed.then(|| event.clone());
        let target_entity_id = event.target_entity_id;
        let action = event.action;
        let result = self
            .boss_bars
            .apply(sequence, scoreboard_adapter::boss(event))
            .map_err(UiRuntimeError::RetainedUiSequence)?;
        if result == ui::RetainedUiApply::Applied
            && let Some(event) = lifecycle
        {
            self.boss_responses.push(event);
        }
        if matches!(
            action,
            protocol::BossAction::Show | protocol::BossAction::Hide
        ) {
            bevy::log::info!(
                session = self.session_id(),
                sequence,
                target_entity_id,
                ?action,
                applied = result == ui::RetainedUiApply::Applied,
                "boss lifecycle received"
            );
        }
        Ok(scoreboard_adapter::apply_outcome(result))
    }

    /// Keeps unsent subscription replies in order until the transport accepts them.
    pub fn flush_boss_responses(
        &mut self,
        mut send: impl FnMut(protocol::Packet) -> Result<(), FormTransportError>,
    ) -> Result<usize, FormTransportError> {
        let mut sent = 0;
        while let Some(event) = self.boss_responses.pending.front() {
            let packet = protocol::boss_registration_response(event)
                .expect("queued boss subscription lifecycle event");
            match send(packet) {
                Ok(()) => {
                    bevy::log::info!(
                        session = self.session_id(),
                        target_entity_id = event.target_entity_id,
                        registered = event.action == protocol::BossAction::Show,
                        "boss subscription reply queued for server"
                    );
                    self.boss_responses.pending.pop_front();
                    sent += 1;
                }
                Err(FormTransportError::Full) => return Err(FormTransportError::Full),
                Err(FormTransportError::Closed) => {
                    self.boss_responses.pending.clear();
                    return Err(FormTransportError::Closed);
                }
            }
        }
        Ok(sent)
    }
}

#[cfg(test)]
mod tests;
