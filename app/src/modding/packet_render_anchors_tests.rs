//! Coverage for render-time roots, independent of packet snapshot cadence.
use super::*;

/// A final interpolated body publication, without requiring textures or a graphics adapter.
fn actor(position: [f32; 3]) -> render_scene::ActorRenderFrame {
    let mut frame = render_scene::ActorRenderFrame::default();
    frame.rig.instances = Arc::from([render_model::ActorGpuInstance {
        world_from_actor: [
            [1., 0., 0., position[0]],
            [0., 1., 0., position[1]],
            [0., 0., 1., position[2]],
        ],
        ..Default::default()
    }]);
    frame.rig.manifest = Arc::from([render_model::ActorDrawManifestEntry {
        identity: render_model::ActorRenderIdentity {
            session_id: 1,
            dimension: 0,
            runtime_id: 42,
            spawn_revision: 1,
            ingress_sequence: 1,
            source_tick: None,
            movement_revision: 1,
            pose_generation: render_api::PoseGeneration::new(1),
            layer: render_model::ACTOR_LAYER_BODY,
        },
        rig: render_model::EntityRigId(0),
        completed_tick: 1,
        reset_generation: 1,
        route: render_model::ActorRigRoute::Compiled,
        instance_index: 0,
        previous_bone_base: 0,
        current_bone_base: 0,
        bone_count: 1,
    }]);
    frame
}

#[test]
fn anchored_lines_follow_render_roots_without_waiting_for_another_guest_snapshot() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../crates/mod-api/native-render-v1.json"
    ))
    .unwrap();
    let source: Scene = serde_json::from_value(fixture["anchored_scene"].clone()).unwrap();
    let mut drawing = Drawing::default();
    let scope = DrawingSession {
        connection: 1,
        dimension: 0,
        permissions: mod_api::drawing::DrawingPermissions::ALL,
        ..Default::default()
    };
    drawing.update(Some(Arc::new(source)), scope);
    let camera = GlobalTransform::from_translation(Vec3::new(1., 65., 1.));
    drawing.follow_anchors(&actor([3., 64., 4.]), &camera);
    assert_eq!(drawing.resolved_lines[0].start, [1., 65., 0.9]);
    assert_eq!(drawing.resolved_lines[0].end, [3., 65.62, 4.]);
    let geometry_revision = drawing.store.snapshot().geometry_revision;
    let previous = Arc::clone(drawing.store.snapshot());
    drawing.follow_anchors(&actor([7., 64., 4.]), &camera);
    assert_eq!(drawing.resolved_lines[0].end, [7., 65.62, 4.]);
    assert_ne!(drawing.store.snapshot().instances, previous.instances);
    assert_eq!(
        drawing.store.snapshot().geometry_revision,
        geometry_revision
    );
    drawing.follow_anchors(&render_scene::ActorRenderFrame::default(), &camera);
    assert!(drawing.output.retained.is_empty());
    let mut other_dimension = actor([7., 64., 4.]);
    Arc::make_mut(&mut other_dimension.rig.manifest)[0]
        .identity
        .dimension = 1;
    drawing.follow_anchors(&other_dimension, &camera);
    assert!(drawing.output.retained.is_empty());
    drawing.update(None, scope);
    drawing.follow_anchors(&actor([7., 64., 4.]), &camera);
    assert!(drawing.output.retained.is_empty());
}
