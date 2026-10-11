use bevy::prelude::Resource;
use render_model::{ActorDrawManifestEntry, ActorRigRoute, MAX_ACTOR_RENDER_INSTANCES};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Instant,
};
pub const MAX_ACTOR_PRESENTED_ACKNOWLEDGEMENTS: usize = 64;

pub const MAX_ACTOR_PRESENTATION_CALLBACKS: usize = 8;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActorDrawFrame {
    pub artwork_identity: [u8; 32],
    pub skin_revision: u64,
    pub geometry_revision: u64,
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
            && self.manifest.len() <= MAX_ACTOR_RENDER_INSTANCES
            && self.manifest.iter().enumerate().all(|(index, entry)| {
                entry.identity.is_exact()
                    && entry.reset_generation != 0
                    && matches!(
                        entry.route,
                        ActorRigRoute::Compiled | ActorRigRoute::StaticFallback
                    )
                    && entry.instance_index as usize == index
                    && entry.bone_count != 0
            })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActorPresentedFrameAck {
    pub artwork_identity: [u8; 32],
    pub skin_revision: u64,
    pub geometry_revision: u64,
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
                artwork_identity: self.artwork_identity,
                skin_revision: self.skin_revision,
                geometry_revision: self.geometry_revision,
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
            && self.artwork_identity == next.artwork_identity
            && self.skin_revision == next.skin_revision
            && self.geometry_revision == next.geometry_revision
            && self.draw_generation < next.draw_generation
            && self.gpu_completed_at <= next.gpu_completed_at
    }
}

#[derive(Debug)]
pub struct ActorPresentationToken {
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
    pub fn try_reserve_callback(&self, draw: ActorDrawFrame) -> Option<ActorPresentationToken> {
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

    pub fn publish_reserved(
        &self,
        token: ActorPresentationToken,
        present_returned_at: Instant,
        gpu_completed_at: Instant,
    ) -> bool {
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
            return false;
        }
        let acknowledgement = ActorPresentedFrameAck {
            artwork_identity: token.draw.artwork_identity,
            skin_revision: token.draw.skin_revision,
            geometry_revision: token.draw.geometry_revision,
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
        true
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
