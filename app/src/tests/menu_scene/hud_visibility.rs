//! HUD visibility through both first-person hand publishers and retained name tags.
use bevy::window::{PrimaryWindow, Window};
use client_ui::ui_runtime::presentation::{PreparedUiPublication, UiPresentationRuntime};
use {
    super::*, client_presentation::actor_publication::HandRigBuilder,
    client_ui::test_support::mini_engine_presentation,
};
use {
    crate::{
        environment::{WeatherState, WorldClock},
        item_use::ItemUseRuntime,
        movement::MovementTicker,
        runtime::{
            network::NetworkHandle, visibility::CaveVisibilityCache, world::WorldStreamFramePoll,
        },
        ui_runtime::presentation::prepare_ui_runtime,
    },
    client_presentation::{camera::FlyCamera, local_player::LocalPlayerFrameCarrier},
};

/// Builds a renderer-free world with gameplay overlays available.
fn fixture_world() -> World {
    let (_pack, _geometry, entities) = super::super::viewmodel_presentation::hand_fixture();
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
    let mut client = player_world(entities.clone());
    let stream = client.stream.as_mut().unwrap();
    stream
        .submit(
            3,
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: 2,
                runtime_id: 2,
                kind: ActorKind::Player {
                    uuid: [2; 16],
                    username: "A".into(),
                },
                position: [0., 64., 2.],
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
    stream.advance_actor_interpolation_frame(2);
    let mut world = super::super::actor_frame_allocations::actor_frame_world(
        client,
        scene,
        artwork,
        HandRigBuilder::from_runtime_assets(&entities).unwrap(),
        (Vec3::new(0., 66., -2.), Vec3::new(0., 66., 0.)),
    );
    world.insert_resource(equipment);
    world.insert_resource(UiRuntime::new(1));
    world.insert_resource(mini_engine_presentation());
    world.insert_resource(MenuRuntime::new(false, 2, "Tester".into()));
    world.insert_resource(NetworkHandle::stub().0);
    world.init_resource::<PreparedUiPublication>();
    world.init_resource::<CaveVisibilityCache>();
    world.init_resource::<render::VisibilityDiagnosticsInput>();
    world.init_resource::<render::VisibilityDiagnostics>();
    world.init_resource::<render::ChunkRenderQueue>();
    world.init_resource::<render::ChunkUploadAcknowledgements>();
    world.init_resource::<WorldStreamFramePoll>();
    world.init_resource::<LocalPlayerFrameCarrier>();
    world.init_resource::<WorldClock>();
    world.init_resource::<WeatherState>();
    world.init_resource::<ItemUseRuntime>();
    world.init_resource::<MovementTicker>();
    world.spawn((Window::default(), PrimaryWindow));
    let camera = world
        .query_filtered::<Entity, With<FlyCamera>>()
        .single(&world)
        .unwrap();
    world
        .entity_mut(camera)
        .insert((Camera::default(), Camera3d::default()));
    world
}

/// Checks the published hand and label state after one visibility update.
fn assert_published(world: &mut World, hands: bool, names: bool) {
    // An inactive animated rig selects the real CPU-hand capture path.
    world.resource_mut::<render::HandRigScene>().clear();
    world.run_system_cached(prepare_ui_runtime).unwrap();
    let pending = world
        .resource::<PreparedUiPublication>()
        .0
        .as_ref()
        .unwrap();
    assert_eq!(pending.preview.hands, hands, "CPU hand capture");
    assert_eq!(
        world
            .resource_mut::<UiPresentationRuntime>()
            .hud_frame_mut()
            .first_person,
        hands,
        "HUD hand ownership"
    );
    assert_eq!(
        !world
            .resource_mut::<UiPresentationRuntime>()
            .nametag_scene()
            .records
            .is_empty(),
        names,
        "retained nametag publication"
    );
    // Terrain startup is unrelated to the visibility option under test.
    world
        .resource_mut::<UiPresentationRuntime>()
        .set_loading_stage(None);
    world
        .run_system_cached(crate::runtime::network::advance_actor_frame)
        .unwrap();
    world.run_system_cached(prepare_actor_render_frame).unwrap();
    assert_eq!(
        world.resource::<render::HandRigScene>().is_active(),
        hands,
        "animated hand publication"
    );
}

#[test]
fn hide_hud_clears_published_hands_and_nametags_and_restores_the_hand_preference() {
    let mut world = fixture_world();
    let mut prior_hand = None;
    for (hide_hud, hide_hand, hands, names) in [
        (false, false, true, true),
        (true, false, false, false),
        (false, false, true, true),
        (false, true, false, true),
        (true, true, false, false),
        (false, true, false, true),
        (false, false, true, true),
    ] {
        {
            let mut menu = world.resource_mut::<MenuRuntime>();
            if prior_hand != Some(hide_hand) {
                menu.set_session_option("hide_hand", Some(i32::from(hide_hand)));
                prior_hand = Some(hide_hand);
            }
            menu.set_session_option("hide_hud", Some(i32::from(hide_hud)));
        }
        assert_published(&mut world, hands, names);
        assert_eq!(
            world
                .resource::<MenuRuntime>()
                .settings_snapshot()
                .0
                .value("hide_hand"),
            i32::from(hide_hand),
            "HUD visibility must preserve the independent hand preference"
        );
    }
}
