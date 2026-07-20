use std::{mem::size_of, sync::Arc};

use bevy::math::{Mat4, Vec3};
use render::{
    ActorCullView, ActorGpuInstance, ActorRenderIdentity, ActorRenderScene, ActorRigFrameBuilder,
    ActorRigGeometry, ActorRigRenderInput, ActorRigRoute, ActorRigSubmission, EntityRigId,
    MAX_ACTOR_BONE_ARENA_BYTES, MAX_RENDER_BONES_PER_ACTOR, MAX_RENDERED_PLAYERS,
    RenderBoneTransform, STANDARD_SKIN_BYTES,
};

fn identity(runtime_id: u64, spawn_revision: u64) -> ActorRenderIdentity {
    ActorRenderIdentity {
        session_id: 7,
        dimension: -1,
        runtime_id,
        spawn_revision,
        ingress_sequence: runtime_id,
        source_tick: Some(runtime_id as i64),
        movement_revision: runtime_id,
        pose_generation: runtime_id,
    }
}

fn bone(translation: [f32; 3]) -> RenderBoneTransform {
    RenderBoneTransform {
        rotation: [0.0, 0.0, 0.0, 1.0],
        translation_scale: [translation[0], translation[1], translation[2], 1.0],
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
        input: input(runtime_id, spawn_revision, 2),
        world_from_actor: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 64.0],
            [0.0, 0.0, 1.0, 0.0],
        ],
        texture_layer: 0,
        texture_region: [0.0, 0.0, 1.0, 1.0],
        route: ActorRigRoute::Compiled,
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
    assert_eq!(size_of::<RenderBoneTransform>(), 32);
    assert_eq!(size_of::<ActorGpuInstance>(), 88);
    let instance = ActorGpuInstance {
        previous_bone_base: 12,
        current_bone_base: 13,
        geometry_id: 14,
        texture_layer: 15,
        partial_tick: 16.25,
        reset_generation: 17,
        texture_region: [18.25, 19.25, 20.25, 21.25],
        ..Default::default()
    };
    let words: [u32; 22] = bytemuck::cast(instance);
    assert_eq!(&words[12..16], &[12, 13, 14, 15]);
    assert_eq!(words[16], 16.25_f32.to_bits());
    assert_eq!(words[17], 17);
    assert_eq!(
        &words[18..22],
        &[
            18.25_f32.to_bits(),
            19.25_f32.to_bits(),
            20.25_f32.to_bits(),
            21.25_f32.to_bits(),
        ]
    );
    assert_eq!(MAX_RENDER_BONES_PER_ACTOR, 96);
    assert_eq!(
        MAX_ACTOR_BONE_ARENA_BYTES,
        MAX_RENDERED_PLAYERS * MAX_RENDER_BONES_PER_ACTOR * 2 * 48
    );
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
fn wide_compiled_rig_intersecting_the_frustum_is_not_player_box_culled() {
    let wide =
        ActorRigGeometry::synthetic_cuboid(EntityRigId(3), [-4.0, -0.5, -0.5], [4.0, 0.5, 0.5], 1)
            .unwrap();
    let mut builder = ActorRigFrameBuilder::new([wide]).unwrap();
    let mut actor = submission(9, 1);
    actor.input.previous_bones = Arc::from([bone([0.0; 3])]);
    actor.input.current_bones = Arc::from([bone([0.0; 3])]);
    actor.world_from_actor[0][3] = 3.0;
    actor.world_from_actor[1][3] = 0.0;
    let view = ActorCullView {
        clip_from_world: Mat4::IDENTITY,
        camera_position: Vec3::ZERO,
        max_distance: 192.0,
    };

    let frame = builder.build(0.5, Some(view), [actor]);

    assert_eq!(frame.instances.len(), 1);
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
fn skin_layer_outside_the_bounded_texture_array_fails_the_frame_closed() {
    let mut scene = ActorRenderScene::default();
    let mut actor = diagnostic_submission(1, 1);
    actor.texture_layer = 1;

    let frame = scene.update_rigs(
        0.5,
        None,
        [actor],
        Arc::from(vec![255_u8; STANDARD_SKIN_BYTES]),
    );

    assert!(frame.rig.instances.is_empty());
    assert!(frame.skins_rgba8.is_empty());
    assert_eq!(frame.rig.rejects.invalid_geometry, 1);
}

#[test]
fn multiple_drawable_actors_can_share_one_validated_skin_layer() {
    let mut scene = ActorRenderScene::default();

    let frame = scene.update_rigs(
        0.5,
        None,
        [diagnostic_submission(1, 1), diagnostic_submission(2, 1)],
        Arc::from(vec![255_u8; STANDARD_SKIN_BYTES]),
    );

    assert_eq!(frame.rig.instances.len(), 2);
    assert_eq!(frame.skins_rgba8.len(), 66 * 66 * 4);
    assert!(
        frame
            .rig
            .instances
            .iter()
            .all(|actor| actor.texture_layer == 0)
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
