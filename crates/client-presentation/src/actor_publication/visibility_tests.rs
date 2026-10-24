use super::JumpPhysics;
use crate::presentation::actors::{local_diagnostic_presentation, select_actor_presentations};
use protocol_ids::{ActorDataKey, ActorFlag};

#[test]
fn invisibility_retains_only_layers_with_an_emissive_eye_mask() {
    let mut stream = chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    });
    let physics = JumpPhysics(
        sim::PlayerState::new(sim::Vec3::ZERO),
        sim::MovementMode::Walking,
    );
    let feed = crate::actor_feed::build_local_player_feed(
        &physics,
        bevy::math::Quat::IDENTITY,
        false,
        true,
        [1; 16],
        || protocol::PlayerSkin::Unavailable(protocol::PlayerSkinUnavailable::InvalidDimensions),
        client_world::LocalItemUse::Unpredicted,
    )
    .unwrap();
    stream.sync_local_player_pose(&feed);
    stream
        .submit(
            1,
            protocol::WorldEvent::Actor(protocol::ActorEvent::Metadata(
                protocol::ActorMetadataUpdateEvent {
                    dimension: 0,
                    runtime_id: 1,
                    metadata: [protocol::ActorMetadata {
                        key: ActorDataKey::Flags.wire_value(),
                        value: protocol::ActorMetadataValue::Flags(
                            1 << ActorFlag::Invisible.wire_value(),
                        ),
                    }]
                    .into(),
                    properties: [].into(),
                    tick: 0,
                },
            )),
        )
        .unwrap();
    assert!(stream.authority().actor(1).unwrap().is_invisible());
    for (alpha_test, emissive, only_emissive, retained) in [
        (true, false, false, false),
        (true, true, false, false),
        (true, false, true, false),
        (false, true, true, false),
        (true, true, true, true),
    ] {
        let body = local_diagnostic_presentation(
            1,
            0,
            1,
            render_api::PoseGeneration::new(1),
            [0.0; 3],
            0.0,
            0.0,
        )
        .unwrap();
        let mut batch = select_actor_presentations(1, true, Some(body), []);
        let original = batch.submissions[0].route;
        batch.submissions[0].material.state = Some(assets::EntityRenderMaterialState {
            alpha_test,
            emissive,
            only_emissive,
            ..Default::default()
        });
        super::super::layers::apply_style_and_visibility(&mut batch, Some(&stream), None);
        assert_eq!(
            batch.submissions[0].route,
            if retained {
                original
            } else {
                render_model::ActorRigRoute::NoDraw
            }
        );
    }
}
