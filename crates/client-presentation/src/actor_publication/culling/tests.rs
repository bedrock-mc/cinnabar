use std::sync::Arc;

use super::*;
use crate::presentation::actors::{
    local_diagnostic_presentation, select_actor_presentations_for_shadow_view,
};
use bevy::prelude::PerspectiveProjection;

/// Builds camera and publication views for the requested shadow mode.
fn views(casts_shadows: bool) -> ActorPublicationViews {
    ActorPublicationViews::new(
        &Transform::IDENTITY,
        &Projection::Perspective(PerspectiveProjection::default()),
        casts_shadows,
    )
}

/// Places a diagnostic remote actor at the requested world position.
fn remote(id: u64, feet: [f32; 3]) -> crate::presentation::actors::ActorRigPresentation {
    local_diagnostic_presentation(1, 0, id, 1, feet, 0.0, 0.0).unwrap()
}

#[test]
fn offscreen_casters_publish_and_animate_only_for_enhanced_shadows() {
    let behind = [0.0, 0.0, 8.0];
    let submission = remote(2, behind).submission;
    let vanilla = views(false);
    let enhanced = views(true);
    assert!(!render::actor_rig_submission_is_visible(
        &submission,
        Some(vanilla.publication)
    ));
    assert!(render::actor_rig_submission_is_visible(
        &submission,
        Some(enhanced.publication)
    ));
    let projection = Projection::Perspective(PerspectiveProjection::default());
    let vanilla_animation =
        animation_view(&Transform::IDENTITY, &projection, Some(vanilla)).unwrap();
    let enhanced_animation =
        animation_view(&Transform::IDENTITY, &projection, Some(enhanced)).unwrap();
    assert!(!vanilla_animation.admits(behind, 1.0, true, Default::default()));
    assert!(enhanced_animation.admits(behind, 1.0, true, Default::default()));
    assert!(!enhanced_animation.admits(
        [0.0, 0.0, render::MAX_ACTOR_RENDER_DISTANCE_BLOCKS + 8.0],
        1.0,
        true,
        Default::default()
    ));
    let batch = select_actor_presentations_for_shadow_view(
        99,
        false,
        None,
        [remote(2, behind)],
        Some(enhanced.main),
        Some(enhanced.publication),
    );
    assert_eq!(batch.submissions.len(), 1);
    let vanilla_batch = select_actor_presentations_for_shadow_view(
        99,
        false,
        None,
        [remote(2, behind)],
        Some(vanilla.main),
        None,
    );
    assert!(vanilla_batch.submissions.is_empty());
}

#[test]
fn visible_actors_precede_offscreen_casters_without_capping_publication() {
    let views = views(true);
    let remotes = (1..=render_model::MAX_RENDERED_PLAYERS as u64 + 1).map(|id| {
        remote(
            id,
            if id == render_model::MAX_RENDERED_PLAYERS as u64 + 1 {
                [0.0, 0.0, -8.0]
            } else {
                [0.0, 0.0, 8.0]
            },
        )
    });
    let batch = select_actor_presentations_for_shadow_view(
        999,
        false,
        None,
        remotes,
        Some(views.main),
        Some(views.publication),
    );
    assert_eq!(
        batch.submissions.len(),
        render_model::MAX_RENDERED_PLAYERS + 1
    );
    assert_eq!(
        batch.submissions[0].input.identity.runtime_id,
        render_model::MAX_RENDERED_PLAYERS as u64 + 1
    );
    assert!(
        batch
            .submissions
            .iter()
            .any(|submission| submission.input.identity.runtime_id
                == render_model::MAX_RENDERED_PLAYERS as u64 + 1)
    );
}

/// Builds a minimal rig fixture without authored skin geometry.
fn rig() -> ActorRigSnapshot<'static> {
    ActorRigSnapshot {
        actor: client_world::ActorLifetimeId {
            session_id: 1,
            dimension: 0,
            runtime_id: 1,
            spawn_revision: 1,
        },
        rig: client_world::EntityRigId(0),
        previous: &[],
        current: &[],
        rest: &[],
        rest_completed_tick: 1,
        rest_reset_generation: 1,
        completed_tick: 1,
        reset_generation: 1,
        fallback: assets::EntityRigFallback::Skip,
        scale: 1.0,
        axis_scale: [1.0; 3],
        previous_body_yaw: 0.0,
        body_yaw: 0.0,
        render: &[],
        bone_names: &[],
        skin_geometry: None,
        skin_mesh: None,
        skin_layers: &[],
        hand: [client_world::HandPhase::default(); 2],
        item_animation: [client_world::ItemAnimationState::default(); 2],
        off_hand_animation: [client_world::ItemAnimationState::default(); 2],
        animation_variables: client_world::ActorAnimationVariables::default(),
        java: client_world::JavaMotion::default(),
        java_equipped: None,
    }
}

#[test]
fn voxel_hidden_actor_can_cast_without_bypassing_vanilla_occlusion() {
    let pose = client_world::ActorPose {
        position: [0.0, 0.0, -8.0],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
    };
    let mut world = client_world::WorldAuthority::new(
        protocol::WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 999,
            local_player_unique_id: 999,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        },
        Arc::new(assets::RuntimeAssets::diagnostic()),
        None,
        [0.0; 3],
        None,
    );
    world
        .apply_ordered_event(
            protocol::WorldEvent::Actor(protocol::ActorEvent::Spawn(protocol::ActorSpawnEvent {
                dimension: 0,
                unique_id: 1,
                runtime_id: 1,
                kind: protocol::ActorKind::Entity {
                    identifier: "minecraft:pig".into(),
                },
                position: pose.position,
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                body_yaw: 0.0,
                held_item: Default::default(),
                metadata: Arc::from([]),
                attributes: Arc::from([]),
                properties: Arc::from([]),
                links: Arc::from([]),
            })),
            Some(1),
        )
        .unwrap();
    let actor = world.actor(1).unwrap();
    assert!(!rig_may_be_published(
        &rig(),
        actor,
        0.0,
        Some(views(false)),
        |_, _| true
    ));
    assert!(rig_may_be_published(
        &rig(),
        actor,
        0.0,
        Some(views(true)),
        |_, _| { panic!("sun caster admission must not query main-camera voxel visibility") }
    ));
}

#[test]
fn local_shadow_body_and_equipment_stay_out_of_main_draws() {
    let local = shadow_only_local(remote(1, [0.0; 3]));
    assert_eq!(local.submission.input.rig, render_model::DIAGNOSTIC_RIG_ID);
    let mut batch = select_actor_presentations_for_shadow_view(
        1,
        false,
        Some(local),
        [remote(2, [0.0, 0.0, -8.0])],
        Some(views(true).main),
        Some(views(true).publication),
    );
    let mut equipment = batch
        .submissions
        .iter()
        .find(|submission| submission.input.identity.runtime_id == 1)
        .unwrap()
        .clone();
    equipment.route = render::ActorRigRoute::Compiled;
    equipment.input.identity.layer = 1;
    batch.submissions.push(equipment);
    shadow_only_caster_layers(&mut batch);
    assert!(
        batch
            .submissions
            .iter()
            .filter(|submission| submission.input.identity.runtime_id == 1)
            .all(|submission| submission.route == render::ActorRigRoute::ShadowOnly)
    );
    assert_eq!(
        batch
            .submissions
            .iter()
            .find(|submission| submission.input.identity.runtime_id == 2)
            .unwrap()
            .route,
        render::ActorRigRoute::Diagnostic
    );
}

#[test]
fn shadow_caster_layers_keep_visible_actors_and_nondrawing_layers_unchanged() {
    let mut batch = crate::presentation::actors::ActorPresentationBatch {
        submissions: vec![
            shadow_only_local(remote(1, [0.0; 3])).submission,
            remote(2, [0.0; 3]).submission,
        ],
        skin_layers: Vec::new(),
        artwork: Default::default(),
    };
    let mut layer = batch.submissions[0].clone();
    layer.input.identity.layer = 1;
    layer.route = render::ActorRigRoute::Compiled;
    batch.submissions.push(layer.clone());
    layer.input.identity.layer = 2;
    layer.route = render::ActorRigRoute::NoDraw;
    batch.submissions.push(layer);
    shadow_only_caster_layers(&mut batch);
    assert_eq!(
        batch.submissions[1].route,
        render::ActorRigRoute::Diagnostic
    );
    assert_eq!(
        batch.submissions[2].route,
        render::ActorRigRoute::ShadowOnly
    );
    assert_eq!(batch.submissions[3].route, render::ActorRigRoute::NoDraw);
}
