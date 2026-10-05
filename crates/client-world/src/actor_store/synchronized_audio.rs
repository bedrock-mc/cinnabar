//! Server sound timing follows the actor's completed interpolation ticks.

use std::collections::VecDeque;

use super::{ActorSnapshot, ActorStore};
use crate::{ActorLifetimeId, CommittedAudioEvent};

const MAX_SOUNDS_PER_ACTOR: usize = 9;
const FIRE_POSITION_DISTANCE_SQUARED: f32 = 0.25;

#[derive(Debug)]
struct PendingSound {
    lifetime: ActorLifetimeId,
    countdown: i64,
    event: CommittedAudioEvent,
}

#[derive(Default, Debug)]
pub(super) struct SynchronizedAudio {
    pending: VecDeque<PendingSound>,
    ready: Vec<CommittedAudioEvent>,
    skipped: u64,
}

impl SynchronizedAudio {
    pub(super) fn clear(&mut self) {
        self.pending.clear();
        self.ready.clear();
    }

    pub(super) fn remove_runtime(&mut self, runtime_id: u64) {
        self.pending
            .retain(|sound| sound.lifetime.runtime_id != runtime_id);
        self.ready.retain(|sound| {
            sound
                .actor_synchronization
                .is_none_or(|owner| owner.runtime_id != runtime_id)
        });
    }
}

impl ActorStore {
    pub(crate) fn queue_synchronized_audio(&mut self, event: CommittedAudioEvent) {
        let protocol::AudioEvent::Level(level) = &event.event else {
            return;
        };
        let actor = self
            .unique_to_runtime
            .get(&level.actor_unique_id)
            .and_then(|runtime| self.actors.get(runtime));
        let Some(actor) = actor else {
            let previous = self.synchronized_audio.skipped;
            self.synchronized_audio.skipped = previous.saturating_add(1);
            if previous == 0 || self.synchronized_audio.skipped / 64 > previous / 64 {
                eprintln!(
                    "skipped synchronized audio for unavailable actor (total {})",
                    self.synchronized_audio.skipped
                );
            }
            return;
        };
        let lifetime = self.lifetime_for(actor);
        let queue = &mut self.synchronized_audio.pending;
        if queue
            .iter()
            .filter(|sound| sound.lifetime == lifetime)
            .count()
            >= MAX_SOUNDS_PER_ACTOR
            && let Some(index) = queue.iter().position(|sound| sound.lifetime == lifetime)
        {
            queue.remove(index);
        }
        queue.push_back(PendingSound {
            lifetime,
            countdown: -1,
            event,
        });
    }

    pub(super) fn advance_synchronized_audio(&mut self) {
        let queue = &mut self.synchronized_audio;
        let mut index = 0;
        while index < queue.pending.len() {
            let pending = &mut queue.pending[index];
            let actor = self
                .actors
                .get(&pending.lifetime.runtime_id)
                .filter(|actor| actor.spawn_revision == pending.lifetime.spawn_revision);
            let Some(actor) = actor else {
                queue.pending.remove(index);
                continue;
            };
            let protocol::AudioEvent::Level(level) = &pending.event.event else {
                unreachable!()
            };
            if let Some(position) = level.fire_at_position
                && waits_for_interpolation(actor, position, pending.countdown)
            {
                pending.countdown = i64::from(actor.interpolation_ticks_remaining) + 1;
            }
            if pending.countdown < 1 {
                let mut event = queue.pending.remove(index).expect("queued sound").event;
                event.actor_synchronization = Some(crate::ActorLifetimeId {
                    session_id: self.session_id,
                    dimension: self.dimension,
                    runtime_id: actor.runtime_id,
                    spawn_revision: actor.spawn_revision,
                });
                queue.ready.push(event);
            } else {
                pending.countdown -= 1;
                index += 1;
            }
        }
    }

    pub(crate) fn synchronized_audio_count(&self) -> usize {
        self.synchronized_audio.pending.len() + self.synchronized_audio.ready.len()
    }

    pub(crate) fn take_synchronized_audio(&mut self) -> Vec<CommittedAudioEvent> {
        std::mem::take(&mut self.synchronized_audio.ready)
    }
}

fn waits_for_interpolation(actor: &ActorSnapshot, fire_position: [f32; 3], countdown: i64) -> bool {
    let distance_squared = |position: [f32; 3]| {
        position
            .into_iter()
            .zip(fire_position)
            .map(|(axis, fire)| (axis - fire).powi(2))
            .sum::<f32>()
    };
    countdown != 0
        && countdown < i64::from(actor.interpolation_ticks_remaining)
        && distance_squared(actor.received_pose.position) <= FIRE_POSITION_DISTANCE_SQUARED
        && distance_squared(actor.position) > FIRE_POSITION_DISTANCE_SQUARED
}

#[cfg(test)]
#[path = "synchronized_audio_tests.rs"]
mod tests;
