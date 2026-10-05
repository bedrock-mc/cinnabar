use std::{
    collections::{BTreeSet, VecDeque},
    sync::{Arc, Mutex},
    time::Instant,
};

use bevy::prelude::Resource;

use super::ActorDrawManifestEntry;

#[path = "gpu/geometry.rs"]
mod geometry;
pub(crate) use geometry::SegmentedVertexBuffer;

pub const MAX_ACTOR_PRESENTED_ACKNOWLEDGEMENTS: usize = 64;
pub(crate) const MAX_ACTOR_PRESENTATION_CALLBACKS: usize = 8;

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
            && self.manifest.len() <= super::MAX_ACTOR_RENDER_INSTANCES
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

#[derive(Default, Resource)]
pub(crate) struct ActorDrawTracker {
    pending: Mutex<Option<PendingActorDraw>>,
}

#[derive(Clone)]
struct PendingActorDraw {
    draw: ActorDrawFrame,
    view: u64,
    expected: BTreeSet<ActorDrawSpan>,
    received: BTreeSet<ActorDrawSpan>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct ActorDrawSpan {
    pub material: u32,
    pub page: u8,
    pub first: u32,
    pub count: u32,
    /// Vertices of the geometry every instance of the span draws.
    pub vertex_count: u32,
}

impl ActorDrawTracker {
    pub(crate) fn clear(&self) {
        *self
            .pending
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = None;
    }

    pub(crate) fn begin(&self, draw: ActorDrawFrame, view: u64, spans: &[ActorDrawSpan]) -> bool {
        let mut next = 0;
        let valid_spans = spans.iter().all(|span| {
            let valid = span.first == next
                && span.count != 0
                && usize::from(span.page) < super::MAX_ACTOR_TEXTURE_PAGES;
            next = span.first.saturating_add(span.count);
            valid
        });
        if draw.frame_generation == 0
            || draw.draw_generation == 0
            || draw.manifest.is_empty()
            || draw.manifest.len() > super::MAX_ACTOR_RENDER_INSTANCES
            || spans.len() > super::MAX_ACTOR_RENDER_INSTANCES
            || !valid_spans
            || next as usize != draw.manifest.len()
        {
            self.clear();
            return false;
        }
        *self
            .pending
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = Some(PendingActorDraw {
            draw,
            view,
            expected: spans.iter().copied().collect(),
            received: BTreeSet::new(),
        });
        true
    }

    pub(crate) fn record_draw(&self, view: u64, span: ActorDrawSpan) {
        if let Some(pending) = self
            .pending
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .as_mut()
            && pending.view == view
            && pending.expected.contains(&span)
        {
            pending.received.insert(span);
        }
    }

    pub(crate) fn take_drawn(&self) -> Option<ActorDrawFrame> {
        self.pending
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .take()
            .filter(|pending| pending.expected == pending.received)
            .map(|pending| pending.draw)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::actor::{ActorRenderIdentity, ActorRigRoute};
    use render_model::EntityRigId;

    fn draw(generation: u64) -> ActorDrawFrame {
        ActorDrawFrame {
            artwork_identity: [0; 32],
            skin_revision: 1,
            geometry_revision: 1,
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
                    layer: 0,
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
        let span = ActorDrawSpan {
            material: 0,
            page: 0,
            first: 0,
            count: 1,
            vertex_count: 3,
        };
        assert!(tracker.begin(draw(1), 9, &[span]));
        assert!(tracker.take_drawn().is_none());
        assert!(tracker.begin(draw(2), 9, &[span]));
        tracker.record_draw(9, span);
        assert_eq!(tracker.take_drawn(), Some(draw(2)));
    }

    #[test]
    fn page_ack_requires_every_span_in_one_intended_view() {
        let tracker = ActorDrawTracker::default();
        let mut frame = draw(3);
        let mut second = frame.manifest[0].clone();
        second.instance_index = 1;
        frame.manifest = Arc::from([frame.manifest[0].clone(), second]);
        let spans = [
            ActorDrawSpan {
                material: 0,
                page: 0,
                first: 0,
                count: 1,
                vertex_count: 3,
            },
            ActorDrawSpan {
                material: 0,
                page: 1,
                first: 1,
                count: 1,
                vertex_count: 3,
            },
        ];
        assert!(tracker.begin(frame.clone(), 8, &spans));
        tracker.record_draw(8, spans[0]);
        tracker.record_draw(8, spans[0]);
        tracker.record_draw(9, spans[1]);
        assert!(tracker.take_drawn().is_none());
        assert!(tracker.begin(frame.clone(), 8, &spans));
        for span in spans {
            tracker.record_draw(8, span);
        }
        assert_eq!(tracker.take_drawn(), Some(frame));
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

    /// A segment appended within an epoch writes into the same buffer; a new epoch replaces it.
    #[test]
    fn appended_segments_reuse_the_buffer_and_new_epochs_replace_it() {
        use crate::actor::ActorRigVertexSegments;
        use bevy::render::renderer::{RenderDevice, RenderQueue, WgpuWrapper};
        use render_model::ActorRigVertex;
        let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
        let (device, queue) = (
            RenderDevice::from(device),
            RenderQueue(Arc::new(WgpuWrapper::new(queue))),
        );
        let first = ActorRigVertexSegments::from_vertices(vec![ActorRigVertex::default(); 64]);
        let mut mirror = super::SegmentedVertexBuffer::default();
        mirror.sync(&device, &queue, "test", &first);
        let buffer = mirror.buffer().unwrap().id();
        // The headroom holds a small registration without reallocating.
        let grown = first.with_segment(Arc::from(vec![ActorRigVertex::default(); 8]));
        mirror.sync(&device, &queue, "test", &grown);
        assert_eq!(mirror.buffer().unwrap().id(), buffer);
        let relaid = ActorRigVertexSegments::from_vertices(vec![ActorRigVertex::default(); 64]);
        mirror.sync(&device, &queue, "test", &relaid);
        assert_ne!(mirror.buffer().unwrap().id(), buffer);
    }
}
