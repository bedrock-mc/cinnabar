use render_scene::ActorDrawFrame;
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

use bevy::prelude::{Res, Resource};

use render_model::ActorArtworkPageId;

#[path = "gpu/geometry.rs"]
mod geometry;
pub(crate) use geometry::SegmentedVertexBuffer;
pub use geometry::storage_binding_bytes;

/// Whether sRGB actor, item, particle and block-entity artwork also allows the `Rgba8Unorm`
/// view shader packs read Java's raw gamma bytes through. Set only while a camera draws a
/// loaded pack, since the extra format changes the allocation on some backends; artwork
/// owners recreate their textures when it changes.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ArtworkRawViews(pub(crate) bool);

impl ArtworkRawViews {
    /// The current setting; absent before any shader-pack renderer installs it.
    pub(crate) fn current(raw: Option<Res<Self>>) -> Self {
        raw.as_deref().copied().unwrap_or_default()
    }

    /// The view formats artwork textures are created with.
    pub(crate) fn view_formats(self) -> &'static [bevy::render::render_resource::TextureFormat] {
        if cfg!(feature = "shaderpacks") && self.0 {
            &[bevy::render::render_resource::TextureFormat::Rgba8Unorm]
        } else {
            &[]
        }
    }
}

/// Whether `texture` allows the raw view packs read: creating one raises no validation error.
#[cfg(all(test, feature = "shaderpacks"))]
pub(crate) fn permits_raw_view(device: &wgpu::Device, texture: &wgpu::Texture) -> bool {
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let _view = texture.create_view(&wgpu::TextureViewDescriptor {
        format: Some(wgpu::TextureFormat::Rgba8Unorm),
        ..Default::default()
    });
    bevy::tasks::block_on(scope.pop()).is_none()
}

/// Follows the main world's pack cameras during extraction, before any artwork owner prepares.
#[cfg(feature = "shaderpacks")]
pub(crate) fn extract_artwork_raw_views(
    cameras: bevy::render::Extract<
        bevy::prelude::Query<
            (),
            bevy::prelude::With<render_scene::shaderpack::ShaderPackRendering>,
        >,
    >,
    mut raw: bevy::prelude::ResMut<ArtworkRawViews>,
) {
    let wanted = ArtworkRawViews(!cameras.is_empty());
    if *raw != wanted {
        *raw = wanted;
    }
}

#[derive(Default, Resource)]
pub(crate) struct ActorDrawTracker {
    pending: Mutex<Option<PendingActorDraw>>,
    #[cfg(test)]
    publications: std::sync::atomic::AtomicUsize,
}

/// Immutable identity and expectations shared by one queued draw and its commands.
#[derive(Clone)]
pub(crate) struct ActorDrawBatch(Arc<QueuedActorDraw>);

struct QueuedActorDraw {
    draw: ActorDrawFrame,
    view: u64,
    expected: BTreeSet<ActorDrawSpan>,
}

struct PendingActorDraw {
    batch: ActorDrawBatch,
    received: BTreeSet<ActorDrawSpan>,
}

impl ActorDrawBatch {
    /// Returns this batch's immutable prepared identity only for its intended view.
    pub(crate) fn frame(&self, view: u64) -> Option<&ActorDrawFrame> {
        (self.0.view == view).then_some(&self.0.draw)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct ActorDrawSpan {
    pub material: u32,
    pub page: ActorArtworkPageId,
    pub first: u32,
    pub count: u32,
    /// Exact vertex count drawn for every instance in the span.
    pub vertex_count: u32,
}

impl ActorDrawTracker {
    /// Invalidates the pending draw, including every previously captured batch handle.
    pub(crate) fn clear(&self) {
        *self
            .pending
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = None;
    }

    /// Validates contiguous bounded spans and publishes a fresh immutable batch identity.
    pub(crate) fn begin(
        &self,
        draw: ActorDrawFrame,
        view: u64,
        spans: &[ActorDrawSpan],
    ) -> Option<ActorDrawBatch> {
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
            return None;
        }
        let batch = ActorDrawBatch(Arc::new(QueuedActorDraw {
            draw,
            view,
            expected: spans.iter().copied().collect(),
        }));
        *self
            .pending
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = Some(PendingActorDraw {
            batch: batch.clone(),
            received: BTreeSet::new(),
        });
        Some(batch)
    }

    /// Publishes only encoded spans for this exact still-pending batch under one lock.
    /// Wrong-view, empty, oversized or stale batches cannot credit another queued draw.
    /// Returns whether at least one previously missing expected span was received.
    pub(crate) fn record_batch(
        &self,
        batch: &ActorDrawBatch,
        view: u64,
        spans: &[ActorDrawSpan],
    ) -> bool {
        if batch.frame(view).is_none()
            || spans.is_empty()
            || spans.len() > render_model::MAX_ACTOR_RENDER_INSTANCES
        {
            return false;
        }
        #[cfg(test)]
        self.publications
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut state = self
            .pending
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let Some(pending) = state
            .as_mut()
            .filter(|pending| Arc::ptr_eq(&pending.batch.0, &batch.0))
        else {
            return false;
        };
        let mut changed = false;
        for span in spans {
            if batch.0.expected.contains(span) {
                changed |= pending.received.insert(*span);
            }
        }
        changed
    }

    /// Consumes the pending draw only when actual encoding covered every expected full span.
    pub(crate) fn take_drawn(&self) -> Option<ActorDrawFrame> {
        self.pending
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .take()
            .filter(|pending| pending.batch.0.expected == pending.received)
            .map(|pending| pending.batch.0.draw.clone())
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

    #[test]
    fn artwork_raw_view_formats_require_the_feature_and_camera_demand() {
        assert!(ArtworkRawViews::default().view_formats().is_empty());
        let formats = ArtworkRawViews(true).view_formats();
        if cfg!(feature = "shaderpacks") {
            assert_eq!(formats, [wgpu::TextureFormat::Rgba8Unorm]);
        } else {
            assert!(formats.is_empty());
        }
    }

    #[cfg(feature = "shaderpacks")]
    #[test]
    fn artwork_raw_views_follow_the_last_pack_camera() {
        use bevy::{ecs::system::RunSystemOnce, prelude::*, render::MainWorld};
        use render_scene::shaderpack::ShaderPackRendering;

        let mut main = MainWorld::default();
        let first = main.spawn_empty().id();
        let second = main.spawn_empty().id();
        let mut render = World::new();
        render.insert_resource(main);
        render.init_resource::<ArtworkRawViews>();
        for (camera, enabled, expected) in [
            (first, false, false),
            (first, true, true),
            (second, true, true),
            (first, false, true),
            (second, false, false),
        ] {
            let mut main = render.resource_mut::<MainWorld>();
            if enabled {
                main.entity_mut(camera).insert(ShaderPackRendering);
            } else {
                main.entity_mut(camera).remove::<ShaderPackRendering>();
            }
            render.run_system_once(extract_artwork_raw_views).unwrap();
            assert_eq!(render.resource::<ArtworkRawViews>().0, expected);
        }
    }

    /// An exact single-instance frame for receipt and GPU-completion regressions.
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
        let _ = tracker.begin(draw(1), 9, &[span]).unwrap();
        assert!(tracker.take_drawn().is_none());
        let batch = tracker.begin(draw(2), 9, &[span]).unwrap();
        assert!(tracker.record_batch(&batch, 9, &[span]));
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
        let batch = tracker.begin(frame.clone(), 8, &spans).unwrap();
        assert!(tracker.record_batch(&batch, 8, &[spans[0], spans[0]]));
        assert!(!tracker.record_batch(&batch, 9, &[spans[1]]));
        assert!(tracker.take_drawn().is_none());
        let batch = tracker.begin(frame.clone(), 8, &spans).unwrap();
        assert!(tracker.record_batch(&batch, 8, &spans));
        assert_eq!(tracker.take_drawn(), Some(frame));
    }

    /// Builds distinct contiguous spans and an exact manifest without needing a GPU.
    fn many_spans(count: usize) -> (ActorDrawFrame, Vec<ActorDrawSpan>) {
        let mut frame = draw(1);
        let entry = frame.manifest[0].clone();
        frame.manifest = (0..count)
            .map(|index| {
                let mut entry = entry.clone();
                entry.instance_index = index as u32;
                entry.identity.runtime_id += index as u64;
                entry
            })
            .collect();
        let spans = (0..count)
            .map(|index| ActorDrawSpan {
                material: (index % 2) as u32,
                page: 0,
                first: index as u32,
                count: 1,
                vertex_count: 3,
            })
            .collect();
        (frame, spans)
    }

    #[test]
    fn stale_and_foreign_batch_handles_cannot_credit_an_identical_new_draw() {
        let tracker = ActorDrawTracker::default();
        let foreign = ActorDrawTracker::default();
        let (frame, spans) = many_spans(3);
        let old = tracker.begin(frame.clone(), 8, &spans).unwrap();
        let elsewhere = foreign.begin(frame.clone(), 8, &spans).unwrap();
        let _current = tracker.begin(frame.clone(), 8, &spans).unwrap();
        assert!(!tracker.record_batch(&old, 8, &spans));
        assert!(!tracker.record_batch(&elsewhere, 8, &spans));
        assert!(tracker.take_drawn().is_none());

        let cleared = tracker.begin(frame.clone(), 8, &spans).unwrap();
        tracker.clear();
        let current = tracker.begin(frame.clone(), 8, &spans).unwrap();
        assert!(!tracker.record_batch(&cleared, 8, &spans));
        assert!(tracker.record_batch(&current, 8, &spans));
        assert_eq!(tracker.take_drawn(), Some(frame.clone()));
        assert!(
            !tracker.record_batch(&current, 8, &spans),
            "taking retires the batch"
        );

        let mut newer = frame.clone();
        newer.draw_generation += 1;
        let next = tracker.begin(newer.clone(), 8, &spans).unwrap();
        assert!(!tracker.record_batch(&current, 8, &spans));
        assert!(tracker.record_batch(&next, 8, &spans));
        assert_eq!(tracker.take_drawn(), Some(newer));
    }

    #[test]
    fn full_span_mismatches_and_omitted_draws_never_fill_missing_coverage() {
        let tracker = ActorDrawTracker::default();
        let (frame, spans) = many_spans(3);
        let batch = tracker.begin(frame.clone(), 8, &spans).unwrap();
        let expected = spans[1];
        for bad in [
            ActorDrawSpan {
                material: expected.material + 1,
                ..expected
            },
            ActorDrawSpan {
                page: expected.page + 1,
                ..expected
            },
            ActorDrawSpan {
                first: expected.first + 10,
                ..expected
            },
            ActorDrawSpan {
                count: expected.count + 1,
                ..expected
            },
            ActorDrawSpan {
                vertex_count: expected.vertex_count + 1,
                ..expected
            },
        ] {
            assert!(!tracker.record_batch(&batch, 8, &[bad]));
        }
        assert!(tracker.record_batch(&batch, 8, &[spans[0], spans[2]]));
        assert!(
            !tracker.record_batch(&batch, 8, &[spans[0], spans[2]]),
            "duplicates add no coverage"
        );
        assert!(
            tracker.take_drawn().is_none(),
            "the skipped middle draw remains missing"
        );
        let batch = tracker.begin(frame.clone(), 8, &spans).unwrap();
        assert!(tracker.record_batch(&batch, 8, &spans[..1]));
        assert!(tracker.record_batch(&batch, 8, &spans[1..]));
        assert_eq!(tracker.take_drawn(), Some(frame));
    }

    #[test]
    fn publication_lock_count_follows_nonempty_commands_instead_of_span_count() {
        use std::sync::atomic::Ordering;
        for count in [1, 32] {
            for partition in [1, 7, 32] {
                let tracker = ActorDrawTracker::default();
                let (frame, spans) = many_spans(count);
                let batch = tracker.begin(frame.clone(), 8, &spans).unwrap();
                assert!(!tracker.record_batch(&batch, 8, &[]));
                assert!(!tracker.record_batch(&batch, 9, &spans));
                assert_eq!(tracker.publications.load(Ordering::Relaxed), 0);
                for command in spans.chunks(partition) {
                    assert!(tracker.record_batch(&batch, 8, command));
                }
                assert_eq!(
                    tracker.publications.load(Ordering::Relaxed),
                    count.div_ceil(partition)
                );
                assert_eq!(tracker.take_drawn(), Some(frame));
            }
        }
    }

    #[test]
    fn invalid_begin_retires_old_batches_and_oversized_publication_fails_closed() {
        let tracker = ActorDrawTracker::default();
        let (frame, spans) = many_spans(1);
        let mut zero_generation = frame.clone();
        zero_generation.draw_generation = 0;
        let mut zero_frame = frame.clone();
        zero_frame.frame_generation = 0;
        let mut empty = frame.clone();
        empty.manifest = Arc::from([]);
        for (invalid, invalid_spans) in [
            (zero_generation, spans.clone()),
            (zero_frame, spans.clone()),
            (empty, Vec::new()),
            (
                frame.clone(),
                vec![ActorDrawSpan {
                    count: 0,
                    ..spans[0]
                }],
            ),
            (
                frame.clone(),
                vec![ActorDrawSpan {
                    first: 1,
                    ..spans[0]
                }],
            ),
        ] {
            let old = tracker.begin(frame.clone(), 8, &spans).unwrap();
            assert!(tracker.begin(invalid, 8, &invalid_spans).is_none());
            assert!(!tracker.record_batch(&old, 8, &spans));
            assert!(tracker.take_drawn().is_none());
        }
        let batch = tracker.begin(frame, 8, &spans).unwrap();
        let too_many = vec![spans[0]; render_model::MAX_ACTOR_RENDER_INSTANCES + 1];
        assert!(!tracker.record_batch(&batch, 8, &too_many));
        assert!(tracker.take_drawn().is_none());
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
