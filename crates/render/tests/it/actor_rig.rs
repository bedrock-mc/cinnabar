use std::{mem::size_of, sync::Arc};

use bevy::math::{Mat4, Vec3};
use render::{
    ActorCullView, ActorGpuInstance, ActorRenderIdentity, ActorRenderScene, ActorRigFrameBuilder,
    ActorRigRenderInput, ActorRigRoute, ActorRigSubmission, MAX_ACTOR_BONE_ARENA_BYTES,
    MAX_ACTOR_RENDER_INSTANCES, pack_overlay_rgba8,
};
use render_api::SkinRgba8;
use render_model::{
    ActorRigGeometry, EntityRigId, MAX_RENDER_BONES_PER_ACTOR, MAX_RENDERED_PLAYERS,
    RenderBoneTransform, STANDARD_SKIN_BYTES,
};

fn identity(runtime_id: u64, spawn_revision: u64) -> ActorRenderIdentity {
    ActorRenderIdentity {
        session_id: 7,
        dimension: -1,
        runtime_id,
        spawn_revision,
        ingress_sequence: runtime_id,
        source_tick: Some(runtime_id),
        movement_revision: runtime_id,
        pose_generation: runtime_id,
        layer: 0,
    }
}

fn bone(translation: [f32; 3]) -> RenderBoneTransform {
    RenderBoneTransform {
        rotation: [0.0, 0.0, 0.0, 1.0],
        translation_scale: [translation[0], translation[1], translation[2], 1.0],
        axis_scale: render_model::UNIT_AXIS_SCALE,
    }
}

fn input(runtime_id: u64, spawn_revision: u64, bones: usize) -> ActorRigRenderInput {
    ActorRigRenderInput {
        identity: identity(runtime_id, spawn_revision),
        rig: EntityRigId(3),
        previous_bones: Arc::from(vec![bone([0.0, 0.0, 0.0]); bones]),
        current_bones: Arc::from(vec![bone([1.0, 0.0, 0.0]); bones]),
        completed_tick: 11,
        reset_generation: 5,
    }
}

fn geometry() -> ActorRigGeometry {
    ActorRigGeometry::synthetic_cuboid(EntityRigId(3), [0.0, 0.0, 0.0], [1.0, 2.0, 1.0], 1)
        .expect("finite bounded synthetic rig")
}

fn submission(runtime_id: u64, spawn_revision: u64) -> ActorRigSubmission {
    ActorRigSubmission {
        material: Default::default(),
        input: input(runtime_id, spawn_revision, 2),
        world_from_actor: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 64.0],
            [0.0, 0.0, 1.0, 0.0],
        ],
        texture_layer: 0,
        route: ActorRigRoute::Compiled,
        tint: 0,
        uv_anim: render::IDENTITY_UV_ANIM,
        light: 0,
        overlay_rgba8: 0,
        culling_bounds: Default::default(),
    }
}

fn diagnostic_submission(runtime_id: u64, spawn_revision: u64) -> ActorRigSubmission {
    ActorRigSubmission {
        input: input(runtime_id, spawn_revision, 6),
        route: ActorRigRoute::Diagnostic,
        ..submission(runtime_id, spawn_revision)
    }
}

#[test]
fn shader_layouts_are_exact_and_the_dual_pose_arena_is_bounded() {
    // Bone poses reach the GPU as 48-byte affine matrices, not in this CPU form.
    assert_eq!(size_of::<RenderBoneTransform>(), 48);
    assert_eq!(
        size_of::<ActorGpuInstance>(),
        render::ACTOR_GPU_INSTANCE_WORDS * 4
    );
    assert_eq!(
        MAX_RENDER_BONES_PER_ACTOR,
        assets::MAX_ENTITY_GEOMETRY_BONES
    );
    assert!(
        MAX_ACTOR_BONE_ARENA_BYTES
            >= MAX_RENDER_BONES_PER_ACTOR * 2 * size_of::<RenderBoneTransform>()
    );
    assert!(
        MAX_ACTOR_BONE_ARENA_BYTES
            < MAX_ACTOR_RENDER_INSTANCES
                * MAX_RENDER_BONES_PER_ACTOR
                * 2
                * size_of::<RenderBoneTransform>()
    );
}

#[test]
fn rig_catalog_rejects_unknown_surface_contract_words() {
    let mut cube = geometry();
    Arc::make_mut(&mut cube.vertices)[0].surface =
        bytemuck::pod_read_unaligned(&2_u32.to_le_bytes());
    assert!(ActorRigGeometry::new(cube.id, cube.vertices, cube.bone_pivots).is_err());
}

#[test]
fn extraction_converts_parented_pose_endpoints_and_clamps_partial_tick() {
    let mut builder = ActorRigFrameBuilder::new([geometry()]).unwrap();
    let mut actor = submission(9, 1);
    actor.input.previous_bones = Arc::from([bone([1.0, 0.0, 0.0]), bone([1.0, 2.0, 0.0])]);
    actor.input.current_bones = Arc::from([bone([2.0, 0.0, 0.0]), bone([2.0, 2.0, 0.0])]);

    let frame = builder.build(f32::INFINITY, None, [actor]);

    assert_eq!(frame.instances.len(), 1);
    assert_eq!(frame.instances[0].partial_tick, 0.0);
    assert_eq!(frame.previous_bones.len(), 2);
    assert_eq!(frame.current_bones.len(), 2);
    assert_eq!(frame.previous_bones[1][0][3], 1.0);
    assert_eq!(frame.previous_bones[1][1][3], 2.0);
}

#[test]
fn invalid_pose_is_rejected_transactionally_and_no_draw_is_attributed() {
    let mut builder = ActorRigFrameBuilder::new([geometry()]).unwrap();
    let mut nonfinite = submission(1, 1);
    Arc::make_mut(&mut nonfinite.input.current_bones)[0].rotation[0] = f32::NAN;
    let mut mismatch = submission(2, 1);
    mismatch.input.current_bones = Arc::from([bone([0.0; 3])]);
    let mut no_draw = submission(3, 1);
    no_draw.route = ActorRigRoute::NoDraw;

    let frame = builder.build(0.5, None, [nonfinite, mismatch, no_draw]);

    assert!(frame.instances.is_empty());
    assert_eq!(frame.rejects.non_finite_pose, 1);
    assert_eq!(frame.rejects.pose_length_mismatch, 1);
    assert_eq!(frame.rejects.no_draw, 1);
    assert!(frame.previous_bones.is_empty());
    assert!(frame.current_bones.is_empty());
}

#[test]
fn culling_precedes_actor_and_bone_arena_reservation() {
    let mut builder = ActorRigFrameBuilder::new([geometry()]).unwrap();
    let view = ActorCullView {
        clip_from_world: Mat4::from_scale(Vec3::splat(0.001)),
        camera_position: Vec3::new(0.0, 65.0, 0.0),
        max_distance: 192.0,
    };
    let mut sources = (0..MAX_RENDERED_PLAYERS)
        .map(|index| {
            let mut source = submission(index as u64 + 1, 1);
            source.world_from_actor[0][3] = 500.0;
            source
        })
        .collect::<Vec<_>>();
    sources.push(submission(999, 1));

    let frame = builder.build(0.5, Some(view), sources);

    assert_eq!(frame.instances.len(), 1);
    assert_eq!(frame.manifest[0].identity.runtime_id, 999);
    assert_eq!(frame.previous_bones.len(), 2);
    assert_eq!(frame.rejects.actor_capacity, 0);
}

#[test]
fn shared_geometry_is_not_duplicated_per_actor_and_overflow_is_deterministic() {
    let mut builder = ActorRigFrameBuilder::new([geometry()]).unwrap();
    let geometry_vertex_count = builder.geometry_vertices().len();
    let actors = (0..MAX_RENDERED_PLAYERS + 2)
        .rev()
        .map(|index| submission(index as u64 + 1, 1));

    let frame = builder.build(0.25, None, actors);

    assert_eq!(frame.instances.len(), MAX_RENDERED_PLAYERS);
    assert_eq!(frame.rejects.actor_capacity, 2);
    assert_eq!(frame.geometry_vertices.len(), geometry_vertex_count);
    assert_eq!(frame.manifest.first().unwrap().identity.runtime_id, 1);
    assert_eq!(
        frame.manifest.last().unwrap().identity.runtime_id,
        MAX_RENDERED_PLAYERS as u64
    );
}

fn equipment_submission(runtime_id: u64, layer: u8) -> ActorRigSubmission {
    let mut equipment = submission(runtime_id, 1);
    equipment.input.identity.layer = layer;
    equipment.tint = 0x00ff_8040;
    equipment
}

#[test]
fn equipment_layers_share_the_actor_and_never_crowd_out_bodies() {
    let mut builder = ActorRigFrameBuilder::new([geometry()]).unwrap();
    let mut submissions = Vec::new();
    // One more layer per player than the instance budget holds, so some must be rejected.
    let layers_per_player = MAX_ACTOR_RENDER_INSTANCES / MAX_RENDERED_PLAYERS;
    for runtime_id in 1..=MAX_RENDERED_PLAYERS as u64 {
        for layer in (0..=layers_per_player as u8).rev() {
            submissions.push(equipment_submission(runtime_id, layer));
        }
    }
    // A repeat of one layer is a superseded duplicate, not a second instance.
    submissions.push(equipment_submission(1, 3));

    let frame = builder.build(0.5, None, submissions);

    assert_eq!(frame.instances.len(), MAX_ACTOR_RENDER_INSTANCES);
    let bodies = frame
        .manifest
        .iter()
        .filter(|entry| entry.identity.layer == 0)
        .count();
    assert_eq!(bodies, MAX_RENDERED_PLAYERS);
    assert!(
        frame.manifest[..MAX_RENDERED_PLAYERS]
            .iter()
            .all(|entry| entry.identity.layer == 0)
    );
    assert_eq!(frame.instances[MAX_RENDERED_PLAYERS].tint, 0x00ff_8040);
    assert!(frame.rejects.actor_capacity > 0);
}

#[test]
fn same_layer_keeps_only_the_newest_identity_per_actor() {
    let mut builder = ActorRigFrameBuilder::new([geometry()]).unwrap();
    let older = equipment_submission(4, 2);
    let mut newer = equipment_submission(4, 2);
    newer.input.identity.pose_generation += 1;
    let body = submission(4, 1);

    let frame = builder.build(0.5, None, [older, newer, body]);

    assert_eq!(frame.instances.len(), 2);
    assert_eq!(frame.manifest[1].identity.layer, 2);
}

#[test]
fn inserted_geometry_republishes_the_catalog_and_resolves_by_rig_id() {
    let mut builder = ActorRigFrameBuilder::new([geometry()]).unwrap();
    let before = builder.build(0.0, None, []).geometry_revision;
    let mesh = ActorRigGeometry::synthetic_cuboid(
        render_model::item_mesh_rig_id(0),
        [0.0; 3],
        [1.0; 3],
        1,
    )
    .unwrap();
    builder.insert_geometry(mesh).unwrap();
    assert!(builder.contains_geometry(render_model::item_mesh_rig_id(0)));

    let mut item = submission(1, 1);
    item.input.rig = render_model::item_mesh_rig_id(0);
    item.input.previous_bones = Arc::from([bone([0.0; 3])]);
    item.input.current_bones = Arc::from([bone([0.0; 3])]);
    let frame = builder.build(0.0, None, [item]);

    assert_ne!(frame.geometry_revision, before);
    assert_eq!(frame.instances.len(), 1);
    assert_eq!(frame.rejects.missing_geometry, 0);
}

#[test]
fn too_many_bones_and_arena_overflow_fail_closed_without_partial_reservation() {
    let mut builder = ActorRigFrameBuilder::new([geometry()]).unwrap();
    let frame = builder.build(
        0.0,
        None,
        [ActorRigSubmission {
            input: input(1, 1, MAX_RENDER_BONES_PER_ACTOR + 1),
            ..submission(1, 1)
        }],
    );

    assert!(frame.instances.is_empty());
    assert_eq!(frame.rejects.bone_capacity, 1);
    assert!(frame.previous_bones.is_empty());
    assert!(frame.current_bones.is_empty());
}

#[test]
fn replacement_and_reset_generations_remain_distinct_in_the_draw_manifest() {
    let mut builder = ActorRigFrameBuilder::new([geometry()]).unwrap();
    let first = submission(7, 1);
    let mut replacement = submission(7, 2);
    replacement.input.reset_generation = 9;

    let first_frame = builder.build(0.5, None, [first]);
    let replacement_frame = builder.build(0.5, None, [replacement]);

    assert_ne!(
        first_frame.manifest[0].identity,
        replacement_frame.manifest[0].identity
    );
    assert_eq!(first_frame.manifest[0].reset_generation, 5);
    assert_eq!(replacement_frame.manifest[0].reset_generation, 9);
    assert!(replacement_frame.frame_generation > first_frame.frame_generation);
}

#[test]
fn missing_geometry_uses_only_an_explicit_fallback_or_no_draw_route() {
    let mut builder = ActorRigFrameBuilder::new([]).unwrap();
    let mut compiled = submission(1, 1);
    compiled.route = ActorRigRoute::Compiled;
    let fallback = diagnostic_submission(2, 1);

    let frame = builder.build(0.5, None, [compiled, fallback]);

    assert_eq!(frame.instances.len(), 1);
    assert_eq!(frame.manifest[0].identity.runtime_id, 2);
    assert_eq!(frame.rejects.missing_geometry, 1);
    assert_eq!(frame.manifest[0].route, ActorRigRoute::Diagnostic);
}

#[test]
fn skin_layer_outside_the_bounded_texture_array_rejects_only_that_actor() {
    let mut scene = ActorRenderScene::default();
    let mut actor = diagnostic_submission(1, 1);
    actor.texture_layer = 1;

    let pixels: Arc<[u8]> = vec![255_u8; STANDARD_SKIN_BYTES].into();
    let frame = scene.update_rigs(
        0.5,
        None,
        [actor, diagnostic_submission(2, 1)],
        &[SkinRgba8::from(Arc::clone(&pixels))],
    );

    assert_eq!(frame.rig.instances.len(), 1);
    assert_eq!(frame.rig.manifest[0].identity.runtime_id, 2);
    let drawn = frame.player_skin(frame.rig.instances[0].texture_layer);
    assert!(Arc::ptr_eq(drawn.unwrap().pixels(), &pixels));
    assert_eq!(frame.rig.rejects.invalid_geometry, 1);
}

#[test]
fn multiple_drawable_actors_can_share_one_validated_skin_layer() {
    let mut scene = ActorRenderScene::default();

    let frame = scene.update_rigs(
        0.5,
        None,
        [diagnostic_submission(1, 1), diagnostic_submission(2, 1)],
        &[SkinRgba8::from(vec![255_u8; STANDARD_SKIN_BYTES])],
    );

    assert_eq!(frame.rig.instances.len(), 2);
    let layer = frame.rig.instances[0].texture_layer;
    assert!(frame.player_skin(layer).is_some());
    assert!(
        frame
            .rig
            .instances
            .iter()
            .all(|actor| actor.texture_layer == layer)
    );
}

#[test]
fn exact_spawn_identity_does_not_require_a_movement_packet() {
    let mut builder = ActorRigFrameBuilder::new([geometry()]).unwrap();
    let mut actor = submission(1, 1);
    actor.input.identity.movement_revision = 0;

    let frame = builder.build(0.5, None, [actor]);

    assert_eq!(frame.instances.len(), 1);
    assert_eq!(frame.rejects.invalid_identity, 0);
}

#[test]
fn per_axis_bone_scale_scales_matrix_columns_and_zero_scale_is_drawable() {
    let mut squashed = bone([0.0; 3]);
    squashed.axis_scale = [1.0, 0.5, 2.0, 1.0];
    squashed.translation_scale[3] = 2.0;
    assert!(squashed.is_finite());
    let mut hidden = bone([0.0; 3]);
    hidden.translation_scale[3] = 0.0;
    assert!(hidden.is_finite(), "vanilla hides bones with zero scale");
    let converted = RenderBoneTransform::from_model_space_scaled(
        [0.0, 0.0, 0.0, 1.0],
        [16.0, 0.0, 0.0, 1.0],
        [1.0, 0.5, 1.0],
    )
    .unwrap();
    assert_eq!(converted.axis_scale, [1.0, 0.5, 1.0, 1.0]);
    assert_eq!(converted.translation_scale[0], 1.0);
}

#[test]
fn overlay_packs_little_endian_rgba8_and_rejects_non_finite() {
    assert_eq!(pack_overlay_rgba8([1.0, 0.0, 0.0, 1.0]), 0xff00_00ff);
    assert_eq!(pack_overlay_rgba8([0.0, 0.0, 0.0, 0.0]), 0);
    assert_eq!(
        pack_overlay_rgba8([f32::NAN, 2.0, -1.0, 0.5]),
        0x80_00_ff_00
    );
}

#[test]
fn overlay_submission_reaches_the_gpu_instance() {
    let mut builder = ActorRigFrameBuilder::new([geometry()]).unwrap();
    let frame = builder.build(
        0.0,
        None,
        [ActorRigSubmission {
            overlay_rgba8: 0x6600_00ff,
            ..submission(1, 1)
        }],
    );
    assert_eq!(frame.instances[0].overlay_rgba8, 0x6600_00ff);
}

// uv_anim reaches the instance; a non-finite channel falls back to identity.
#[test]
fn uv_anim_submission_reaches_the_gpu_instance() {
    let mut builder = ActorRigFrameBuilder::new([geometry()]).unwrap();
    let frame = builder.build(
        0.0,
        None,
        [ActorRigSubmission {
            uv_anim: [0.0, 0.5, f32::NAN, 0.25],
            light: 0,
            ..submission(1, 1)
        }],
    );
    assert_eq!(frame.instances[0].uv_anim, [0.0, 0.5, 1.0, 0.25]);
}

// World light reaches the instance packed as block, sky and daylight with the lit bit set.
#[test]
fn packed_actor_light_reaches_the_gpu_instance() {
    let light = render::pack_actor_light(3, 12);
    assert_eq!(light, 0x8000_0000 | (12 << 4) | 3);
    assert_eq!(render::pack_actor_light(99, 99), 0x8000_0000 | 0xff);
    let mut builder = ActorRigFrameBuilder::new([geometry()]).unwrap();
    let frame = builder.build(
        0.0,
        None,
        [ActorRigSubmission {
            light,
            ..submission(1, 1)
        }],
    );
    assert_eq!(frame.instances[0].light, light);
}

#[test]
fn review_render_invalid_skin_layer_skips_only_its_actor() {
    let mut scene = ActorRenderScene::default();
    scene.insert_geometry(geometry()).unwrap();
    let valid = submission(1, 1);
    let mut invalid = submission(2, 1);
    invalid.texture_layer = 1;
    let frame = scene.update_rigs(
        0.5,
        None,
        [valid, invalid],
        &[SkinRgba8::from(vec![255; STANDARD_SKIN_BYTES])],
    );
    assert_eq!(frame.rig.manifest.len(), 1);
    assert_eq!(frame.rig.manifest[0].identity.runtime_id, 1);
    assert_eq!(frame.rig.rejects.invalid_geometry, 1);
    assert_eq!(frame.rig.instances.len(), 1);
    assert!(
        frame
            .player_skin(frame.rig.instances[0].texture_layer)
            .is_some()
    );
}

#[test]
fn review_render_catalog_recomputes_mutated_vertex_bone_requirements() {
    let mut geometry =
        ActorRigGeometry::synthetic_cuboid(EntityRigId(3), [0.0; 3], [1.0; 3], 2).unwrap();
    for vertex in Arc::make_mut(&mut geometry.vertices) {
        vertex.bone_index = 1;
    }
    let mut builder = ActorRigFrameBuilder::new([geometry.clone()]).unwrap();
    let mut actor = submission(1, 1);
    actor.input.previous_bones = Arc::from([bone([0.0; 3])]);
    actor.input.current_bones = Arc::clone(&actor.input.previous_bones);
    assert_eq!(
        builder
            .build(0.5, None, [actor.clone()])
            .rejects
            .invalid_geometry,
        1
    );
    let mut builder = ActorRigFrameBuilder::new([]).unwrap();
    builder.insert_geometry(geometry).unwrap();
    assert_eq!(
        builder.build(0.5, None, [actor]).rejects.invalid_geometry,
        1
    );
}

// Shared pixels and equal copies keep the revision; a changed final byte advances it.
#[test]
fn rig_skin_revision_tracks_pixels_across_shared_and_independent_payloads() {
    let mut scene = ActorRenderScene::default();
    let pixels: Arc<[u8]> = vec![7; STANDARD_SKIN_BYTES].into();
    let revision = scene
        .update_rigs(
            0.0,
            None,
            [diagnostic_submission(1, 1)],
            &[SkinRgba8::from(Arc::clone(&pixels))],
        )
        .skin_revision;
    for next in [Arc::clone(&pixels), Arc::from(pixels.to_vec())] {
        let frame = scene.update_rigs(
            0.0,
            None,
            [diagnostic_submission(1, 1)],
            &[SkinRgba8::from(next)],
        );
        assert_eq!(frame.skin_revision, revision);
        let drawn = frame.player_skin(frame.rig.instances[0].texture_layer);
        assert!(Arc::ptr_eq(drawn.unwrap().pixels(), &pixels));
    }
    let mut changed = pixels.to_vec();
    *changed.last_mut().unwrap() = 8;
    let changed: Arc<[u8]> = changed.into();
    let frame = scene.update_rigs(
        0.0,
        None,
        [diagnostic_submission(1, 1)],
        &[SkinRgba8::from(Arc::clone(&changed))],
    );
    assert_eq!(frame.skin_revision, revision.wrapping_add(1));
    let drawn = frame.player_skin(frame.rig.instances[0].texture_layer);
    assert!(Arc::ptr_eq(drawn.unwrap().pixels(), &changed));
}
