//! Server sound timing follows the actor's completed interpolation ticks.

use std::collections::VecDeque;

use super::{ActorSnapshot, ActorStore};
use crate::CommittedAudioEvent;
use actor_animation::ActorLifetimeId;

const MAX_SOUNDS_PER_ACTOR: usize = 9;
const FIRE_POSITION_DISTANCE_SQUARED: f32 = 0.25;

/// Pack event shared by rocket lifecycle and explicit level-sound routing.
pub const ROCKET_LAUNCH_EVENT: &str = "launch";

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
    /// Reserves the spawn's audio admission credit until the rocket's first actor tick.
    pub(super) fn queue_rocket_launch_audio(&mut self, runtime_id: u64) {
        let actor = &self.actors[&runtime_id];
        if actor.entity_fact.native_class
            != assets::facts::entity::NativeEntityClass::FireworksRocket
        {
            return;
        }
        let lifetime = self.lifetime_for(actor);
        self.synchronized_audio.pending.push_back(PendingSound {
            lifetime,
            countdown: -1,
            event: CommittedAudioEvent {
                sequence: actor.spawn_revision,
                dimension: self.dimension,
                // Publication assigns the live epoch; dimension reset discards this lifetime.
                dimension_epoch: 0,
                client_generated: true,
                actor_synchronization: None,
                event: protocol::AudioEvent::Level(protocol::LevelAudioEvent {
                    sound_event: ROCKET_LAUNCH_EVENT.into(),
                    position: actor.position,
                    data: -1,
                    // Launch resolves the individual pack event, without an entity route.
                    actor_identifier: "".into(),
                    is_baby: false,
                    is_global: false,
                    actor_unique_id: actor.unique_id,
                    fire_at_position: Some(actor.position),
                }),
            },
        });
    }

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
                tracing::warn!(
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
            .filter(|sound| sound.lifetime == lifetime && !sound.event.client_generated)
            .count()
            >= MAX_SOUNDS_PER_ACTOR
            && let Some(index) = queue
                .iter()
                .position(|sound| sound.lifetime == lifetime && !sound.event.client_generated)
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
            if pending.event.client_generated {
                let mut event = queue.pending.remove(index).expect("queued launch").event;
                if actor.flag(protocol_ids::ActorFlag::Silent.wire_value()) {
                    continue;
                }
                // Motion has just completed its tick; launch uses the position at tick entry.
                if let protocol::AudioEvent::Level(level) = &mut event.event {
                    level.position = actor.previous_pose.position;
                    level.fire_at_position = Some(actor.previous_pose.position);
                }
                event.actor_synchronization = Some(actor_animation::ActorLifetimeId {
                    session_id: self.session_id,
                    dimension: self.dimension,
                    runtime_id: actor.runtime_id,
                    spawn_revision: actor.spawn_revision,
                });
                queue.ready.push(event);
                continue;
            }
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
                event.actor_synchronization = Some(actor_animation::ActorLifetimeId {
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
