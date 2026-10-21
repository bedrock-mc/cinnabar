//! First-person scenes retain native composition without interrupting pack terrain.

use super::*;
use render_scene::{HandRigScene, ViewmodelScene};

#[test]
fn active_hand_rig_keeps_pack_categories_drawn() {
    let mut world = super::native_tests::ready_world();
    world.init_resource::<HandRigScene>();
    world.init_resource::<ViewmodelScene>();
    super::native_tests::assert_replacement(&mut world);
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
    world.remove_resource::<ActorSkinPipelines>();
    super::native_tests::assert_replacement(&mut world);
    world.resource_mut::<HandRigScene>().clear();
    super::native_tests::assert_replacement(&mut world);
}

#[test]
fn active_static_viewmodel_keeps_pack_categories_drawn() {
    let mut world = super::native_tests::ready_world();
    world.init_resource::<HandRigScene>();
    world.init_resource::<ViewmodelScene>();
    super::native_tests::assert_replacement(&mut world);
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
    super::native_tests::assert_replacement(&mut world);
    world.resource_mut::<ViewmodelScene>().clear(&gate);
    super::native_tests::assert_replacement(&mut world);
}
