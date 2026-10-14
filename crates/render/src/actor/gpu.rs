use render_scene::ActorDrawFrame;
use std::sync::Mutex;

use bevy::prelude::Resource;

use render_model::ActorArtworkPageId;

#[path = "gpu/geometry.rs"]
mod geometry;
pub(crate) use geometry::SegmentedVertexBuffer;
pub use geometry::storage_binding_bytes;

#[derive(Default, Resource)]
pub(crate) struct ActorDrawTracker {
    pending: Mutex<PendingActorDraw>,
}

/// The frame awaiting draws; its span storage is reused across frames.
#[derive(Default)]
struct PendingActorDraw {
    /// `None` when no frame is pending.
    draw: Option<ActorDrawFrame>,
    view: u64,
    /// Distinct spans in strictly ascending `first` order, as `begin` validates.
    expected: Vec<ActorDrawSpan>,
    /// Whether each `expected` span has drawn in `view`.
    received: Vec<bool>,
    /// Expected spans not yet drawn.
    missing: usize,
}

impl PendingActorDraw {
    /// Locks the tracker state, recovering it after a panicking holder.
    fn lock(pending: &Mutex<Self>) -> std::sync::MutexGuard<'_, Self> {
        pending.lock().unwrap_or_else(|poison| poison.into_inner())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct ActorDrawSpan {
    pub material: u32,
    pub page: ActorArtworkPageId,
    pub first: u32,
    pub count: u32,
    /// Vertices of the geometry every instance of the span draws.
    pub vertex_count: u32,
}

impl ActorDrawTracker {
    pub(crate) fn clear(&self) {
        PendingActorDraw::lock(&self.pending).draw = None;
    }

    pub(crate) fn begin(&self, draw: ActorDrawFrame, view: u64, spans: &[ActorDrawSpan]) -> bool {
        let mut next = 0;
        let valid_spans = spans.iter().all(|span| {
            let valid = span.first == next
                && span.count != 0
                && usize::from(span.page) < render_model::MAX_ACTOR_TEXTURE_PAGES;
            next = span.first.saturating_add(span.count);
            valid
        });
        if draw.frame_generation == 0
            || draw.draw_generation == 0
            || draw.manifest.is_empty()
            || draw.manifest.len() > render_model::MAX_ACTOR_RENDER_INSTANCES
            || spans.len() > render_model::MAX_ACTOR_RENDER_INSTANCES
            || !valid_spans
            || next as usize != draw.manifest.len()
        {
            self.clear();
            return false;
        }
        let mut pending = PendingActorDraw::lock(&self.pending);
        pending.draw = Some(draw);
        pending.view = view;
        pending.expected.clear();
        pending.expected.extend_from_slice(spans);
        pending.received.clear();
        pending.received.resize(spans.len(), false);
        pending.missing = spans.len();
        true
    }

    pub(crate) fn record_draw(&self, view: u64, span: ActorDrawSpan) {
        let mut pending = PendingActorDraw::lock(&self.pending);
        if pending.draw.is_none() || pending.view != view {
            return;
        }
        // Contiguous validated spans have unique, ascending `first` offsets.
        let Ok(index) = pending
            .expected
            .binary_search_by_key(&span.first, |expected| expected.first)
        else {
            return;
        };
        if pending.expected[index] == span && !pending.received[index] {
            pending.received[index] = true;
            pending.missing -= 1;
        }
    }

    pub(crate) fn take_drawn(&self) -> Option<ActorDrawFrame> {
        let mut pending = PendingActorDraw::lock(&self.pending);
        let missing = pending.missing;
        pending.draw.take().filter(|_| missing == 0)
    }
}

#[cfg(test)]
mod tests {
    use render_model::ActorDrawManifestEntry;
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };

    use super::*;
    use render_model::EntityRigId;
    use render_model::{ActorRenderIdentity, ActorRigRoute};
    use render_scene::ActorPresentationGate;

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
                    pose_generation: render_api::PoseGeneration::new(6),
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
        use bevy::render::renderer::{RenderDevice, RenderQueue};
        use render_model::ActorRigVertex;
        use render_model::ActorRigVertexSegments;
        let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
        let (device, queue) = (RenderDevice::from(device), RenderQueue::new(queue));
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
