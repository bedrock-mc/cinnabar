//! First-person scenes retain native composition even without other fallback layers.

use super::*;
use bevy::ecs::system::RunSystemOnce;
use render_scene::{HandRigScene, ViewmodelScene};

/// Creates a scene with neither a native sky nor active first-person rendering.
fn native_world() -> World {
    let mut world = World::new();
    world.insert_resource(
        render_scene::AtmosphereFrame::default().with_sky_kind(render_scene::SkyKind::Nether),
    );
    world.init_resource::<HandRigScene>();
    world.init_resource::<ViewmodelScene>();
    world
}

/// Reads the same extracted resources that control replacement and view uniforms.
fn scene_needs_copy(world: &mut World) -> bool {
    world
        .run_system_once(|native: NativeSceneLayers| native.first_person_native())
        .unwrap()
}

#[test]
fn active_hand_rig_preserves_native_scene() {
    let mut world = native_world();
    assert!(!scene_needs_copy(&mut world));
    let rig = render_model::ActorRigRenderFrame {
        instances: Arc::from([render_model::ActorGpuInstance::default()]),
        previous_bones: Arc::from([[[0.0; 4]; 3]]),
        current_bones: Arc::from([[[0.0; 4]; 3]]),
        maximum_vertex_count: 3,
        ..default()
    };
    assert!(world.resource_mut::<HandRigScene>().publish(
        rig,
        vec![255; render_model::STANDARD_SKIN_BYTES].into(),
        default(),
        1.0,
        1,
    ));
    assert!(scene_needs_copy(&mut world));
    world.resource_mut::<HandRigScene>().clear();
    assert!(!scene_needs_copy(&mut world));
}

#[test]
fn active_static_viewmodel_preserves_native_scene() {
    let mut world = native_world();
    assert!(!scene_needs_copy(&mut world));
    let token = render_scene::ViewmodelToken {
        session: 1,
        actor_session: 1,
        dimension: 1,
        runtime: 1,
        spawn: 1,
        owner: Entity::from_raw_u32(1).unwrap(),
        viewport: [4; 2],
        samples: 1,
        hdr: false,
        skin: [1; 32],
        geometry: [2; 32],
        revision: 1,
    };
    let skin = render_model::ViewmodelSkin::new(
        vec![255; render_model::viewmodel::VIEWMODEL_TEXTURE_BYTES].into(),
        token.skin,
    )
    .unwrap();
    let geometry = render_model::ViewmodelGeometry {
        vertices: Arc::from([]),
        identity: token.geometry,
        allowed_rigs: Arc::from([]),
        cube_origin: false,
    };
    let gate = render_scene::ViewmodelCompletionGate::default();
    assert!(
        world
            .resource_mut::<ViewmodelScene>()
            .publish(token, &skin, &geometry, &gate)
    );
    assert!(scene_needs_copy(&mut world));
    world.resource_mut::<ViewmodelScene>().clear(&gate);
    assert!(!scene_needs_copy(&mut world));
}
