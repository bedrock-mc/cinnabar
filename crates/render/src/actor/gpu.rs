use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Instant,
};

use bevy::prelude::Resource;

use super::ActorDrawManifestEntry;

pub const MAX_ACTOR_PRESENTED_ACKNOWLEDGEMENTS: usize = 64;
pub(crate) const MAX_ACTOR_PRESENTATION_CALLBACKS: usize = 8;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActorDrawFrame {
    pub frame_generation: u64,
    pub draw_generation: u64,
    pub manifest: Arc<[ActorDrawManifestEntry]>,
}

impl ActorDrawFrame {
    #[must_use]
    pub fn is_exact(&self) -> bool {
        self.frame_generation != 0
            && self.draw_generation != 0
            && !self.manifest.is_empty()
            && self.manifest.len() <= super::MAX_RENDERED_PLAYERS
            && self.manifest.iter().enumerate().all(|(index, entry)| {
                entry.identity.is_exact()
                    && entry.completed_tick != 0
                    && entry.reset_generation != 0
                    && matches!(
                        entry.route,
                        super::ActorRigRoute::Compiled | super::ActorRigRoute::StaticFallback
                    )
                    && entry.instance_index as usize == index
                    && entry.bone_count != 0
            })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActorPresentedFrameAck {
    pub frame_sequence: u64,
    pub frame_generation: u64,
    pub draw_generation: u64,
    pub manifest: Arc<[ActorDrawManifestEntry]>,
    pub present_returned_at: Instant,
    pub gpu_completed_at: Instant,
}

impl ActorPresentedFrameAck {
    #[must_use]
    pub fn is_exact(&self) -> bool {
        self.frame_sequence != 0
            && ActorDrawFrame {
                frame_generation: self.frame_generation,
                draw_generation: self.draw_generation,
                manifest: Arc::clone(&self.manifest),
            }
            .is_exact()
            && self.present_returned_at <= self.gpu_completed_at
    }

    #[must_use]
    pub fn forms_consecutive_pair_with(&self, next: &Self) -> bool {
        self.is_exact()
            && next.is_exact()
            && self.frame_sequence.checked_add(1) == Some(next.frame_sequence)
            && self.draw_generation < next.draw_generation
            && self.gpu_completed_at <= next.gpu_completed_at
    }
}

#[derive(Debug)]
pub(crate) struct ActorPresentationToken {
    epoch: u64,
    frame_sequence: u64,
    draw: ActorDrawFrame,
}

#[derive(Default)]
struct ActorPresentationState {
    epoch: u64,
    next_frame_sequence: u64,
    in_flight_callbacks: usize,
    acknowledgements: VecDeque<ActorPresentedFrameAck>,
}

#[derive(Clone, Default, Resource)]
pub struct ActorPresentationGate {
    state: Arc<Mutex<ActorPresentationState>>,
}

impl ActorPresentationGate {
    pub(crate) fn try_reserve_callback(
        &self,
        draw: ActorDrawFrame,
    ) -> Option<ActorPresentationToken> {
        if !draw.is_exact() {
            return None;
        }
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if state.in_flight_callbacks == MAX_ACTOR_PRESENTATION_CALLBACKS
            || state.acknowledgements.len() == MAX_ACTOR_PRESENTED_ACKNOWLEDGEMENTS
        {
            return None;
        }
        let frame_sequence = state.next_frame_sequence.checked_add(1)?;
        state.next_frame_sequence = frame_sequence;
        state.in_flight_callbacks += 1;
        Some(ActorPresentationToken {
            epoch: state.epoch,
            frame_sequence,
            draw,
        })
    }

    pub(crate) fn publish_reserved(
        &self,
        token: ActorPresentationToken,
        present_returned_at: Instant,
        gpu_completed_at: Instant,
    ) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state.in_flight_callbacks = state.in_flight_callbacks.saturating_sub(1);
        if token.epoch != state.epoch
            || present_returned_at > gpu_completed_at
            || state.acknowledgements.len() == MAX_ACTOR_PRESENTED_ACKNOWLEDGEMENTS
            || state
                .acknowledgements
                .iter()
                .any(|acknowledgement| acknowledgement.frame_sequence == token.frame_sequence)
        {
            return;
        }
        let acknowledgement = ActorPresentedFrameAck {
            frame_sequence: token.frame_sequence,
            frame_generation: token.draw.frame_generation,
            draw_generation: token.draw.draw_generation,
            manifest: token.draw.manifest,
            present_returned_at,
            gpu_completed_at,
        };
        let insertion = state
            .acknowledgements
            .partition_point(|current| current.frame_sequence < acknowledgement.frame_sequence);
        state.acknowledgements.insert(insertion, acknowledgement);
    }

    #[must_use]
    pub fn drain(&self) -> Vec<ActorPresentedFrameAck> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state.acknowledgements.drain(..).collect()
    }

    pub fn clear(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state.epoch = state.epoch.saturating_add(1);
        state.acknowledgements.clear();
    }
}

#[derive(Default, Resource)]
pub(crate) struct ActorDrawTracker {
    pending: Mutex<Option<PendingActorDraw>>,
}

#[derive(Clone)]
struct PendingActorDraw {
    draw: ActorDrawFrame,
    drawn: bool,
}

impl ActorDrawTracker {
    pub(crate) fn clear(&self) {
        *self
            .pending
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = None;
    }

    pub(crate) fn begin(&self, draw: ActorDrawFrame) -> bool {
        if draw.frame_generation == 0 || draw.draw_generation == 0 || draw.manifest.is_empty() {
            self.clear();
            return false;
        }
        *self
            .pending
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) =
            Some(PendingActorDraw { draw, drawn: false });
        true
    }

    pub(crate) fn record_draw(&self) {
        if let Some(pending) = self
            .pending
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .as_mut()
        {
            pending.drawn = true;
        }
    }

    pub(crate) fn take_drawn(&self) -> Option<ActorDrawFrame> {
        self.pending
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .take()
            .filter(|pending| pending.drawn)
            .map(|pending| pending.draw)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::actor::{ActorRenderIdentity, ActorRigRoute, EntityRigId};

    fn draw(generation: u64) -> ActorDrawFrame {
        ActorDrawFrame {
            frame_generation: generation,
            draw_generation: generation,
            manifest: Arc::from([ActorDrawManifestEntry {
                identity: ActorRenderIdentity {
                    session_id: 1,
                    dimension: 0,
                    runtime_id: 2,
                    spawn_revision: 3,
                    ingress_sequence: 4,
                    source_tick: Some(5),
                    movement_revision: 4,
                    pose_generation: 6,
                },
                rig: EntityRigId(0),
                completed_tick: 7,
                reset_generation: 8,
                route: ActorRigRoute::Compiled,
                instance_index: 0,
                previous_bone_base: 0,
                current_bone_base: 0,
                bone_count: 1,
            }]),
        }
    }

    #[test]
    fn draw_tracker_requires_actual_draw_execution() {
        let tracker = ActorDrawTracker::default();
        assert!(tracker.begin(draw(1)));
        assert!(tracker.take_drawn().is_none());
        assert!(tracker.begin(draw(2)));
        tracker.record_draw();
        assert_eq!(tracker.take_drawn(), Some(draw(2)));
    }

    #[test]
    fn presentation_gate_orders_out_of_order_gpu_callbacks() {
        let gate = ActorPresentationGate::default();
        let first = gate.try_reserve_callback(draw(1)).unwrap();
        let second = gate.try_reserve_callback(draw(2)).unwrap();
        let now = Instant::now();
        gate.publish_reserved(second, now, now + Duration::from_millis(2));
        gate.publish_reserved(first, now, now + Duration::from_millis(1));

        let acknowledgements = gate.drain();
        assert_eq!(
            acknowledgements
                .iter()
                .map(|acknowledgement| acknowledgement.frame_sequence)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert!(acknowledgements[0].forms_consecutive_pair_with(&acknowledgements[1]));
    }

    #[test]
    fn presentation_gate_rejects_callbacks_from_before_lifecycle_clear() {
        let gate = ActorPresentationGate::default();
        let stale = gate.try_reserve_callback(draw(1)).unwrap();
        gate.clear();
        let now = Instant::now();
        gate.publish_reserved(stale, now, now);
        assert!(gate.drain().is_empty());
    }
}
