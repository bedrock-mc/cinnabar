use super::*;
use render_api::PoseGeneration;
use render_model::{ActorRenderIdentity, ActorRigGeometrySpan};

/// Publishes deterministic dense endpoints with distinct bytes for each actor and bone.
fn frame(actors: &[(u64, usize)]) -> ActorRigRenderFrame {
    let mut instances = Vec::new();
    let mut manifest = Vec::new();
    let mut previous = Vec::new();
    for &(id, count) in actors {
        let base = previous.len() as u32;
        instances.push(ActorGpuInstance {
            world_from_actor: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
            previous_bone_base: base,
            current_bone_base: base,
            partial_tick: 0.25,
            reset_generation: 1,
            ..Default::default()
        });
        manifest.push(ActorDrawManifestEntry {
            identity: ActorRenderIdentity {
                session_id: 1,
                dimension: 0,
                runtime_id: id,
                spawn_revision: 1,
                ingress_sequence: 1,
                source_tick: Some(1),
                movement_revision: 1,
                pose_generation: PoseGeneration::new(1),
                layer: 0,
            },
            rig: EntityRigId(0),
            completed_tick: 1,
            reset_generation: 1,
            route: ActorRigRoute::Compiled,
            instance_index: (instances.len() - 1) as u32,
            previous_bone_base: base,
            current_bone_base: base,
            bone_count: count as u32,
        });
        previous.extend((0..count).map(|bone| {
            [
                [1.0, 0.0, 0.0, id as f32],
                [0.0, 1.0, 0.0, bone as f32],
                [0.0, 0.0, 1.0, 0.0],
            ]
        }));
    }
    let current = previous
        .iter()
        .map(|matrix| {
            let mut matrix = *matrix;
            matrix[2][3] = 2.0;
            matrix
        })
        .collect::<Vec<_>>();
    ActorRigRenderFrame {
        frame_generation: 1,
        geometry_revision: 1,
        instances: instances.into(),
        manifest: manifest.into(),
        previous_bones: previous.into(),
        current_bones: current.into(),
        geometry_spans: Arc::from([ActorRigGeometrySpan {
            first_vertex: 0,
            vertex_count: 3,
        }]),
        maximum_vertex_count: 3,
        ..Default::default()
    }
}

/// Prepares renderer data from an immutable CPU publication.
fn prepare(cache: &mut BoneResidency, frame: &ActorRigRenderFrame) -> BoneFrame {
    cache.prepare(frame, Arc::clone(&frame.instances)).unwrap()
}

/// Applies exactly the planned GPU writes, then checks every endpoint and projected record.
fn apply_and_compare(
    cache: &BoneResidency,
    prepared: &BoneFrame,
    source: &ActorRigRenderFrame,
    gpu: &mut [Vec<u8>; 2],
) {
    cache.writes(prepared, |current, offset, bytes| {
        let buffer = &mut gpu[usize::from(current)];
        let offset = offset as usize;
        if buffer.len() < offset + bytes.len() {
            buffer.resize(offset + bytes.len(), 0);
        }
        buffer[offset..offset + bytes.len()].copy_from_slice(bytes);
    });
    for (index, entry) in source.manifest.iter().enumerate() {
        let projected = &prepared.manifest[index];
        let instance = &prepared.instances[index];
        assert_eq!(projected.previous_bone_base, instance.previous_bone_base);
        assert_eq!(projected.current_bone_base, instance.current_bone_base);
        for (buffer, bones, original, resident) in [
            (
                &gpu[0],
                &source.previous_bones,
                entry.previous_bone_base,
                projected.previous_bone_base,
            ),
            (
                &gpu[1],
                &source.current_bones,
                entry.current_bone_base,
                projected.current_bone_base,
            ),
        ] {
            let length = entry.bone_count as usize;
            let expected = bytemuck::cast_slice::<_, u8>(
                &bones[original as usize..original as usize + length],
            );
            let start = resident as usize * ACTOR_BONE_MATRIX_BYTES;
            assert_eq!(&buffer[start..start + expected.len()], expected);
        }
        let mut restored = *instance;
        restored.previous_bone_base = entry.previous_bone_base;
        restored.current_bone_base = entry.current_bone_base;
        assert_eq!(
            bytemuck::bytes_of(&restored),
            bytemuck::bytes_of(&source.instances[index])
        );
        let mut restored = projected.clone();
        restored.previous_bone_base = entry.previous_bone_base;
        restored.current_bone_base = entry.current_bone_base;
        assert_eq!(&restored, entry);
    }
    assert!(
        (cache.previous.len() + cache.current.len()) * ACTOR_BONE_MATRIX_BYTES
            <= render_model::MAX_ACTOR_BONE_ARENA_BYTES
    );
    assert!(cache.slots.len() <= MAX_ACTOR_RENDER_INSTANCES);
    let mut ranges = cache
        .slots
        .values()
        .map(|slot| slot.range.clone())
        .collect::<Vec<_>>();
    ranges.sort_by_key(|range| range.start);
    assert!(ranges.windows(2).all(|pair| pair[0].end <= pair[1].start));
}

#[test]
fn production_builder_reuses_endpoints_while_instance_scalars_advance() {
    use render_model::{
        ActorRigFrameBuilder, ActorRigGeometry, ActorRigRenderInput, ActorRigSubmission,
        RenderBoneTransform,
    };
    let geometry =
        ActorRigGeometry::synthetic_cuboid(EntityRigId(0), [0.0; 3], [1.0; 3], 30).unwrap();
    let mut builder = ActorRigFrameBuilder::new([geometry]).unwrap();
    let pose: Arc<[RenderBoneTransform]> = vec![
        RenderBoneTransform {
            rotation: [0.0, 0.0, 0.0, 1.0],
            translation_scale: [0.0, 0.0, 0.0, 1.0],
            axis_scale: render_model::UNIT_AXIS_SCALE,
        };
        30
    ]
    .into();
    let base = frame(&(1..=100).map(|id| (id, 30)).collect::<Vec<_>>());
    let mut submissions = base
        .manifest
        .iter()
        .map(|entry| ActorRigSubmission {
            material: Default::default(),
            culling_bounds: Default::default(),
            input: ActorRigRenderInput {
                identity: entry.identity,
                rig: EntityRigId(0),
                previous_bones: Arc::clone(&pose),
                current_bones: Arc::clone(&pose),
                completed_tick: 1,
                reset_generation: 1,
            },
            world_from_actor: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
            texture_layer: 0,
            route: ActorRigRoute::Compiled,
            tint: 0,
            overlay_rgba8: 0,
            uv_anim: render_model::IDENTITY_UV_ANIM,
            light: 0,
        })
        .collect::<Vec<_>>();
    let first = builder.build(0.0, None, submissions.clone());
    assert_eq!(first.instances.len(), 100);
    let mut cache = BoneResidency::default();
    let mut gpu = [Vec::new(), Vec::new()];
    let uploaded = prepare(&mut cache, &first);
    assert_eq!(
        uploaded.uploaded_bytes(),
        100 * 30 * ACTOR_BONE_MATRIX_BYTES * 2
    );
    assert_eq!((uploaded.previous.len(), uploaded.current.len()), (1, 1));
    apply_and_compare(&cache, &uploaded, &first, &mut gpu);
    for submission in &mut submissions {
        submission.world_from_actor[0][3] = 17.0;
        submission.tint = 0xffaabbcc;
    }
    let second = builder.build(0.75, None, submissions.clone());
    let uploaded = prepare(&mut cache, &second);
    assert_eq!(uploaded.uploaded_bytes(), 0);
    assert_eq!(uploaded.instances[0].partial_tick, 0.75);
    assert_eq!(uploaded.instances[0].world_from_actor[0][3], 17.0);
    apply_and_compare(&cache, &uploaded, &second, &mut gpu);
    Arc::make_mut(&mut submissions[0].input.current_bones)[0].translation_scale[0] = 3.0;
    let third = builder.build(1.0, None, submissions);
    assert_eq!(
        third.manifest[0].identity.pose_generation,
        second.manifest[0].identity.pose_generation
    );
    let uploaded = prepare(&mut cache, &third);
    assert_eq!(uploaded.uploaded_bytes(), 30 * ACTOR_BONE_MATRIX_BYTES);
    apply_and_compare(&cache, &uploaded, &third, &mut gpu);
}

#[test]
fn exact_bytes_not_generations_control_both_endpoint_writes() {
    let mut source = frame(&[(1, 2)]);
    let mut cache = BoneResidency::default();
    prepare(&mut cache, &source);
    source.frame_generation += 1;
    Arc::make_mut(&mut source.manifest)[0]
        .identity
        .pose_generation = PoseGeneration::new(9);
    source.previous_bones = source.previous_bones.to_vec().into();
    assert_eq!(prepare(&mut cache, &source).uploaded_bytes(), 0);
    Arc::make_mut(&mut source.previous_bones)[0][0][1] = -0.0;
    let change = prepare(&mut cache, &source);
    assert_eq!(change.uploaded_bytes(), 2 * ACTOR_BONE_MATRIX_BYTES);
    assert!(change.current.is_empty());
    Arc::make_mut(&mut source.current_bones)[1][1][3] = 7.0;
    let change = prepare(&mut cache, &source);
    assert_eq!(change.uploaded_bytes(), 2 * ACTOR_BONE_MATRIX_BYTES);
    assert!(change.previous.is_empty());
}

#[test]
fn reordering_and_skipped_publications_preserve_dense_sources_and_stable_slots() {
    let first = frame(&[(1, 3), (2, 5)]);
    let snapshot = first.clone();
    let mut cache = BoneResidency::default();
    let mut gpu = [Vec::new(), Vec::new()];
    let prepared = prepare(&mut cache, &first);
    apply_and_compare(&cache, &prepared, &first, &mut gpu);
    let mut latest = frame(&[(2, 5), (3, 2), (1, 3)]);
    latest.frame_generation = 20; // Intermediate publications were never extracted.
    Arc::make_mut(&mut latest.instances)[0].partial_tick = 0.8;
    let prepared = prepare(&mut cache, &latest);
    assert_eq!(prepared.instances[0].current_bone_base, 3);
    assert_eq!(prepared.instances[2].current_bone_base, 0);
    assert_eq!(prepared.uploaded_bytes(), 2 * 2 * ACTOR_BONE_MATRIX_BYTES);
    apply_and_compare(&cache, &prepared, &latest, &mut gpu);
    assert_eq!(first, snapshot);
    assert_eq!(latest.instances[0].current_bone_base, 0);
    assert_eq!(latest.manifest[2].instance_index, 2);
}

#[test]
fn layers_lifetimes_resets_geometry_and_resource_replacement_refresh_bones() {
    let first = frame(&[(1, 2)]);
    let bytes = 2 * 2 * ACTOR_BONE_MATRIX_BYTES;
    let mut cache = BoneResidency::default();
    prepare(&mut cache, &first);
    for change in 0..8 {
        let mut next = first.clone();
        let entry = &mut Arc::make_mut(&mut next.manifest)[0];
        match change {
            0 => entry.identity.session_id += 1,
            1 => entry.identity.dimension += 1,
            2 => entry.identity.spawn_revision += 1,
            3 => entry.identity.layer += 1,
            4 => entry.rig.0 += 1,
            5 => entry.reset_generation += 1,
            6 => next.geometry_revision += 1,
            _ => entry.route = ActorRigRoute::StaticFallback,
        }
        assert_eq!(prepare(&mut cache, &next).uploaded_bytes(), bytes);
        prepare(&mut cache, &first);
    }
    cache.clear();
    assert_eq!(prepare(&mut cache, &first).uploaded_bytes(), bytes);
    let mut replacement = BoneResidency::default();
    assert_eq!(prepare(&mut replacement, &first).uploaded_bytes(), bytes);
    let mut layers = frame(&[(1, 2), (1, 2)]);
    Arc::make_mut(&mut layers.manifest)[1].identity.layer = 1;
    let prepared = prepare(&mut cache, &layers);
    assert_ne!(
        prepared.instances[0].current_bone_base,
        prepared.instances[1].current_bone_base
    );
}

#[test]
fn resized_and_reused_slots_upload_even_when_old_bytes_are_identical() {
    let mut cache = BoneResidency::default();
    let mut gpu = [Vec::new(), Vec::new()];
    let first = frame(&[(1, 4), (2, 3)]);
    let prepared = prepare(&mut cache, &first);
    apply_and_compare(&cache, &prepared, &first, &mut gpu);
    let smaller = frame(&[(1, 2), (2, 3)]);
    let prepared = prepare(&mut cache, &smaller);
    assert_eq!(prepared.uploaded_bytes(), 2 * 2 * ACTOR_BONE_MATRIX_BYTES);
    assert_eq!(prepared.instances[1].current_bone_base, 4);
    apply_and_compare(&cache, &prepared, &smaller, &mut gpu);
    let mut replacement = smaller.clone();
    Arc::make_mut(&mut replacement.manifest)[0]
        .identity
        .runtime_id = 99;
    let prepared = prepare(&mut cache, &replacement);
    assert_eq!(prepared.uploaded_bytes(), 2 * 2 * ACTOR_BONE_MATRIX_BYTES);
    apply_and_compare(&cache, &prepared, &replacement, &mut gpu);
}

#[test]
fn fragmentation_compacts_without_losing_admitted_actors_or_increasing_capacity() {
    let size = MAX_RENDER_BONES_PER_ACTOR / 2;
    let mut actors = Vec::new();
    let mut remaining = MAX_ACTOR_POSE_BONES;
    while remaining != 0 {
        let count = remaining.min(size);
        actors.push((actors.len() as u64 + 1, count));
        remaining -= count;
    }
    assert!(actors.len() < MAX_ACTOR_RENDER_INSTANCES);
    let mut cache = BoneResidency::default();
    let mut gpu = [Vec::new(), Vec::new()];
    let full = frame(&actors);
    let prepared = prepare(&mut cache, &full);
    apply_and_compare(&cache, &prepared, &full, &mut gpu);
    let last = actors.last().unwrap().0;
    actors.retain(|(id, _)| id % 2 == 0 || *id == last);
    actors.push((last + 1, size * 2));
    let sparse = frame(&actors);
    let prepared = prepare(&mut cache, &sparse);
    assert!(prepared.compacted);
    assert_eq!(prepared.instances.len(), actors.len());
    assert_eq!(
        prepared.uploaded_bytes(),
        sparse.previous_bones.len() * ACTOR_BONE_MATRIX_BYTES * 2
    );
    apply_and_compare(&cache, &prepared, &sparse, &mut gpu);
    assert_eq!(prepare(&mut cache, &sparse).uploaded_bytes(), 0);
}

#[test]
fn invalid_or_empty_publication_discards_proof_without_partial_draws() {
    let original = frame(&[(1, 2), (2, 3)]);
    let mut cache = BoneResidency::default();
    for bad in 0..6 {
        prepare(&mut cache, &original);
        let mut next = original.clone();
        match bad {
            0 => Arc::make_mut(&mut next.manifest)[1].bone_count = u32::MAX,
            1 => Arc::make_mut(&mut next.manifest)[0].previous_bone_base = u32::MAX,
            2 => Arc::make_mut(&mut next.manifest)[1].identity.runtime_id = 1,
            3 => Arc::make_mut(&mut next.instances)[0].geometry_id = u32::MAX,
            4 => Arc::make_mut(&mut next.manifest)[0].route = ActorRigRoute::NoDraw,
            _ => next = ActorRigRenderFrame::default(),
        }
        assert!(cache.prepare(&next, Arc::clone(&next.instances)).is_none());
        assert!(cache.slots.is_empty());
        assert_eq!(
            prepare(&mut cache, &original).uploaded_bytes(),
            5 * 2 * ACTOR_BONE_MATRIX_BYTES
        );
    }
}

#[test]
fn projected_receipts_keep_old_encoded_offsets_until_completion() {
    use crate::actor::gpu::{ActorDrawSpan, ActorDrawTracker};
    use render_scene::{ActorDrawFrame, ActorPresentationGate};
    use std::time::Instant;
    let mut cache = BoneResidency::default();
    prepare(&mut cache, &frame(&[(1, 2), (2, 3)]));
    let source = frame(&[(2, 3), (1, 2)]);
    let prepared = prepare(&mut cache, &source);
    assert_ne!(
        prepared.manifest[0].previous_bone_base,
        source.manifest[0].previous_bone_base
    );
    let draw = ActorDrawFrame {
        artwork_identity: [0; 32],
        skin_revision: 1,
        geometry_revision: 1,
        frame_generation: 1,
        draw_generation: 1,
        manifest: Arc::clone(&prepared.manifest),
    };
    let tracker = ActorDrawTracker::default();
    let spans = [ActorDrawSpan {
        material: 0,
        page: 0,
        first: 0,
        count: 2,
        vertex_count: 3,
    }];
    let batch = tracker.begin(draw, 7, &spans).unwrap();
    assert!(!tracker.record_batch(&batch, 8, &spans));
    assert!(tracker.record_batch(&batch, 7, &spans));
    let drawn = tracker.take_drawn().unwrap();
    let gate = ActorPresentationGate::default();
    let token = gate.try_reserve_callback(drawn).unwrap();
    assert!(gate.drain().is_empty());
    let replacement = prepare(&mut cache, &frame(&[(2, 7), (1, 2)]));
    assert!(!Arc::ptr_eq(&replacement.manifest, &prepared.manifest));
    assert_eq!(prepared.manifest[0].bone_count, 3);
    assert_eq!(replacement.manifest[0].bone_count, 7);
    let now = Instant::now();
    assert!(gate.publish_reserved(token, now, now));
    let ack = gate.drain().pop().unwrap();
    assert_eq!(ack.manifest, prepared.manifest);
    assert!(ack.is_exact());
    let stale = gate
        .try_reserve_callback(ActorDrawFrame {
            artwork_identity: ack.artwork_identity,
            skin_revision: ack.skin_revision,
            geometry_revision: ack.geometry_revision,
            frame_generation: ack.frame_generation,
            draw_generation: ack.draw_generation,
            manifest: ack.manifest,
        })
        .unwrap();
    gate.clear();
    assert!(!gate.publish_reserved(stale, now, now));
}

/// Reads a single matrix from fake GPU bytes without imposing alignment on the byte buffer.
fn gpu_matrix(bytes: &[u8], index: usize) -> Matrix {
    let start = index * ACTOR_BONE_MATRIX_BYTES;
    bytemuck::pod_read_unaligned(&bytes[start..start + ACTOR_BONE_MATRIX_BYTES])
}

#[test]
fn interpolated_points_normals_and_motion_history_keep_dense_cpu_contract() {
    use crate::motion_blur::{actor_history::ActorHistory, history::CameraHistory};
    use bevy::math::{Mat4, UVec4, Vec3, Vec4};
    let camera = CameraHistory::new(
        Mat4::IDENTITY,
        Mat4::IDENTITY,
        UVec4::new(0, 0, 640, 320),
        0,
    );
    let mut history = ActorHistory::default();
    let mut reference = ActorHistory::default();
    let mut cache = BoneResidency::default();
    let mut gpu = [Vec::new(), Vec::new()];
    for (step, alpha) in [0.0, 0.25, 1.0].into_iter().enumerate() {
        let mut source = if step == 0 {
            frame(&[(1, 2), (2, 3)])
        } else {
            frame(&[(2, 3), (1, 2)])
        };
        for instance in Arc::make_mut(&mut source.instances) {
            instance.partial_tick = alpha;
        }
        if step == 2 {
            Arc::make_mut(&mut source.manifest)[0].reset_generation += 1;
        }
        let untouched = source.clone();
        let prepared = prepare(&mut cache, &source);
        apply_and_compare(&cache, &prepared, &source, &mut gpu);
        for (index, entry) in source.manifest.iter().enumerate() {
            for bone in 0..entry.bone_count as usize {
                let a = source.previous_bones[entry.previous_bone_base as usize + bone];
                let b = source.current_bones[entry.current_bone_base as usize + bone];
                let ga = gpu_matrix(
                    &gpu[0],
                    prepared.instances[index].previous_bone_base as usize + bone,
                );
                let gb = gpu_matrix(
                    &gpu[1],
                    prepared.instances[index].current_bone_base as usize + bone,
                );
                for w in [0.0, 1.0] {
                    let point = Vec4::new(0.2, -0.5, 1.0, w);
                    let transform = |matrix: Matrix| {
                        Vec3::from_array(matrix.map(|row| Vec4::from_array(row).dot(point)))
                    };
                    let expected = transform(a).lerp(transform(b), alpha);
                    let actual =
                        transform(ga).lerp(transform(gb), prepared.instances[index].partial_tick);
                    assert_eq!(actual, expected);
                }
            }
        }
        assert_eq!(
            history.advance(&source, camera, camera, true),
            reference.advance(&untouched, camera, camera, true)
        );
        assert_eq!(
            bytemuck::cast_slice::<_, u8>(&history.instances),
            bytemuck::cast_slice::<_, u8>(&reference.instances)
        );
        assert_eq!(history.bones, reference.bones);
        assert_eq!(source, untouched);
    }
}
