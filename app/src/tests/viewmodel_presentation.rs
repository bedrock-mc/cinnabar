use crate::player_runtime::PlayerRuntime;
use client_ui::ui_runtime::{
    UiRuntime,
    inventory_router::{EquipmentRoute, EquipmentRouteResult},
};
use protocol::{
    ActorHandedness, ContainerIdentity, EquipmentEvent, InventoryContentEvent, InventoryEvent,
    InventorySlotEvent, NetworkItemStack, SlotIdentity,
};

fn assert_cube_scene(app: &bevy::prelude::App, expected: bool) {
    assert_eq!(
        app.world()
            .resource::<render::ViewmodelScene>()
            .is_opaque_cube(),
        expected
    );
}
fn container(window: i32) -> ContainerIdentity {
    ContainerIdentity {
        window_id: Some(window),
        slot_type: None,
        dynamic_id: None,
    }
}
fn equipment(actor: u64, stack: NetworkItemStack) -> EquipmentEvent {
    EquipmentEvent {
        actor_runtime_id: actor,
        stack,
        inventory_slot: 0,
        selected_slot: 0,
        window_id: 119,
        handedness: Some(ActorHandedness::Left),
    }
}
fn route(
    player_runtime: &mut PlayerRuntime,
    runtime: &mut UiRuntime,
    sequence: u64,
    event: EquipmentEvent,
) {
    let result = runtime
        .route_equipment(player_runtime, runtime.session_id(), sequence, event)
        .unwrap();
    if let EquipmentRouteResult::Routed(EquipmentRoute::LocalSelected {
        fifo_sequence,
        event,
    }) = result
    {
        runtime.retain_local_selected_equipment(player_runtime, fifo_sequence, event);
    }
}

#[test]
fn offhand_empty_provider_preserves_unknown_present_and_actual_authority_routes() {
    let mut player_runtime = PlayerRuntime::new(1);

    let mut runtime = UiRuntime::new(1);
    runtime
        .publish_local_runtime_id(&mut player_runtime, 1, 7)
        .unwrap();
    assert_eq!(runtime.gameplay_hud().offhand_is_empty(), None);
    route(
        &mut player_runtime,
        &mut runtime,
        1,
        equipment(8, NetworkItemStack::empty()),
    );
    assert_eq!(runtime.gameplay_hud().offhand_is_empty(), None);
    let mut present = NetworkItemStack::empty();
    present.count = 1;
    present.network_id = 1;
    route(&mut player_runtime, &mut runtime, 2, equipment(7, present));
    assert_eq!(runtime.gameplay_hud().offhand_is_empty(), Some(false));
    assert!(runtime.gameplay_hud().offhand_stack().is_some());
    route(
        &mut player_runtime,
        &mut runtime,
        3,
        equipment(7, NetworkItemStack::empty()),
    );
    assert_eq!(runtime.gameplay_hud().offhand_is_empty(), Some(true));
    assert!(runtime.gameplay_hud().offhand_stack().is_none());
    crate::session::begin_session(&mut runtime, &mut player_runtime, 2);
    assert_eq!(runtime.gameplay_hud().offhand_is_empty(), None);
    runtime
        .enqueue_inventory_event(
            &mut player_runtime,
            2,
            1,
            InventoryEvent::Content(InventoryContentEvent {
                container: container(119),
                slots: vec![NetworkItemStack::empty()].into(),
                storage_item: NetworkItemStack::empty(),
            }),
        )
        .unwrap();
    runtime.drain_pending_inventory(&mut player_runtime);
    assert_eq!(runtime.gameplay_hud().offhand_is_empty(), Some(true));
    runtime
        .enqueue_inventory_event(
            &mut player_runtime,
            2,
            2,
            InventoryEvent::Slot(InventorySlotEvent {
                identity: SlotIdentity {
                    container: container(119),
                    slot: 999,
                },
                stack: {
                    let mut stack = NetworkItemStack::empty();
                    stack.network_id = 2;
                    stack.count = 1;
                    stack
                },
                storage_item: None,
            }),
        )
        .unwrap();
    runtime.drain_pending_inventory(&mut player_runtime);
    assert_eq!(runtime.gameplay_hud().offhand_is_empty(), Some(true));
    crate::session::begin_session(&mut runtime, &mut player_runtime, 3);
    assert_eq!(runtime.gameplay_hud().offhand_is_empty(), None);
}

#[test]
fn cpu_hand_quad_is_retained_alongside_unchanged_held_items() {
    let player_runtime = PlayerRuntime::new(1);

    use crate::ui_runtime::presentation::tests::fixture_font;
    use client_ui::ui_runtime::presentation::{UiPresentationRuntime, refresh_hud_frame};
    use ui::IconRef;
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let pixels = vec![255; 64 * 64 * 4];
    presentation.set_player_preview_skin(Some(&pixels), Default::default());
    let mut runtime = UiRuntime::new(1);
    let settings = crate::camera::CameraSettingsAuthority::default();
    refresh_hud_frame(
        &player_runtime,
        &mut runtime,
        &mut presentation,
        None,
        settings.perspective(),
        0,
    );
    let right = presentation.hud_frame().right_hand;
    assert!(right.is_some());
    let main_item = IconRef {
        page: 1,
        uv: [0, 0, 16, 16],
        glint: false,
    };
    let offhand = IconRef {
        page: 2,
        uv: [0, 0, 16, 16],
        glint: false,
    };
    presentation.hud_frame_mut().held_item_icon = Some(main_item);
    presentation.hud_frame_mut().offhand_viewmodel_icon = Some(offhand);
    assert_eq!(presentation.cpu_empty_hand_fallback(), right);
    assert_eq!(presentation.hud_frame().right_hand, right);
    assert_eq!(presentation.hud_frame().held_item_icon, Some(main_item));
    assert_eq!(
        presentation.hud_frame().offhand_viewmodel_icon,
        Some(offhand)
    );
    refresh_hud_frame(
        &player_runtime,
        &mut runtime,
        &mut presentation,
        None,
        settings.perspective(),
        1,
    );
    assert_eq!(presentation.hud_frame().right_hand, right);
}

// The GPU first-person rig owns the hand: the HUD's CPU hand and held-item quads must not also
// draw in the screen corner.
#[test]
fn active_hand_rig_retires_the_cpu_hand_and_item_quads() {
    let player_runtime = PlayerRuntime::new(1);

    use crate::ui_runtime::presentation::tests::{fixture_font, fixture_hud};
    use client_ui::ui_runtime::presentation::{UiPresentationRuntime, refresh_hud_frame};
    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    presentation.set_player_preview_skin(Some(&vec![255; 64 * 64 * 4]), Default::default());
    let mut runtime = UiRuntime::new(1);
    let settings = crate::camera::CameraSettingsAuthority::default();
    refresh_hud_frame(
        &player_runtime,
        &mut runtime,
        &mut presentation,
        None,
        settings.perspective(),
        0,
    );
    let hand = presentation.hud_frame().right_hand.expect("hand carrier");
    let frame = presentation.hud_frame_mut();
    frame.first_person = true;
    frame.held_item_icon = Some(hand);
    let carriers = |presentation: &mut UiPresentationRuntime| {
        let input = presentation
            .build(
                &player_runtime,
                &runtime,
                0,
                [1280, 720],
                ui::DpiScale::new(1.).unwrap(),
            )
            .unwrap();
        let corner = [hand.uv[0], hand.uv[1]];
        let mut vertices = input
            .batches
            .iter()
            .filter(|batch| batch.texture_page == u32::from(hand.page))
            .flat_map(|batch| {
                let start = batch.first_index as usize;
                input.indices[start..start + batch.index_count as usize].to_vec()
            })
            .filter(|&index| input.vertices[index as usize].uv == corner.map(f32::from))
            .collect::<Vec<_>>();
        vertices.sort_unstable();
        vertices.dedup();
        vertices.len()
    };
    assert_eq!(carriers(&mut presentation), 1, "a held item hides the arm");
    presentation.hud_frame_mut().held_item_icon = None;
    assert_eq!(
        carriers(&mut presentation),
        1,
        "the empty hand shows the arm"
    );
    presentation.hud_frame_mut().held_item_icon = Some(hand);
    presentation.hud_frame_mut().hand_rig_active = true;
    assert_eq!(carriers(&mut presentation), 0);
}

pub(super) struct FixturePack(std::path::PathBuf);
impl FixturePack {
    fn write(&self, path: &str, value: &[u8]) {
        let path = self.0.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, value).unwrap();
    }
}
impl Drop for FixturePack {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
/// Compiles a small player rig with an arm and sleeve for hand-publication tests.
pub(super) fn hand_fixture() -> (
    FixturePack,
    render::ViewmodelGeometry,
    std::sync::Arc<assets::RuntimeEntityAssets>,
) {
    use assets::{RuntimeActorCatalog, RuntimeEntityAssets, encode_entity_blob};
    let root = std::env::temp_dir().join(format!(
        "neutral-hand-fixture-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    let pack = FixturePack(root);
    for directory in ["animations", "animation_controllers"] {
        std::fs::create_dir(pack.0.join(directory)).unwrap();
    }
    pack.write("entity/player.json", br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:player","geometry":{"default":"geometry.humanoid.custom"},"materials":{"default":"entity_alphatest"},"textures":{"default":"textures/entity/test"},"render_controllers":["controller.render.test"]}}}"#);
    let mut bones = Vec::new();
    for (name, parent, pivot) in [
        ("root", None, [0, 0, 0]),
        ("waist", Some("root"), [0, 12, 0]),
        ("body", Some("waist"), [0, 24, 0]),
        ("rightArm", Some("body"), [-5, 22, 0]),
        ("rightSleeve", Some("rightArm"), [-5, 22, 0]),
    ] {
        let mut bone = serde_json::json!({"name":name,"pivot":pivot});
        if let Some(parent) = parent {
            bone["parent"] = parent.into();
        }
        if matches!(name, "rightArm" | "rightSleeve") {
            bone["cubes"] = serde_json::json!([{"origin":[-8,12,-2],"size":[4,12,4],
                "uv":[40,if name == "rightArm" {16} else {32}], "inflate":if name == "rightArm" {0.0} else {0.25}}]);
        }
        bones.push(bone);
    }
    pack.write("models/entity/test.json", &serde_json::to_vec(&serde_json::json!({"format_version":"1.12.0",
        "minecraft:geometry":[{"description":{"identifier":"geometry.humanoid.custom","texture_width":64,"texture_height":64},"bones":bones}]})).unwrap());
    pack.write("render_controllers/test.json", br#"{"format_version":"1.8.0","render_controllers":{"controller.render.test":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}"#);
    std::fs::create_dir_all(pack.0.join("textures/entity")).unwrap();
    image::RgbaImage::from_pixel(64, 64, image::Rgba([20, 40, 60, 255]))
        .save(pack.0.join("textures/entity/test.png"))
        .unwrap();
    let manifest = include_bytes!("../../../assets/vanilla-source.json");
    let compiled = pack_compiler::compile_entity_assets(&pack.0, manifest).unwrap();
    let bytes = encode_entity_blob(&compiled).unwrap();
    let entities = std::sync::Arc::new(RuntimeEntityAssets::decode(&bytes).unwrap());
    let actor = pack_compiler::compile_actor_assets(&pack.0, manifest).unwrap();
    let catalog = RuntimeActorCatalog::decode(&actor.bytes, &entities).unwrap();
    let artwork = render::ActorArtworkPages::new(&catalog);
    let geometry = render::ViewmodelGeometry::from_runtime(&entities, &artwork).unwrap();
    (pack, geometry, entities)
}

#[test]
fn menu_input_leak_real_producer_to_hand_adapter_keeps_cpu_until_completion_and_clears_on_unknown_or_held()
 {
    let mut player_runtime = PlayerRuntime::new(1);

    use crate::{
        camera::FlyCamera,
        presentation::viewmodel::{HandAdapter, HandFallback, ViewmodelPublish},
        runtime::world::ClientWorld,
    };
    use bevy::{
        camera::{Camera, ComputedCameraValues, RenderTarget, RenderTargetInfo},
        ecs::system::RunSystemOnce,
        prelude::*,
    };
    use protocol::{
        ActorEvent, ActorKind, ActorSpawnEvent, PlayerListEntry, PlayerListUpdateEvent, PlayerSkin,
        StandardSkin, WorldBootstrap, WorldEvent,
    };
    use std::sync::Arc;
    let (_pack, geometry, entities) = hand_fixture();
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
                    username: "test".into(),
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
                    username: "test".into(),
                    verified: true,
                    skin: PlayerSkin::Standard(StandardSkin {
                        geometry: None,
                        cape: None,
                        width: 64,
                        height: 64,
                        rgba8: vec![255; 64 * 64 * 4].into(),
                    }),
                }]
                .into(),
            })),
        )
        .unwrap();
    stream.prepare_actor_appearance_fixture();
    stream.advance_actor_interpolation_ticks(1);
    let mut world = ClientWorld::new_with_entity_assets(assets, entities);
    world.stream = Some(stream);
    let mut runtime = UiRuntime::new(1);
    runtime
        .publish_local_runtime_id(&mut player_runtime, 1, 1)
        .unwrap();
    runtime.retain_local_selected_equipment(
        &mut player_runtime,
        1,
        EquipmentEvent {
            actor_runtime_id: 1,
            stack: NetworkItemStack::empty(),
            inventory_slot: 0,
            selected_slot: 0,
            window_id: 0,
            handedness: Some(ActorHandedness::Right),
        },
    );
    let mut app = App::new();
    app.insert_resource(runtime)
        .insert_resource(player_runtime.clone())
        .insert_resource(world)
        .insert_resource(geometry)
        .init_resource::<HandAdapter>()
        .init_resource::<render::ViewmodelScene>()
        .init_resource::<render::ViewmodelCompletionGate>();
    app.world_mut().spawn((
        FlyCamera::default(),
        Camera {
            computed: ComputedCameraValues {
                target_info: Some(RenderTargetInfo {
                    physical_size: UVec2::new(640, 480),
                    scale_factor: 1.0,
                }),
                ..Default::default()
            },
            ..Default::default()
        },
        RenderTarget::default(),
        Msaa::Off,
    ));
    let observe = |mut hand: ViewmodelPublish,
                   player_runtime: Res<PlayerRuntime>,
                   runtime: Res<UiRuntime>,
                   world: Res<ClientWorld>| {
        assert!(!hand.observe(&player_runtime, &runtime, &world, true, false, [640, 480]));
    };
    app.world_mut().run_system_once(observe).unwrap();
    assert_eq!(
        app.world().resource::<HandAdapter>().stats.fallback,
        Some(HandFallback::ItemsUnknownOrHeld)
    );
    crate::tests::with_ui_player(&mut app, |runtime, player_runtime| {
        runtime.retain_local_selected_equipment(
            player_runtime,
            2,
            equipment(1, NetworkItemStack::empty()),
        );
    });
    app.world_mut().run_system_once(observe).unwrap();
    assert_eq!(
        app.world().resource::<HandAdapter>().stats.mode,
        Some(render::ViewmodelMode::EmptyHandNeutralStaticFallback)
    );
    assert_eq!(
        app.world().resource::<HandAdapter>().stats.skin_validations,
        1
    );
    let mut menu = crate::menu::MenuRuntime::new(false, 2, "Tester".into());
    menu.open_pause();
    app.insert_resource(menu);
    app.world_mut().run_system_once(observe).unwrap();
    assert!(app.world().resource::<HandAdapter>().stats.mode.is_some());
    app.world_mut()
        .resource_mut::<crate::menu::MenuRuntime>()
        .activate(crate::menu::MenuAction::PauseSettings);
    app.world_mut().run_system_once(observe).unwrap();
    assert!(app.world().resource::<HandAdapter>().stats.mode.is_some());
    app.world_mut()
        .resource_mut::<crate::menu::MenuRuntime>()
        .activate(crate::menu::MenuAction::Navigate(
            crate::menu::MenuScreen::Home,
        ));
    app.world_mut().run_system_once(observe).unwrap();
    assert!(app.world().resource::<HandAdapter>().stats.mode.is_none());
    assert_eq!(
        app.world().resource::<HandAdapter>().stats.fallback,
        Some(HandFallback::Hidden)
    );
    app.world_mut()
        .resource_mut::<crate::menu::MenuRuntime>()
        .set_visible(false);
    app.world_mut()
        .run_system_once(
            |mut hand: ViewmodelPublish,
             player_runtime: Res<PlayerRuntime>,
             runtime: Res<UiRuntime>,
             world: Res<ClientWorld>| {
                assert!(!hand.observe(&player_runtime, &runtime, &world, false, false, [640, 480]));
            },
        )
        .unwrap();
    assert_eq!(
        app.world().resource::<HandAdapter>().stats.fallback,
        Some(HandFallback::Hidden)
    );
    app.world_mut()
        .run_system_once(
            |mut hand: ViewmodelPublish,
             player_runtime: Res<PlayerRuntime>,
             runtime: Res<UiRuntime>,
             world: Res<ClientWorld>| {
                assert!(!hand.observe(&player_runtime, &runtime, &world, true, true, [640, 480]));
            },
        )
        .unwrap();
    assert_eq!(app.world().resource::<HandAdapter>().stats.mode, None);
    app.world_mut()
        .run_system_once(
            |mut hand: ViewmodelPublish,
             player_runtime: Res<PlayerRuntime>,
             runtime: Res<UiRuntime>,
             world: Res<ClientWorld>| {
                assert!(!hand.observe(&player_runtime, &runtime, &world, true, false, [800, 480]));
            },
        )
        .unwrap();
    assert_eq!(
        app.world().resource::<HandAdapter>().stats.fallback,
        Some(HandFallback::View)
    );
    app.world_mut().run_system_once(observe).unwrap();
    assert_eq!(
        app.world().resource::<HandAdapter>().stats.skin_validations,
        1
    );
    let mut held = NetworkItemStack::empty();
    held.count = 1;
    held.network_id = 1;
    crate::tests::with_ui_player(&mut app, |runtime, player_runtime| {
        runtime.retain_local_selected_equipment(player_runtime, 3, equipment(1, held));
    });
    app.world_mut().run_system_once(observe).unwrap();
    assert_eq!(
        app.world().resource::<HandAdapter>().stats.fallback,
        Some(HandFallback::ItemsUnknownOrHeld)
    );
    crate::tests::with_ui_player(&mut app, |runtime, player_runtime| {
        crate::session::begin_session(runtime, player_runtime, 2);
    });
    app.world_mut().run_system_once(observe).unwrap();
    assert_eq!(app.world().resource::<HandAdapter>().stats.mode, None);
    assert_eq!(
        app.world().resource::<HandAdapter>().stats.fallback,
        Some(HandFallback::Ownership)
    );
    // Both stacks may be explicitly empty but belong to a different retained
    // UI player identity. They cannot authorize the current stream's hand.
    crate::tests::with_ui_player(&mut app, |runtime, player_runtime| {
        runtime
            .publish_local_runtime_id(player_runtime, 2, 99)
            .unwrap();
        runtime.retain_local_selected_equipment(
            player_runtime,
            1,
            EquipmentEvent {
                actor_runtime_id: 99,
                stack: NetworkItemStack::empty(),
                inventory_slot: 0,
                selected_slot: 0,
                window_id: 0,
                handedness: Some(ActorHandedness::Right),
            },
        );
        runtime.retain_local_selected_equipment(
            player_runtime,
            2,
            equipment(99, NetworkItemStack::empty()),
        );
    });
    app.world_mut().run_system_once(observe).unwrap();
    assert_eq!(
        app.world().resource::<HandAdapter>().stats.fallback,
        Some(HandFallback::Ownership)
    );
    assert_eq!(app.world().resource::<HandAdapter>().stats.mode, None);
    crate::tests::with_ui_player(&mut app, |runtime, player_runtime| {
        crate::session::begin_session(runtime, player_runtime, 3);
        runtime
            .publish_local_runtime_id(player_runtime, 3, 1)
            .unwrap();
        runtime.retain_local_selected_equipment(
            player_runtime,
            1,
            EquipmentEvent {
                actor_runtime_id: 1,
                stack: NetworkItemStack::empty(),
                inventory_slot: 0,
                selected_slot: 0,
                window_id: 0,
                handedness: Some(ActorHandedness::Right),
            },
        );
        runtime.retain_local_selected_equipment(
            player_runtime,
            2,
            equipment(1, NetworkItemStack::empty()),
        );
    });
    app.world_mut().run_system_once(observe).unwrap();
    assert_eq!(
        app.world().resource::<HandAdapter>().stats.mode,
        Some(render::ViewmodelMode::EmptyHandNeutralStaticFallback)
    );
}

#[test]
fn ui_only_headless_hand_adapter_keeps_the_cpu_path_without_gpu_resources() {
    use crate::{presentation::viewmodel::ViewmodelPublish, runtime::world::ClientWorld};
    use bevy::{ecs::system::RunSystemOnce, prelude::*};
    let (_pack, _geometry, entities) = hand_fixture();
    let mut app = App::new();
    app.insert_resource(UiRuntime::new(1))
        .insert_resource(PlayerRuntime::new(1))
        .insert_resource(ClientWorld::new_with_entity_assets(
            std::sync::Arc::new(assets::RuntimeAssets::diagnostic()),
            entities,
        ));
    app.world_mut()
        .run_system_once(
            |mut hand: ViewmodelPublish,
             player_runtime: Res<PlayerRuntime>,
             runtime: Res<UiRuntime>,
             world: Res<ClientWorld>| {
                assert!(!hand.observe(&player_runtime, &runtime, &world, true, false, [640, 480]));
                hand.clear();
            },
        )
        .unwrap();
}

mod cube;
