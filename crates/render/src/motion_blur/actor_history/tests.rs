use super::*;
use bevy::math::{UVec4, Vec3, Vec4};
use render_api::PoseGeneration;
use render_model::{ActorDrawManifestEntry, ActorGpuInstance, ActorRigRoute, DIAGNOSTIC_RIG_ID};
use std::sync::Arc;

/// A one-bone publication whose position, pose endpoints and interpolation come from the caller.
pub(crate) fn frame(position: Vec3, pose: [f32; 3]) -> ActorRigRenderFrame {
    let rows = |x| {
        [
            [1.0, 0.0, 0.0, x],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ]
    };
    let mut root = rows(0.0);
    for (row, value) in root.iter_mut().zip(position.to_array()) {
        row[3] = value;
    }
    ActorRigRenderFrame {
        instances: Arc::from([ActorGpuInstance {
            world_from_actor: root,
            partial_tick: pose[2],
            ..Default::default()
        }]),
        previous_bones: Arc::from([rows(pose[0])]),
        current_bones: Arc::from([rows(pose[1])]),
        manifest: Arc::from([ActorDrawManifestEntry {
            identity: ActorRenderIdentity {
                session_id: 1,
                dimension: 0,
                runtime_id: 7,
                spawn_revision: std::num::NonZeroU64::MIN,
                ingress_sequence: 1,
                source_tick: Some(1),
                movement_revision: 1,
                pose_generation: PoseGeneration::new(1),
                layer: 0,
            },
            rig: DIAGNOSTIC_RIG_ID,
            completed_tick: 1,
            reset_generation: 0,
            route: ActorRigRoute::Compiled,
            instance_index: 0,
            previous_bone_base: 0,
            current_bone_base: 0,
            bone_count: 1,
        }]),
        ..Default::default()
    }
}

/// Projects the shader's previous rendered bone and root for one current instance.
pub(in crate::motion_blur) fn previous_point(
    history: &ActorHistory,
    index: usize,
    local: Vec3,
) -> Vec3 {
    let motion = history.instances[index];
    assert_eq!(motion.pose[1], 1);
    let bone = history.bones[motion.pose[0] as usize];
    let posed = Vec3::from_array(bone.map(|row| Vec4::from_array(row).dot(local.extend(1.0))));
    motion.previous_clip_from_actor.project_point3(posed)
}

/// A stationary camera with identity projection makes root and bone history directly observable.
fn camera() -> CameraHistory {
    CameraHistory::new(
        Mat4::IDENTITY,
        Mat4::IDENTITY,
        UVec4::new(0, 0, 640, 320),
        0,
    )
}

#[test]
fn previous_rendered_pose_is_distinct_from_the_previous_simulation_tick() {
    let mut history = ActorHistory::default();
    history.advance(
        &frame(Vec3::ZERO, [0.0, 2.0, 0.25]),
        camera(),
        camera(),
        false,
    );
    assert!(history.advance(
        &frame(Vec3::ZERO, [0.0, 2.0, 0.75]),
        camera(),
        camera(),
        true
    ));
    assert!(previous_point(&history, 0, Vec3::ZERO).abs_diff_eq(Vec3::new(0.5, 0.0, 0.0), 0.00001));
    assert!(!history.advance(
        &frame(Vec3::ZERO, [0.0, 2.0, 0.75]),
        camera(),
        camera(),
        true
    ));
    assert!(previous_point(&history, 0, Vec3::ZERO).abs_diff_eq(Vec3::new(1.5, 0.0, 0.0), 0.00001));
}

#[test]
fn moving_actor_under_a_stationary_camera_retains_real_object_exposure() {
    let mut history = ActorHistory::default();
    history.advance(
        &frame(Vec3::ZERO, [0.0, 0.0, 1.0]),
        camera(),
        camera(),
        false,
    );
    let moved = history.advance(&frame(Vec3::X, [0.0, 0.0, 1.0]), camera(), camera(), true);
    assert!(moved);
    assert_eq!(previous_point(&history, 0, Vec3::ZERO), Vec3::ZERO);
    assert_eq!(
        history.instances[0]
            .current_clip_from_actor
            .project_point3(Vec3::ZERO),
        Vec3::X
    );
    let mut old_camera = camera();
    let exposure = old_camera.advance_with_actor_motion(
        camera(),
        crate::motion_blur::tests::settings(),
        moved,
    );
    assert!(exposure.strength.x > 0.0);
    assert_eq!(exposure.previous_clip_from_clip, Mat4::IDENTITY);
}

#[test]
fn cuts_resets_and_actor_lifetimes_do_not_inherit_previous_motion() {
    for change in 0..6 {
        let mut history = ActorHistory::default();
        let original = frame(Vec3::ZERO, [0.0, 0.0, 1.0]);
        history.advance(&original, camera(), camera(), false);
        let mut next = original.clone();
        let entry = &mut Arc::make_mut(&mut next.manifest)[0];
        match change {
            0 => {
                entry.identity.spawn_revision =
                    entry.identity.spawn_revision.checked_add(1).unwrap()
            }
            1 => entry.identity.session_id += 1,
            2 => entry.identity.dimension += 1,
            3 => entry.reset_generation += 1,
            4 => {
                history.advance(&ActorRigRenderFrame::default(), camera(), camera(), true);
            }
            _ => (),
        }
        assert!(!history.advance(&next, camera(), camera(), change != 5));
        assert_eq!(history.instances[0].pose[1], 0);
    }
}

/// Combines publications while keeping their bone ranges and instance addresses valid.
fn combine(frames: &[ActorRigRenderFrame]) -> ActorRigRenderFrame {
    let mut result = ActorRigRenderFrame::default();
    let mut instances = Vec::new();
    let mut manifest = Vec::new();
    let mut a = Vec::new();
    let mut b = Vec::new();
    for frame in frames {
        for (instance, entry) in frame.instances.iter().zip(frame.manifest.iter()) {
            let mut instance = *instance;
            let mut entry = entry.clone();
            instance.previous_bone_base += a.len() as u32;
            instance.current_bone_base += b.len() as u32;
            entry.previous_bone_base = instance.previous_bone_base;
            entry.current_bone_base = instance.current_bone_base;
            entry.instance_index = instances.len() as u32;
            instances.push(instance);
            manifest.push(entry);
        }
        a.extend_from_slice(&frame.previous_bones);
        b.extend_from_slice(&frame.current_bones);
    }
    result.instances = instances.into();
    result.manifest = manifest.into();
    result.previous_bones = a.into();
    result.current_bones = b.into();
    result
}

#[test]
fn reordered_actors_and_equipment_keep_their_own_root_and_pose_history() {
    let body = frame(Vec3::X, [0.0, 2.0, 0.25]);
    let mut equipment = frame(Vec3::X * 2.0, [2.0, 4.0, 0.25]);
    Arc::make_mut(&mut equipment.manifest)[0].identity.layer = 1;
    let mut remote = frame(Vec3::X * 3.0, [4.0, 6.0, 0.25]);
    Arc::make_mut(&mut remote.manifest)[0].identity.runtime_id = 11;
    let mut history = ActorHistory::default();
    history.advance(
        &combine(&[body.clone(), equipment.clone(), remote.clone()]),
        camera(),
        camera(),
        false,
    );
    let mut next = combine(&[remote, equipment, body]);
    for entry in Arc::make_mut(&mut next.manifest) {
        entry.identity.movement_revision += 1;
        entry.identity.pose_generation = PoseGeneration::new(2);
        entry.identity.ingress_sequence += 1;
    }
    assert!(!history.advance(&next, camera(), camera(), true));
    for (index, expected_x) in [7.5, 4.5, 1.5].into_iter().enumerate() {
        assert!(
            previous_point(&history, index, Vec3::ZERO).abs_diff_eq(Vec3::X * expected_x, 0.00001)
        );
    }
}

#[test]
fn repeated_material_layers_share_one_previous_rendered_bone_sample() {
    let original = frame(Vec3::ZERO, [0.0, 2.0, 0.25]);
    let mut repeated = original.clone();
    let mut second = repeated.manifest[0].clone();
    second.instance_index = 1;
    repeated.instances = Arc::from([original.instances[0], original.instances[0]]);
    repeated.manifest = Arc::from([original.manifest[0].clone(), second]);
    let mut history = ActorHistory::default();
    history.advance(&repeated, camera(), camera(), false);
    history.advance(&repeated, camera(), camera(), true);
    assert_eq!(history.bones.len(), 1);
    assert_eq!(history.instances[0].pose, history.instances[1].pose);
}

#[test]
fn actor_projection_subtracts_distant_camera_origins_without_cancellation() {
    let local = Vec3::new(0.25, 1.0, -4.0);
    for origin in [
        Vec3::ZERO,
        Vec3::splat(1_000_000.0),
        Vec3::splat(-1_000_000.0),
    ] {
        let frame = frame(origin + Vec3::new(0.0, 0.0, -4.0), [0.0, 0.0, 1.0]);
        let camera = CameraHistory::new(
            Mat4::IDENTITY,
            Mat4::from_translation(origin),
            UVec4::new(0, 0, 640, 320),
            0,
        );
        let mut history = ActorHistory::default();
        history.advance(&frame, camera, camera, false);
        history.advance(&frame, camera, camera, true);
        assert!(previous_point(&history, 0, Vec3::new(0.25, 1.0, 0.0)).abs_diff_eq(local, 0.00001));
    }
}

#[test]
fn camera_followed_actor_keeps_sub_block_detail_in_both_frame_projections() {
    let viewport = UVec4::new(0, 0, 1920, 1080);
    let projection = glam::camera::rh::proj::directx::perspective_infinite_reverse(
        std::f32::consts::FRAC_PI_2,
        viewport.z as f32 / viewport.w as f32,
        0.1,
    );
    for origin in [
        Vec3::ZERO,
        Vec3::splat(1_000_000.0),
        Vec3::splat(-1_000_000.0),
    ] {
        let old_camera =
            CameraHistory::new(projection, Mat4::from_translation(origin), viewport, 0);
        let next_origin = origin + Vec3::X;
        let next_camera =
            CameraHistory::new(projection, Mat4::from_translation(next_origin), viewport, 0);
        let mut history = ActorHistory::default();
        history.advance(
            &frame(origin - Vec3::Z, [0.0, 0.0, 1.0]),
            old_camera,
            old_camera,
            false,
        );
        let next = frame(next_origin - Vec3::Z, [0.0, 0.0, 1.0]);
        history.advance(&next, old_camera, next_camera, true);
        let local = Vec3::new(0.02, 0.02, 0.0);
        let current = history.instances[0]
            .current_clip_from_actor
            .project_point3(local);
        let previous = previous_point(&history, 0, local);
        assert!(
            current.x > 0.0 && current.y > 0.0,
            "local offsets must survive projection"
        );
        assert!(
            ((current - previous).truncate()
                * bevy::math::Vec2::new(viewport.z as f32, viewport.w as f32))
            .length()
                < 0.001,
            "camera-followed actor acquired screen motion at {origin:?}"
        );
        let mut moved = next;
        Arc::make_mut(&mut moved.current_bones)[0][0][3] = 0.02;
        history.advance(&moved, next_camera, next_camera, true);
        let current = history.instances[0]
            .current_clip_from_actor
            .project_point3(local + Vec3::X * 0.02);
        let previous = previous_point(&history, 0, local);
        assert!(
            (current.x - previous.x) * viewport.z as f32 > 1.0,
            "real sub-block bone motion was lost at {origin:?}"
        );
    }
}

#[test]
fn warm_actor_history_reuses_scratch_storage() {
    let frame = frame(Vec3::ZERO, [0.0, 2.0, 0.25]);
    let mut history = ActorHistory::default();
    history.advance(&frame, camera(), camera(), false);
    history.advance(&frame, camera(), camera(), true);
    let allocations = test_alloc::thread_allocations();
    assert!(!history.advance(&frame, camera(), camera(), true));
    assert_eq!(test_alloc::thread_allocations(), allocations);
}

#[test]
fn followed_actor_current_and_previous_motion_projections_agree_at_distant_coordinates() {
    let projection = glam::camera::rh::proj::directx::perspective_infinite_reverse(1.2, 2.0, 0.1);
    let origin = Vec3::splat(1_000_000.0);
    let world_from_view = Mat4::from_translation(origin);
    let view = CameraHistory::new(projection, world_from_view, UVec4::new(0, 0, 640, 320), 0);
    let frame = frame(origin + Vec3::new(0.0, 0.0, -4.0), [0.0, 0.0, 1.0]);
    let local = Vec3::new(0.25, 1.0, 0.0);
    let mut history = ActorHistory::default();
    history.advance(&frame, view, view, false);
    history.advance(&frame, view, view, true);
    let previous = previous_point(&history, 0, local);
    // Both shader inputs project model-local vertices after subtracting the camera origin.
    let current = history.instances[0]
        .current_clip_from_actor
        .project_point3(local);
    assert!(
        current.abs_diff_eq(previous, 0.00001),
        "{current:?} != {previous:?}"
    );
}
