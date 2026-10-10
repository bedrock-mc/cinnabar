//! Scene-stack regressions through the real animated hand publication system.
use crate::player_runtime::PlayerRuntime;
use std::sync::Arc;

use bevy::prelude::*;
use client_ui::ui_runtime::{UiRuntime, presentation::forms::ServerUiPack};
use protocol::{
    ActorEvent, ActorKind, ActorSpawnEvent, PlayerListEntry, PlayerListUpdateEvent, PlayerSkin,
    StandardSkin, WorldBootstrap, WorldEvent,
};
use {
    crate::{
        menu::MenuRuntime,
        runtime::{network::prepare_actor_render_frame, world::ClientWorld},
    },
    client_presentation::actor_publication::HandRigBuilder,
    client_ui::test_support::mini_engine_presentation,
    launcher::menu::{MenuAction, MenuScreen},
};

mod hud_visibility;

/// Gives the local player a known rig and skin without any server connection.
fn player_world(entities: Arc<assets::RuntimeEntityAssets>) -> ClientWorld {
    let assets = Arc::new(assets::RuntimeAssets::diagnostic());
    let mut stream = chunk_pipeline::WorldStream::new_with_asset_sets(
        WorldBootstrap {
            local_player_unique_id: 1,
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0., 64., 0.],
            world_spawn_position: [0, 64, 0],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        assets.clone(),
        entities.clone(),
        [0., 64., 0.],
        None,
    );
    stream
        .submit(
            1,
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: 1,
                runtime_id: 1,
                kind: ActorKind::Player {
                    uuid: [1; 16],
                    username: "Tester".into(),
                },
                position: [0., 64., 0.],
                velocity: [0.; 3],
                pitch: 0.,
                yaw: 0.,
                head_yaw: 0.,
                body_yaw: 0.,
                held_item: Default::default(),
                metadata: Arc::from([]),
                attributes: Arc::from([]),
                properties: Arc::from([]),
                links: Arc::from([]),
            })),
        )
        .unwrap();
    stream
        .submit(
            2,
            WorldEvent::Actor(ActorEvent::PlayerList(PlayerListUpdateEvent {
                entries: vec![PlayerListEntry::Add {
                    uuid: [1; 16],
                    unique_id: 1,
                    username: "Tester".into(),
                    verified: true,
                    skin: PlayerSkin::Standard(StandardSkin {
                        geometry: None,
                        cape: None,
                        width: render_model::STANDARD_SKIN_SIDE as u32,
                        height: render_model::STANDARD_SKIN_SIDE as u32,
                        rgba8: vec![255; render_model::STANDARD_SKIN_BYTES].into(),
                    }),
                }]
                .into(),
            })),
        )
        .unwrap();
    stream.advance_actor_interpolation_frame(1);
    assert!(
        stream.authority().actor_rig(1).is_some(),
        "fixture has a player rig"
    );
    let mut world = ClientWorld::new_with_entity_assets(assets, entities);
    world.stream = Some(stream);
    world
}

#[test]
fn menu_input_leak_animated_hand_obeys_pack_visibility_and_restores_after_settings() {
    let (_pack, _geometry, entities) = super::viewmodel_presentation::hand_fixture();
    let scene =
        render::ActorRenderScene::with_runtime_entity_assets_and_equipment(&entities, &[]).unwrap();
    let icons = Arc::new(
        assets::RuntimeIconCatalog::decode(
            &assets::encode_icon_catalog([0; 32], &[], &[]).unwrap(),
        )
        .unwrap(),
    );
    let (equipment, artwork, _) =
        client_presentation::presentation::equipment::EquipmentRuntime::build(
            entities.clone(),
            None,
            icons,
            None,
            None,
            render::ActorArtworkPages::default(),
        );
    let mut world = super::actor_frame_allocations::actor_frame_world(
        player_world(entities.clone()),
        scene,
        artwork,
        HandRigBuilder::from_runtime_assets(&entities).unwrap(),
        (Vec3::new(0., 66., -2.), Vec3::new(0., 66., 0.)),
    );
    world.insert_resource(equipment);
    world.insert_resource(PlayerRuntime::new(1));
    world.insert_resource(UiRuntime::new(1));
    world.insert_resource(mini_engine_presentation());
    world.insert_resource(MenuRuntime::new(false, 2, "Tester".into()));
    world
        .run_system_cached(crate::runtime::network::advance_actor_frame)
        .unwrap();
    world.run_system_cached(prepare_actor_render_frame).unwrap();
    assert!(
        world.resource::<render::HandRigScene>().is_active(),
        "HUD submits the hand"
    );
    world.resource_mut::<MenuRuntime>().open_pause();
    world
        .run_system_cached(crate::runtime::network::advance_actor_frame)
        .unwrap();
    world.run_system_cached(prepare_actor_render_frame).unwrap();
    assert!(
        world.resource::<render::HandRigScene>().is_active(),
        "pause keeps the hand"
    );
    let opaque_pause =
        br#"{"namespace":"pause","pause_screen":{"type":"screen","render_game_behind":false}}"#;
    world
        .resource_mut::<client_ui::ui_runtime::presentation::UiPresentationRuntime>()
        .set_server_ui_pack(&ServerUiPack {
            ui_layers: vec![vec![
                (
                    "ui/_ui_defs.json".into(),
                    br#"{"ui_defs":["ui/pause_screen.json"]}"#.to_vec(),
                ),
                ("ui/pause_screen.json".into(), opaque_pause.to_vec()),
            ]],
            ..Default::default()
        });
    world
        .run_system_cached(crate::runtime::network::advance_actor_frame)
        .unwrap();
    world.run_system_cached(prepare_actor_render_frame).unwrap();
    assert!(
        !world.resource::<render::HandRigScene>().is_active(),
        "opaque pack screen clears the hand"
    );
    world
        .resource_mut::<client_ui::ui_runtime::presentation::UiPresentationRuntime>()
        .set_server_ui_pack(&ServerUiPack::default());
    world
        .run_system_cached(crate::runtime::network::advance_actor_frame)
        .unwrap();
    world.run_system_cached(prepare_actor_render_frame).unwrap();
    assert!(
        world.resource::<render::HandRigScene>().is_active(),
        "pause restores the hand"
    );
    world
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::Navigate(MenuScreen::Settings));
    world
        .run_system_cached(crate::runtime::network::advance_actor_frame)
        .unwrap();
    world.run_system_cached(prepare_actor_render_frame).unwrap();
    assert!(
        world.resource::<render::HandRigScene>().is_active(),
        "Settings opened from pause retains the live world hand"
    );
    world.resource_mut::<MenuRuntime>().set_visible(false);
    world
        .run_system_cached(crate::runtime::network::advance_actor_frame)
        .unwrap();
    world.run_system_cached(prepare_actor_render_frame).unwrap();
    assert!(
        world.resource::<render::HandRigScene>().is_active(),
        "closing Settings restores it"
    );
}
