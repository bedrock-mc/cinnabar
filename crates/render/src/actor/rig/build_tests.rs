use std::{cell::Cell, collections::HashMap, sync::Arc};

use super::*;

/// Deterministic xorshift stream for generated batches.
struct Random(u64);

impl Random {
    /// Advances the deterministic fixture generator.
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// Returns a deterministic fixture choice below a positive bound.
    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }
}

/// Creates a reproducible render-bone transform from its seed.
fn bone(seed: u64) -> RenderBoneTransform {
    let value = |shift: u64| ((seed >> shift) % 97) as f32 / 48.0 - 1.0;
    RenderBoneTransform {
        rotation: [value(0), value(7), value(14), 0.5 + value(21).abs()],
        translation_scale: [value(28), value(35), value(42), 1.0],
        axis_scale: [1.0 + value(49).abs(), 1.0, 0.5 + value(56).abs(), 1.0],
    }
}

/// Creates a shared seeded pose for repeated actor-draw comparisons.
fn pose(seed: u64, bones: usize) -> Arc<[RenderBoneTransform]> {
    (0..bones as u64)
        .map(|index| bone(seed ^ index.wrapping_mul(0x9e37_79b9)))
        .collect()
}

/// Three geometries with distinct bone counts and pivots, one of them pivoting off the origin.
fn geometries() -> Vec<ActorRigGeometry> {
    [(3, 4, 0.0), (4, 6, 0.25), (5, 2, -1.5)]
        .into_iter()
        .map(|(id, bones, pivot)| {
            let cuboid =
                ActorRigGeometry::synthetic_cuboid(EntityRigId(id), [0.0; 3], [1.0; 3], bones)
                    .unwrap();
            let pivots: Vec<[f32; 3]> = (0..bones)
                .map(|bone| [pivot, bone as f32 * 0.5, -pivot])
                .collect();
            ActorRigGeometry::new(EntityRigId(id), cuboid.vertices.clone(), pivots).unwrap()
        })
        .collect()
}

/// Creates an actor draw with the chosen identity, rig and previous/current poses.
fn submission(
    runtime_id: u64,
    layer: u8,
    rig: u32,
    bones: [Arc<[RenderBoneTransform]>; 2],
) -> ActorRigSubmission {
    let [previous_bones, current_bones] = bones;
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
                layer,
            },
            rig: EntityRigId(rig),
            previous_bones,
            current_bones,
            completed_tick: 7,
            reset_generation: 1,
        },
        world_from_actor: [
            [1.0, 0.0, 0.0, runtime_id as f32],
            [0.0, 1.0, 0.0, 64.0],
            [0.0, 0.0, 1.0, layer as f32],
        ],
        texture_layer: u32::from(layer),
        route: ActorRigRoute::Compiled,
        tint: 0,
        uv_anim: crate::IDENTITY_UV_ANIM,
        light: 0,
        overlay_rgba8: 0,
    }
}

/// Chooses a repeatable fixture bone count from the rig identity.
fn bone_count(rig: u32) -> usize {
    match rig {
        3 => 4,
        4 => 6,
        _ => 2,
    }
}

/// One frame's batch: duplicated actor layers, shared and fresh poses, endpoints sharing one
/// allocation, unknown geometry, invalid poses, transforms and generations, and hidden draws.
fn batch(
    random: &mut Random,
    frame: u64,
    persistent: &[Arc<[RenderBoneTransform]>],
) -> (
    Vec<ActorRigSubmission>,
    HashMap<ActorRenderIdentity, ActorArtworkLocation>,
) {
    let mut submissions = Vec::new();
    let mut assignments = HashMap::new();
    for _ in 0..60 {
        let runtime_id = 1 + random.below(24);
        let layer = [0, 0, 1, 24, 32, 40][random.below(6) as usize];
        let rig = [3, 4, 5, 5, 9][random.below(5) as usize];
        let bones = bone_count(rig);
        let kept: Vec<_> = persistent
            .iter()
            .filter(|pose| pose.len() == bones)
            .collect();
        let previous = match random.below(3) {
            0 => pose(random_seed(runtime_id, frame), bones),
            _ => Arc::clone(kept[(runtime_id as usize + rig as usize) % kept.len()]),
        };
        let shared = random.below(2) == 0;
        let current = if shared {
            Arc::clone(&previous)
        } else {
            pose(random_seed(runtime_id, frame + 1), bones)
        };
        let mut entry = submission(runtime_id, layer, rig, [previous, current]);
        entry.input.identity.movement_revision = random.below(3);
        match random.below(20) {
            0 => entry.route = ActorRigRoute::NoDraw,
            1 => entry.world_from_actor[1][1] = f32::NAN,
            2 => entry.input.reset_generation = u64::from(u32::MAX) + 1,
            3 => {
                let mut bones = entry.input.current_bones.to_vec();
                bones[0].rotation = [0.0; 4];
                entry.input.current_bones = bones.into();
            }
            4 => {
                let mut bones = entry.input.previous_bones.to_vec();
                let last = bones.len() - 1;
                bones[last].translation_scale[0] = f32::INFINITY;
                entry.input.previous_bones = bones.into();
            }
            5 => entry.input.identity.ingress_sequence = 0,
            _ => {}
        }
        if layer > 0 && random.below(3) > 0 {
            let page = random.below(4) as ActorArtworkPageId;
            let location = ActorArtworkLocation {
                page,
                layer: entry.texture_layer,
                pose_mode: assets::ActorPoseMode::CompiledLiteral,
                multitexture: (random.below(2) == 0).then(|| [page.into(), 7]),
            };
            assignments.insert(entry.input.identity, location);
        }
        submissions.push(entry);
    }
    (submissions, assignments)
}

/// Poses kept across frames for every geometry's bone count, so their matrices are cached.
fn persistent_poses(seed: u64) -> Vec<Arc<[RenderBoneTransform]>> {
    [2, 4, 6]
        .into_iter()
        .flat_map(|bones| (0..7).map(move |index| pose(seed * 131 + index, bones)))
        .collect()
}

/// Combines actor and frame identities into a reproducible fixture seed.
fn random_seed(runtime_id: u64, frame: u64) -> u64 {
    runtime_id.wrapping_mul(0x2545_f491_4f6c_dd1d) ^ frame.wrapping_mul(0x9e37_79b9_7f4a_7c15)
}

/// Rebuilds the prior draw path with stable submission sorts and repeated page/matrix lookup.
fn reference(
    catalog: &GeometryCatalog,
    partial_tick: f32,
    submissions: Vec<ActorRigSubmission>,
    assignments: &HashMap<ActorRenderIdentity, ActorArtworkLocation>,
) -> (ActorRigRenderFrame, Vec<ActorArtworkPageId>) {
    let page_of = |identity: &ActorRenderIdentity| {
        assignments
            .get(identity)
            .map_or(0, |location| location.page)
    };
    let key = |submission: &ActorRigSubmission| {
        let identity = submission.input.identity;
        (
            identity.session_id,
            identity.dimension,
            identity.runtime_id,
            identity.layer,
        )
    };
    let mut ordered = submissions;
    ordered.sort_by(|a, b| {
        key(a)
            .cmp(&key(b))
            .then(b.input.identity.cmp(&a.input.identity))
    });
    ordered.dedup_by_key(|submission| key(submission));
    ordered.sort_by_key(|submission| {
        let identity = submission.input.identity;
        (identity.layer, page_of(&identity), submission.input.rig)
    });
    let (mut instances, mut previous_bones, mut current_bones, mut manifest) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut maximum_vertex_count = 0;
    let mut rejects = ActorRigRejects::default();
    for submission in ordered {
        if let Err(error) = eligibility::validate_input(&submission) {
            error.count(&mut rejects);
            continue;
        }
        if !actor_rig_submission_is_visible(&submission, None) {
            continue;
        }
        if instances.len() == MAX_ACTOR_RENDER_INSTANCES {
            rejects.actor_capacity += 1;
            continue;
        }
        let (geometry_id, geometry) = match eligibility::geometry(catalog, &submission) {
            Ok(geometry) => geometry,
            Err(error) => {
                error.count(&mut rejects);
                continue;
            }
        };
        if previous_bones.len() + submission.input.previous_bones.len() > MAX_ACTOR_POSE_BONES {
            rejects.bone_capacity += 1;
            continue;
        }
        let previous_bone_base = previous_bones.len() as u32;
        let current_bone_base = current_bones.len() as u32;
        let previous_valid = bone_arena::append_pose_matrices(
            &mut previous_bones,
            &submission.input.previous_bones,
            &geometry.bone_pivots,
        );
        let current_valid = bone_arena::append_pose_matrices(
            &mut current_bones,
            &submission.input.current_bones,
            &geometry.bone_pivots,
        );
        if !previous_valid || !current_valid {
            previous_bones.truncate(previous_bone_base as usize);
            current_bones.truncate(current_bone_base as usize);
            rejects.non_finite_pose += 1;
            continue;
        }
        let Some(&geometry_index) = catalog.indices.get(&geometry_id) else {
            previous_bones.truncate(previous_bone_base as usize);
            current_bones.truncate(current_bone_base as usize);
            rejects.invalid_geometry += 1;
            continue;
        };
        maximum_vertex_count =
            maximum_vertex_count.max(catalog.published_spans[geometry_index as usize].vertex_count);
        let Ok(reset_generation) = u32::try_from(submission.input.reset_generation) else {
            previous_bones.truncate(previous_bone_base as usize);
            current_bones.truncate(current_bone_base as usize);
            rejects.invalid_identity += 1;
            continue;
        };
        let instance_index = instances.len() as u32;
        instances.push(ActorGpuInstance {
            world_from_actor: submission.world_from_actor,
            previous_bone_base,
            current_bone_base,
            geometry_id: geometry_index,
            texture_layer: submission.texture_layer,
            partial_tick,
            reset_generation,
            tint: submission.tint,
            uv_anim: sanitized_uv_anim(submission.uv_anim),
            light: submission.light,
            overlay_rgba8: submission.overlay_rgba8,
            multitexture_layers: [u32::MAX; 2],
            material: submission.material.gpu_word(),
            glint: submission.material.glint.parameters(),
            dissolve_multiplier: if submission.material.dissolve_multiplier.is_finite() {
                submission.material.dissolve_multiplier.max(0.0)
            } else {
                1.0
            },
            light_color_multiplier: if submission.material.light_color_multiplier.is_finite() {
                submission.material.light_color_multiplier
            } else {
                1.0
            },
        });
        manifest.push(ActorDrawManifestEntry {
            identity: submission.input.identity,
            rig: submission.input.rig,
            completed_tick: submission.input.completed_tick,
            reset_generation: submission.input.reset_generation,
            route: submission.route,
            instance_index,
            previous_bone_base,
            current_bone_base,
            bone_count: submission.input.previous_bones.len() as u32,
        });
    }
    for (instance, entry) in instances.iter_mut().zip(&manifest) {
        instance.multitexture_layers = assignments
            .get(&entry.identity)
            .and_then(|location| location.multitexture)
            .unwrap_or([u32::MAX; 2]);
    }
    let pages = manifest
        .iter()
        .map(|entry| page_of(&entry.identity))
        .collect();
    let frame = ActorRigRenderFrame {
        instances: instances.into(),
        previous_bones: previous_bones.into(),
        current_bones: current_bones.into(),
        manifest: manifest.into(),
        maximum_vertex_count,
        rejects,
        ..ActorRigRenderFrame::default()
    };
    (frame, pages)
}

/// Builds with located submissions draw exactly what the stable-sorting, uncached build drew,
/// frame after frame as the matrix cache warms, for every instance, bone, entry and page.
#[test]
fn located_builds_match_the_reference_draw() {
    let mut builder = ActorRigFrameBuilder::new(geometries()).unwrap();
    let mut random = Random(0x0123_4567_89ab_cdef);
    let persistent = persistent_poses(31);
    let mut drawn = 0;
    for frame in 0..40 {
        let (submissions, assignments) = batch(&mut random, frame, &persistent);
        let partial_tick = (frame % 5) as f32 / 4.0;
        let (expected, expected_pages) = reference(
            &builder.catalog,
            partial_tick,
            submissions.clone(),
            &assignments,
        );
        let located = submissions.into_iter().map(|submission| {
            let location = assignments.get(&submission.input.identity).copied();
            (submission, location)
        });
        let actual = builder.build_located(partial_tick, None, located, |_, location| {
            location.map_or(0, |location| location.page)
        });
        let bytes = |frame: &ActorRigRenderFrame| {
            [
                bytemuck::cast_slice::<_, u8>(&frame.instances[..]).to_vec(),
                bytemuck::cast_slice::<_, u8>(&frame.previous_bones[..]).to_vec(),
                bytemuck::cast_slice::<_, u8>(&frame.current_bones[..]).to_vec(),
            ]
        };
        assert_eq!(bytes(&actual), bytes(&expected), "frame {frame}");
        assert_eq!(actual.manifest, expected.manifest, "frame {frame}");
        assert_eq!(actual.rejects, expected.rejects, "frame {frame}");
        assert_eq!(actual.maximum_vertex_count, expected.maximum_vertex_count);
        let pages: Vec<_> = builder
            .instance_locations()
            .iter()
            .map(|location| location.map_or(0, |location| location.page))
            .collect();
        assert_eq!(pages, expected_pages, "frame {frame}");
        drawn += actual.instances.len();
    }
    assert!(drawn > 400, "{drawn} instances");
}

/// A paged build asks each submission that survives deduplication for its page exactly once.
#[test]
fn paged_builds_ask_each_page_once() {
    let mut builder = ActorRigFrameBuilder::new(geometries()).unwrap();
    let mut random = Random(0xfeed_beef);
    let persistent = persistent_poses(1);
    let (submissions, assignments) = batch(&mut random, 1, &persistent);
    let mut keys: Vec<_> = submissions
        .iter()
        .map(|submission| {
            let identity = submission.input.identity;
            (
                identity.session_id,
                identity.dimension,
                identity.runtime_id,
                identity.layer,
            )
        })
        .collect();
    keys.sort_unstable();
    keys.dedup();
    let asked = Cell::new(0);
    let frame = builder.build_paged(0.5, None, submissions, |identity| {
        asked.set(asked.get() + 1);
        assignments
            .get(identity)
            .map_or(0, |location| location.page)
    });
    assert!(!frame.instances.is_empty());
    assert_eq!(asked.get(), keys.len());
}

/// Steady frames of fresh poses, each posing both endpoints with one allocation, allocate
/// within a fixed budget however many actors draw: cached matrices reuse released buffers.
#[test]
fn steady_frames_of_fresh_poses_allocate_independently_of_actor_count() {
    let allocations = |actors: u64| {
        let mut builder = ActorRigFrameBuilder::new(geometries()).unwrap();
        let mut worst = 0;
        for frame in 0..48 {
            let submissions: Vec<_> = (1..=actors)
                .map(|runtime_id| {
                    let bones = pose(random_seed(runtime_id, frame), 6);
                    submission(runtime_id, 0, 4, [Arc::clone(&bones), bones])
                })
                .collect();
            let before = crate::alloc_count::thread_allocations();
            let built = builder.build(0.5, None, submissions);
            let allocated = crate::alloc_count::thread_allocations() - before;
            assert_eq!(built.instances.len() as u64, actors);
            if frame >= 32 {
                worst = worst.max(allocated);
            }
        }
        worst
    };
    // Four published arrays allocate each frame. The pose map may also rebuild its
    // table after removals, depending on its randomized buckets and allocator reuse.
    for actors in [4, 40, 400] {
        let allocated = allocations(actors);
        assert!(
            allocated <= 5,
            "{allocated} allocations for {actors} actors"
        );
    }
}
