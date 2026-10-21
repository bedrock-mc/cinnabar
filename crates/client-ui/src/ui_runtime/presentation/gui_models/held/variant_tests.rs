//! Held previews retain selected definition identity, geometry and artwork.
use super::*;
use crate::ui_runtime::presentation::UiPresentationRuntime;
use player_preview::PreviewEquipmentOwner;

/// Compiles original player hand bones and independently recognizable held cubes.
fn fixture(item: &str, mapped: &str) -> assets::SessionEntityPack {
    let mut files=vec![
        ("entity/player.json".into(),br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:player","geometry":{"default":"geometry.player"},"render_controllers":["controller.render.player"]}}}"#.to_vec()),
        ("models/player.json".into(),br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.player","texture_width":64,"texture_height":64},"bones":[{"name":"body","cubes":[{"origin":[-4,0,-2],"size":[8,12,4],"uv":[0,0]}]},{"name":"rightItem","parent":"body","pivot":[-6,12,0]},{"name":"leftItem","parent":"body","pivot":[6,12,0]}]}]}"#.to_vec()),
        ("render_controllers/item.json".into(),br#"{"format_version":"1.8.0","render_controllers":{"controller.render.player":{"geometry":"Geometry.default"},"controller.render.item":{"geometry":"Geometry.default","textures":["Texture.default"],"materials":[{"*":"Material.default"}]}}}"#.to_vec()),
    ];
    let mut textures = Vec::new();
    for (name, identifier, size, color) in [
        ("ordinary", item, 2, [210, 70, 40, 255]),
        ("mapped", mapped, 6, [40, 190, 100, 255]),
    ] {
        let texture = format!("textures/{name}");
        let mut description = serde_json::json!({"identifier":identifier,"materials":{"default":"entity_alphatest"},"textures":{"default":texture},"geometry":{"default":format!("geometry.{name}")},"animations":{"wield_third_person":format!("animation.{name}.wield_third_person")},"scripts":{"animate":["wield_third_person"]},"render_controllers":["controller.render.item"]});
        if name == "mapped" {
            description["item"] = serde_json::json!({(item):"q.owner_identifier == 'minecraft:player' && q.is_in_ui && c.item_slot == 'main_hand'"});
        }
        for (path, json) in [
            (
                format!("attachables/{name}.json"),
                serde_json::json!({"format_version":"1.10.0","minecraft:attachable":{"description":description}}),
            ),
            (
                format!("models/{name}.json"),
                serde_json::json!({
                    "format_version":"1.16.0", "minecraft:geometry":[{
                        "description":{"identifier":format!("geometry.{name}"),"texture_width":32,"texture_height":32},
                        "bones":[{"name":if item == assets::gui_item::SHIELD_IDENTIFIER {assets::gui_item::SHIELD_ROOT_BONE} else {"rightitem"},"cubes":[{"origin":[0,0,0],"size":[size,size,size],"uv":[0,0]}]}]
                    }]
                }),
            ),
            (
                format!("animations/{name}.json"),
                serde_json::json!({"format_version":"1.10.0","animations":{format!("animation.{name}.wield_third_person"): {"loop":true,"bones":{"rightitem":{"position":[2,3,4],"rotation":[0,0,0]}}}}}),
            ),
        ] {
            files.push((path.into(), serde_json::to_vec(&json).unwrap()));
        }
        textures.push(assets::EquipmentTexture {
            identifier: texture.into(),
            width: 32,
            height: 32,
            rgba8: color.repeat(1024).into(),
        });
    }
    let compiled = pack_entity_compiler::compile_entity_pack(files)
        .unwrap()
        .unwrap();
    assets::SessionEntityPack {
        assets: Arc::new(RuntimeEntityAssets::from_compiled(compiled.assets).unwrap()),
        textures: Arc::from([]),
        bindings: Arc::from([]),
        equipment: Some(Arc::new(
            RuntimeEquipmentCatalog::from_parts(
                [1; 32],
                compiled.equipment_bindings.into_vec(),
                textures,
            )
            .unwrap(),
        )),
    }
}

/// Admits a local player through actor authority and resolves its real compiled rig.
fn owner(entities: Arc<assets::RuntimeEntityAssets>) -> chunk_pipeline::WorldStream {
    let mut world = chunk_pipeline::WorldStream::new_with_asset_sets(
        protocol::WorldBootstrap {
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            dimension: 0,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(assets::RuntimeAssets::diagnostic()),
        entities,
        [0.0; 3],
        None,
    );
    world
        .submit(
            1,
            protocol::WorldEvent::Actor(protocol::ActorEvent::Spawn(protocol::ActorSpawnEvent {
                dimension: 0,
                unique_id: 1,
                runtime_id: 1,
                kind: protocol::ActorKind::Player {
                    uuid: [1; 16],
                    username: "Preview fixture".into(),
                },
                position: [0.0; 3],
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
        )
        .unwrap();
    world
}

#[test]
fn selection_consumers_held_previews_draw_the_selected_definition_in_both_orders() {
    use crate::test_support::fixture_hud;
    use ui_engine::test_support::fixture_font;
    for (item, mapped) in [
        ("fixture:z_tool", "fixture:a_tool"),
        ("fixture:a_tool", "fixture:z_tool"),
    ] {
        for session in [false, true] {
            let pack = fixture(item, mapped);
            let icons = Arc::new(
                RuntimeIconCatalog::decode(
                    &assets::encode_icon_catalog(
                        pack.assets.source_manifest_sha256(),
                        &[assets::IconSprite {
                            width: 16,
                            height: 16,
                            rgba8: vec![255; 1024].into(),
                        }],
                        &[assets::IconEntry {
                            identifier: item.into(),
                            metadata: 0,
                            sprite: 0,
                        }],
                    )
                    .unwrap(),
                )
                .unwrap(),
            );
            let mut ui =
                UiPresentationRuntime::with_hud_and_icons(fixture_font(), fixture_hud(), icons)
                    .unwrap();
            if !session {
                ui.set_equipment_catalog(pack.equipment.clone());
            }
            ui.set_gui_models(&assets::RuntimeAssets::diagnostic(), &pack.assets)
                .unwrap();
            ui.set_player_preview_skin(Some(&[255; 64 * 64 * 4]), Default::default());
            ui.set_preview_equipment_assets(
                (!session).then(|| Arc::clone(&pack.assets)),
                session.then(|| Arc::clone(&pack.assets)),
            );
            if session {
                ui.set_preview_pack_equipment(pack.equipment.clone());
            }
            let world = owner(Arc::clone(&pack.assets));
            let mut cached = None;
            for admitted in [true, true, false, true] {
                if admitted {
                    ui.set_player_preview_gear_with_owner(
                        [None; 4],
                        Some((item, 0)),
                        PreviewEquipmentOwner {
                            actor: world.authority().actor(1).unwrap(),
                            rig: world.authority().actor_rig(1).unwrap(),
                            input: Default::default(),
                        },
                    );
                } else {
                    ui.set_player_preview_gear([None; 4], Some((item, 0)));
                }
                let name = if admitted { "mapped" } else { "ordinary" };
                let actual = assert_model(&ui, &pack, 0, name);
                if admitted && let Some(previous) = cached.take() {
                    assert!(Arc::ptr_eq(&previous, &actual.vertices));
                }
                cached = admitted.then_some(actual.vertices);
            }
            let runtime = crate::ui_runtime::UiRuntime::new(1);
            let mut player = player_state::PlayerState::new(1);
            player.inventory.set_local_selected_slot(0);
            for window in [
                protocol::PLAYER_INVENTORY_WINDOW_ID,
                protocol::OFFHAND_WINDOW_ID,
            ] {
                player
                    .inventory
                    .ledger_mut()
                    .apply(&protocol::InventoryEvent::Content(
                        protocol::InventoryContentEvent {
                            container: protocol::ContainerIdentity::window(window),
                            slots: Arc::from([protocol::NetworkItemStack {
                                network_id: 1,
                                count: 1,
                                ..Default::default()
                            }]),
                            storage_item: Default::default(),
                        },
                    ));
            }
            ui.dress_player_preview(&player, &runtime, Some(&world), Default::default(), |_| {
                Some(client_world::CanonicalItemStack {
                    identifier: Some(item.into()),
                    identity: assets::ItemStackIdentity::empty(),
                    visual: assets::ItemVisualRoute::Missing,
                    enchanted: false,
                    glint: assets::item::glint::ItemGlint::Session { enabled: true },
                    use_animation: assets::item_use::ItemUseAnimation::None,
                    grip: assets::item::grip::ItemGrip::Unresolved,
                    charged_projectile: None,
                    charged_projectile_role: assets::item_use::ItemProjectileRole::None,
                    damage: None,
                    durability: assets::item::durability::ItemDurability::NotDamageable,
                })
            });
            assert_model(&ui, &pack, 0, "mapped");
            assert_model(&ui, &pack, 1, "ordinary");
            assert!(
                ui.gui_player_mesh()
                    .unwrap()
                    .vertices()
                    .iter()
                    .any(|vertex| vertex.style_flags & u16::from(ui::UI_STYLE_GLINT) != 0)
            );
            if session {
                let mut textures = pack.equipment.as_ref().unwrap().textures().to_vec();
                textures
                    .iter_mut()
                    .find(|texture| texture.identifier.as_ref() == "textures/mapped")
                    .unwrap()
                    .rgba8 = [30, 110, 240, 255].repeat(1024).into();
                let replacement = assets::SessionEntityPack {
                    assets: Arc::clone(&pack.assets),
                    textures: Arc::from([]),
                    bindings: Arc::from([]),
                    equipment: Some(Arc::new(
                        RuntimeEquipmentCatalog::from_parts(
                            [1; 32],
                            pack.equipment.as_ref().unwrap().bindings().to_vec(),
                            textures,
                        )
                        .unwrap(),
                    )),
                };
                ui.set_preview_pack_equipment(replacement.equipment.clone());
                ui.set_player_preview_gear_with_owner(
                    [None; 4],
                    Some((item, 0)),
                    PreviewEquipmentOwner {
                        actor: world.authority().actor(1).unwrap(),
                        rig: world.authority().actor_rig(1).unwrap(),
                        input: Default::default(),
                    },
                );
                assert_model(&ui, &replacement, 0, "mapped");
                ui.set_preview_pack_equipment(None);
                ui.set_player_preview_gear([None; 4], Some((item, 0)));
                assert!(
                    ui.preview_held_models([true; 2])[0]
                        .as_ref()
                        .unwrap()
                        .placements
                        .iter()
                        .all(|placement| matches!(placement, PreviewHeldPlacement::Sprite { .. }))
                );
            }
        }
    }
}

/// Checks selected source geometry, atlas texels and the actual projected publication.
fn assert_model(
    ui: &UiPresentationRuntime,
    pack: &assets::SessionEntityPack,
    hand: usize,
    name: &str,
) -> player_preview::PreviewHeldModel {
    let actual = ui.preview_held_models([true; 2])[hand].clone().unwrap();
    let geometry =
        render_model::find_geometry_index(&pack.assets, &format!("geometry.{name}")).unwrap();
    let texture = pack
        .equipment
        .as_ref()
        .unwrap()
        .texture(&format!("textures/{name}"))
        .unwrap();
    let expected = render_model::attachable_geometry(
        &pack.assets,
        geometry as usize,
        render_model::item_mesh_rig_id(0),
        texture,
    )
    .unwrap();
    assert!(
        actual.vertices == expected.vertices,
        "wrong selected mesh for {name}"
    );
    let page = &ui.textures.pages()[actual.source.page as usize];
    let offset = (actual.source.uv[1] as usize * page.dimensions()[0] as usize
        + actual.source.uv[0] as usize)
        * 4;
    assert_eq!(&page.pixels()[offset..offset + 4], &texture.rgba8[..4]);
    let mesh = ui.gui_player_mesh().unwrap();
    assert!(
        mesh.batches()
            .iter()
            .skip(1)
            .any(|batch| batch.texture_page == actual.source.page)
    );
    actual
}

#[test]
fn selection_consumers_ownerless_shield_thumbnail_keeps_the_ordinary_definition() {
    use crate::test_support::fixture_hud;
    use ui_engine::test_support::fixture_font;
    let item = "minecraft:shield";
    let pack = fixture(item, "zfixture:shield");
    let icons = Arc::new(
        RuntimeIconCatalog::decode(
            &assets::encode_icon_catalog(
                pack.assets.source_manifest_sha256(),
                &[assets::IconSprite {
                    width: 16,
                    height: 16,
                    rgba8: vec![255; 1024].into(),
                }],
                &[assets::IconEntry {
                    identifier: item.into(),
                    metadata: 0,
                    sprite: 0,
                }],
            )
            .unwrap(),
        )
        .unwrap(),
    );
    let mut ui =
        UiPresentationRuntime::with_hud_and_icons(fixture_font(), fixture_hud(), icons).unwrap();
    ui.set_equipment_catalog(pack.equipment.clone());
    ui.set_gui_models(&assets::RuntimeAssets::diagnostic(), &pack.assets)
        .unwrap();
    let icon = ui.item_icon(item, 0).unwrap();
    let actual = ui
        .gui_models
        .models
        .get(&super::super::icon_key(icon))
        .expect("ordinary shield thumbnail must remain available without an owner");
    let index = render_model::find_geometry_index(&pack.assets, "geometry.ordinary").unwrap();
    let texture = pack
        .equipment
        .as_ref()
        .unwrap()
        .texture("textures/ordinary")
        .unwrap();
    let source =
        ui.gui_models.textures[&atlas::key([texture.width, texture.height], &texture.rgba8)];
    let expected = crate::ui_runtime::presentation::item_gui::shield(
        &pack.assets.geometries()[index as usize],
        texture,
        source,
    )
    .unwrap();
    assert_eq!(actual.vertices(), expected.vertices());
}

#[path = "selected_cache_tests.rs"]
mod cache;
