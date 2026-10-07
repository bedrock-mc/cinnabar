use super::*;
use crate::actor::rig::{
    ActorDrawManifestEntry, ActorGpuInstance, ActorRenderIdentity, ActorRigFrameBuilder,
    ActorRigGeometry, ActorRigRejects, ActorRigRenderFrame, ActorRigRenderInput, ActorRigRoute,
    ActorRigSubmission, EntityRigId,
};
use render_model::UNIT_AXIS_SCALE;
use std::sync::Arc;

/// Creates a finite pose with rotation, translation and nonuniform scale.
fn bone(index: usize) -> RenderBoneTransform {
    RenderBoneTransform {
        rotation: [0.0, 0.25, 0.0, 0.75],
        translation_scale: [index as f32 * 0.01, 0.5, -0.25, 1.0],
        axis_scale: [0.75, 1.25, 1.0, 1.0],
    }
}

/// Reproduces the old per-actor temporary matrix collection for comparison.
fn reference_matrices(
    transforms: &[RenderBoneTransform],
    pivots: &[[f32; 3]],
) -> Option<Vec<[[f32; 4]; 3]>> {
    transforms
        .iter()
        .enumerate()
        .map(|(index, transform)| {
            affine_matrix(*transform, pivots.get(index).copied().unwrap_or([0.0; 3]))
        })
        .collect()
}

/// Creates a complete actor submission without any equipment or asset dependency.
fn submission(runtime_id: u64, bones: usize) -> ActorRigSubmission {
    ActorRigSubmission {
        material: Default::default(),
        culling_bounds: Default::default(),
        input: ActorRigRenderInput {
            identity: ActorRenderIdentity {
                session_id: 1,
                dimension: 0,
                runtime_id,
                spawn_revision: 1,
                ingress_sequence: runtime_id,
                source_tick: Some(1),
                movement_revision: 1,
                pose_generation: 1,
                layer: 0,
            },
            rig: EntityRigId(3),
            previous_bones: (0..bones).map(bone).collect(),
            current_bones: (1..=bones).map(bone).collect(),
            completed_tick: 7,
            reset_generation: 1,
        },
        world_from_actor: [
            [1.0, 0.0, 0.0, runtime_id as f32],
            [0.0, 1.0, 0.0, 64.0],
            [0.0, 0.0, 1.0, 0.0],
        ],
        texture_layer: 0,
        route: ActorRigRoute::Compiled,
        tint: 0,
        uv_anim: crate::IDENTITY_UV_ANIM,
        light: 0,
        overlay_rgba8: 0,
    }
}

#[test]
fn append_preserves_existing_matrices_and_reuses_the_final_arena_allocation() {
    let mut arena = Vec::with_capacity(100);
    arena.push([[5.0; 4]; 3]);
    let capacity = arena.capacity();
    let transforms: Vec<_> = (0..64).map(bone).collect();
    let pivots = [[0.25, 0.5, -0.25]; 64];
    let reference = reference_matrices(&transforms, &pivots).unwrap();
    assert!(append_pose_matrices(&mut arena, &transforms, &pivots));
    assert_eq!(arena.capacity(), capacity);
    assert_eq!(arena[0], [[5.0; 4]; 3]);
    assert_eq!(arena[1..], reference);
}

#[test]
fn draw_light_multiplier_keeps_finite_values_and_defaults_non_finite_inputs() {
    let geometry =
        ActorRigGeometry::synthetic_cuboid(EntityRigId(3), [0.0; 3], [1.0; 3], 1).unwrap();
    let mut builder = ActorRigFrameBuilder::new([geometry]).unwrap();
    assert_eq!(ActorGpuInstance::default().light_color_multiplier, 1.0);
    for value in [-0.25, 0.5, 1.8, f32::NAN, f32::INFINITY] {
        let mut input = submission(1, 1);
        input.material.light_color_multiplier = value;
        let frame = builder.build(0.5, None, [input]);
        assert_eq!(frame.instances.len(), 1);
        assert_eq!(
            frame.instances[0].light_color_multiplier,
            if value.is_finite() { value } else { 1.0 }
        );
    }
}

#[test]
fn invalid_late_bones_roll_back_the_entire_pose_without_touching_the_prefix() {
    for invalid in [
        RenderBoneTransform {
            rotation: [0.0; 4],
            ..bone(0)
        },
        RenderBoneTransform {
            translation_scale: [f32::NAN, 0.0, 0.0, 1.0],
            axis_scale: UNIT_AXIS_SCALE,
            ..bone(0)
        },
    ] {
        let mut arena = vec![[[5.0; 4]; 3]];
        let transforms = [bone(0), bone(1), invalid];
        assert!(reference_matrices(&transforms, &[]).is_none());
        assert!(!append_pose_matrices(&mut arena, &transforms, &[]));
        assert_eq!(arena, vec![[[5.0; 4]; 3]]);
    }
}

#[test]
fn complete_frames_match_reference_matrices_and_invalid_actors_leave_no_arena_holes() {
    let geometry =
        ActorRigGeometry::synthetic_cuboid(EntityRigId(3), [0.0; 3], [1.0, 2.0, 1.0], 4).unwrap();
    let pivots = Arc::clone(&geometry.bone_pivots);
    let mut builder = ActorRigFrameBuilder::new([geometry]).unwrap();
    let mut inputs: Vec<_> = (1..=5).map(|id| submission(id, 3)).collect();
    // Invalid quaternions are finite and reach the late matrix validation after an append starts.
    Arc::make_mut(&mut inputs[1].input.current_bones)[2].rotation = [0.0; 4];
    Arc::make_mut(&mut inputs[2].input.previous_bones)[2].rotation = [0.0; 4];
    inputs[3].input.reset_generation = u64::from(u32::MAX) + 1;
    let geometry_index = builder.catalog.indices[&EntityRigId(3)];
    let mut previous_bones = Vec::new();
    let mut current_bones = Vec::new();
    let mut instances = Vec::new();
    let mut manifest = Vec::new();
    for input in [&inputs[0], &inputs[4]] {
        let base = previous_bones.len() as u32;
        previous_bones.extend(reference_matrices(&input.input.previous_bones, &pivots).unwrap());
        current_bones.extend(reference_matrices(&input.input.current_bones, &pivots).unwrap());
        let instance_index = instances.len() as u32;
        instances.push(ActorGpuInstance {
            glint: input.material.glint.parameters(),
            world_from_actor: input.world_from_actor,
            previous_bone_base: base,
            current_bone_base: base,
            geometry_id: geometry_index,
            texture_layer: input.texture_layer,
            partial_tick: 0.5,
            reset_generation: 1,
            tint: input.tint,
            uv_anim: input.uv_anim,
            light: input.light,
            overlay_rgba8: input.overlay_rgba8,
            multitexture_layers: [u32::MAX; 2],
            material: input.material.kind as u32,
            dissolve_multiplier: input.material.dissolve_multiplier,
            light_color_multiplier: input.material.light_color_multiplier,
        });
        manifest.push(ActorDrawManifestEntry {
            identity: input.input.identity,
            rig: input.input.rig,
            completed_tick: input.input.completed_tick,
            reset_generation: 1,
            route: input.route,
            instance_index,
            previous_bone_base: base,
            current_bone_base: base,
            bone_count: 3,
        });
    }
    let expected = ActorRigRenderFrame {
        frame_generation: 1,
        geometry_revision: builder.catalog.revision,
        instances: instances.into(),
        previous_bones: previous_bones.into(),
        current_bones: current_bones.into(),
        geometry_vertices: builder.catalog.vertices.clone(),
        geometry_spans: Arc::clone(&builder.catalog.published_spans),
        manifest: manifest.into(),
        maximum_vertex_count: builder.catalog.published_spans[geometry_index as usize].vertex_count,
        rejects: ActorRigRejects {
            non_finite_pose: 2,
            invalid_identity: 1,
            ..ActorRigRejects::default()
        },
    };
    assert_eq!(builder.build(0.5, None, inputs), expected);
}

#[test]
fn replacing_geometry_rebinds_a_cached_pose_to_its_new_pivots() {
    let id = EntityRigId(3);
    let geometry = ActorRigGeometry::synthetic_cuboid(id, [0.0; 3], [1.0; 3], 1).unwrap();
    let mut builder = ActorRigFrameBuilder::new([geometry.clone()]).unwrap();
    let input = submission(1, 1);
    let before = builder.build(0.5, None, [input.clone()]);
    let mut replacement = geometry;
    replacement.bone_pivots = Arc::from([[0.25, 1.5, -0.5]]);
    builder.insert_geometry(replacement.clone()).unwrap();
    let after = builder.build(0.5, None, [input.clone()]);
    assert_ne!(before.current_bones, after.current_bones);
    assert_eq!(
        after.current_bones.as_ref(),
        reference_matrices(&input.input.current_bones, &replacement.bone_pivots).unwrap()
    );
    assert_eq!(
        after.previous_bones.as_ref(),
        reference_matrices(&input.input.previous_bones, &replacement.bone_pivots).unwrap()
    );
}

#[test]
fn shadow_only_instances_follow_the_exact_main_manifest_even_across_layers() {
    let geometry =
        ActorRigGeometry::synthetic_cuboid(EntityRigId(3), [0.0; 3], [1.0; 3], 1).unwrap();
    let mut builder = ActorRigFrameBuilder::new([geometry]).unwrap();
    let mut shadow = submission(1, 1);
    shadow.route = ActorRigRoute::ShadowOnly;
    let mut equipment = submission(2, 1);
    equipment.input.identity.layer = 1;
    let frame = builder.build(0.5, None, [shadow, equipment, submission(3, 1)]);
    assert_eq!(frame.instances.len(), 3);
    assert!(
        frame.manifest[..2]
            .iter()
            .all(|entry| entry.route == ActorRigRoute::Compiled)
    );
    assert_eq!(frame.manifest[2].route, ActorRigRoute::ShadowOnly);
    assert_eq!(frame.manifest[2].identity.runtime_id, 1);
    let main = crate::actor::gpu::ActorDrawFrame {
        artwork_identity: [0; 32],
        skin_revision: 1,
        geometry_revision: frame.geometry_revision,
        frame_generation: frame.frame_generation,
        draw_generation: 1,
        manifest: Arc::from(&frame.manifest[..2]),
    };
    assert!(main.is_exact());
    assert_eq!(main.manifest[0].instance_index, 0);
    assert_eq!(main.manifest[1].instance_index, 1);
}

#[test]
fn combined_session_pack_replacement_rebinds_cached_poses_to_new_pivots() {
    for id in [
        render_model::pack_rig_id(0),
        render_model::pack_equipment_rig_id(0),
    ] {
        let geometry = ActorRigGeometry::synthetic_cuboid(id, [0.0; 3], [1.0; 3], 1).unwrap();
        let mut builder = ActorRigFrameBuilder::new([geometry.clone()]).unwrap();
        let mut input = submission(1, 1);
        input.input.rig = id;
        let before = builder.build(0.5, None, [input.clone()]);
        let mut replacement = geometry;
        replacement.bone_pivots = Arc::from([[0.25, 1.5, -0.5]]);
        let (entities, equipment) = if id == render_model::pack_rig_id(0) {
            (vec![replacement.clone()], Vec::new())
        } else {
            (Vec::new(), vec![replacement.clone()])
        };
        assert_eq!(
            builder.replace_session_pack_geometries(entities, equipment),
            (Ok(()), Ok(()))
        );
        let after = builder.build(0.5, None, [input.clone()]);
        assert_ne!(before.current_bones, after.current_bones, "rig {id:?}");
        assert_eq!(
            after.current_bones.as_ref(),
            reference_matrices(&input.input.current_bones, &replacement.bone_pivots).unwrap(),
            "rig {id:?}"
        );
        assert_eq!(
            after.previous_bones.as_ref(),
            reference_matrices(&input.input.previous_bones, &replacement.bone_pivots).unwrap(),
            "rig {id:?}"
        );
    }
}

#[test]
fn invalid_reset_generation_preserves_the_previous_maximum_vertex_count_behavior() {
    let geometry =
        ActorRigGeometry::synthetic_cuboid(EntityRigId(3), [0.0; 3], [1.0; 3], 1).unwrap();
    let vertices = geometry.vertices.len() as u32;
    let mut builder = ActorRigFrameBuilder::new([geometry]).unwrap();
    let mut input = submission(1, 3);
    input.input.reset_generation = u64::from(u32::MAX) + 1;
    let frame = builder.build(0.5, None, [input]);
    assert_eq!(frame.maximum_vertex_count, vertices);
    assert_eq!(frame.rejects.invalid_identity, 1);
    assert!(frame.instances.is_empty());
    assert!(frame.previous_bones.is_empty());
    assert!(frame.current_bones.is_empty());
}

#[test]
#[ignore = "benchmark"]
fn frame_cost_bench_actor_bone_arena_50x64() {
    let previous: Vec<_> = (0..64).map(bone).collect();
    let current: Vec<_> = (1..=64).map(bone).collect();
    let pivots = [[0.25, 0.5, -0.25]; 64];
    let mut samples = [Vec::new(), Vec::new()];
    let mut frames = [(Vec::new(), Vec::new()), (Vec::new(), Vec::new())];
    // Alternate order across eleven batches so a changing host load affects both paths.
    for batch in 0..11 {
        let order = if batch % 2 == 0 {
            [false, true]
        } else {
            [true, false]
        };
        for optimized in order {
            let started = std::time::Instant::now();
            let mut valid = true;
            for _ in 0..50 {
                let mut previous_arena = Vec::new();
                let mut current_arena = Vec::new();
                for _ in 0..50 {
                    if optimized {
                        valid &= append_pose_matrices(
                            &mut previous_arena,
                            std::hint::black_box(&previous),
                            &pivots,
                        );
                        valid &= append_pose_matrices(
                            &mut current_arena,
                            std::hint::black_box(&current),
                            &pivots,
                        );
                    } else {
                        previous_arena.extend(
                            reference_matrices(std::hint::black_box(&previous), &pivots).unwrap(),
                        );
                        current_arena.extend(
                            reference_matrices(std::hint::black_box(&current), &pivots).unwrap(),
                        );
                    }
                }
                frames[usize::from(optimized)] =
                    std::hint::black_box((previous_arena, current_arena));
            }
            samples[usize::from(optimized)].push(started.elapsed() / 50);
            assert!(valid);
        }
        assert_eq!(frames[0], frames[1]);
    }
    for values in &mut samples {
        values.sort();
    }
    eprintln!(
        "FRAME_COST actor_bone_arena_50x64: median_old={:.3}ms median_new={:.3}ms temporary_pose_allocations=100->0 batches=11",
        samples[0][5].as_secs_f64() * 1e3,
        samples[1][5].as_secs_f64() * 1e3,
    );
}

#[test]
fn review_render_pose_cache_accounts_for_changed_bone_pivots() {
    let pose: Arc<[RenderBoneTransform]> = Arc::from([bone(0)]);
    let mut cache = PoseMatrixCache::default();
    let mut old = Vec::new();
    assert!(cache.append(&mut old, &pose, EntityRigId(3), &[[0.0; 3]]));
    let pivots = [[1.0, 2.0, 3.0]];
    let expected = reference_matrices(&pose, &pivots).unwrap();
    let mut actual = Vec::new();
    assert!(cache.append(&mut actual, &pose, EntityRigId(3), &pivots));
    assert_eq!(actual, expected);
    assert_ne!(actual, old);
}

#[test]
fn draw_readiness_shares_rejections_without_changing_frame_or_cache_state() {
    let geometry =
        ActorRigGeometry::synthetic_cuboid(EntityRigId(3), [0.0; 3], [1.0; 3], 1).unwrap();
    let mut builder = ActorRigFrameBuilder::new([geometry]).unwrap();
    let good = submission(1, 1);
    assert!(builder.can_draw_submission(&good));
    assert_eq!(builder.frame_generation, 0);
    assert!(builder.matrices.entries.is_empty());
    assert_eq!(builder.build(0.0, None, [good.clone()]).instances.len(), 1);
    for change in 0..5 {
        let generation = builder.frame_generation;
        let cached = builder.matrices.entries.len();
        let mut bad = good.clone();
        match change {
            0 => bad.route = ActorRigRoute::NoDraw,
            1 => bad.input.rig = EntityRigId(u32::MAX),
            2 => bad.input.reset_generation = u64::MAX,
            3 => bad.world_from_actor[0][0] = f32::NAN,
            _ => {
                let mut bone = bone(0);
                bone.rotation = [0.0; 4];
                bad.input.current_bones = Arc::from([bone]);
            }
        }
        let allocated = crate::alloc_count::thread_allocations();
        assert!(!builder.can_draw_submission(&bad));
        assert_eq!(crate::alloc_count::thread_allocations() - allocated, 0);
        assert_eq!(builder.frame_generation, generation);
        assert_eq!(builder.matrices.entries.len(), cached);
        assert!(builder.build(0.0, None, [bad]).instances.is_empty());
    }
    builder.frame_generation = u64::MAX;
    assert!(!builder.can_draw_submission(&good));
    assert!(builder.build(0.0, None, [good]).instances.is_empty());
}
