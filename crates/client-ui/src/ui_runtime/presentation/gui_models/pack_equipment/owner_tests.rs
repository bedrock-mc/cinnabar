//! Preview artwork and dye policy follow the player admitted by the item condition.
use super::*;
use player_preview::PreviewEquipmentOwner;

const ITEM: &str = "fixture:chestplate";
const PLAYER: [u8; 4] = [40, 190, 100, 255];
const STAND: [u8; 4] = [220, 110, 30, 255];
const STARTUP: [u8; 4] = [70, 100, 210, 255];
const DYE: u32 = 0x00d0_3040;

#[test]
fn preview_atlas_fallback_keeps_the_selected_chest_category() {
    let base = catalog(&[Definition {
        name: ITEM,
        condition: None,
        color: STARTUP,
        dyed: false,
    }]);
    let mut pack = catalog(&[Definition {
        name: "fixture:wings",
        condition: Some("1"),
        color: PLAYER,
        dyed: false,
    }]);
    let equipment = pack.equipment.as_ref().unwrap();
    let mut bindings = equipment.bindings().to_vec();
    bindings[0].category = assets::EquipmentCategory::Elytra;
    pack.equipment = Some(Arc::new(
        assets::RuntimeEquipmentCatalog::from_parts(
            [2; 32],
            bindings,
            equipment.textures().to_vec(),
        )
        .unwrap(),
    ));
    let world = owner(Arc::clone(&base.assets));
    let mut presentation = material_tests::presentation(base.equipment.as_ref().unwrap());
    let side = render_model::UI_MODEL_ATLAS_SIDE;
    let full =
        UiTexturePage::owned([side; 2], vec![255; (side * side * 4) as usize].into()).unwrap();
    presentation.gui_models.sources.pages = vec![full; MODEL_PAGES];
    presentation.gui_models.sources.required_pages = MODEL_PAGES;
    presentation.set_preview_equipment_assets(Some(base.assets), Some(pack.assets));
    presentation.set_preview_pack_equipment(pack.equipment);
    let cape_page = 100;
    presentation.menu_preview_model.cape_key = Some("fixture-cape".into());
    presentation.menu_artwork.refs.insert(
        "fixture-cape".into(),
        ui::IconRef {
            page: cape_page,
            uv: [0, 0, 64, 32],
            glint: false,
        },
    );
    presentation.gui_models.live_player.cape_vertices =
        player_preview::cape::rest_vertices().to_vec();
    presentation.set_player_preview_gear_with_owner(
        [None, Some((ITEM, None, false)), None, None],
        None,
        PreviewEquipmentOwner {
            actor: world.authority().actor(1).unwrap(),
            rig: world.authority().actor_rig(1).unwrap(),
            input: Default::default(),
        },
    );
    let mesh = presentation.gui_player_mesh().unwrap();
    let draw_cape = mesh
        .batches()
        .iter()
        .any(|batch| batch.texture_page == cape_page);
    let cape = render_api::CapeImage {
        width: 64,
        height: 32,
        rgba8: [210, 60, 40, 255].repeat(64 * 32).into(),
    };
    let pixels = player_preview::render_body_with_cape(
        &render_model::standard_biped_vertices(),
        &[120, 150, 180, 255].repeat(64 * 64),
        Default::default(),
        player_preview::PreviewView::Doll {
            yaw: 150.0,
            tilt: -10.0,
        },
        0.0,
        &presentation.player_preview_gear,
        draw_cape.then_some(&cape),
    );
    let image = image::RgbaImage::from_raw(
        player_preview::PREVIEW_WIDTH,
        player_preview::PREVIEW_HEIGHT,
        pixels,
    )
    .unwrap();
    let image = image::imageops::resize(
        &image,
        image.width() * 4,
        image.height() * 4,
        image::imageops::FilterType::Nearest,
    );
    super::super::super::forms::snapshot::write_image(&image, "preview-atlas-chest-cape");
    assert!(presentation.gui_models.pack_equipment.region(1).is_none());
    assert_eq!(
        &presentation.player_preview_gear.armor[1]
            .as_ref()
            .unwrap()
            .rgba[..4],
        &STARTUP
    );
    assert!(
        !presentation.gui_models.wearing_elytra,
        "the drawn startup chestplate must keep its cape"
    );
    assert!(
        draw_cape,
        "the preview mesh must retain the cape beside its fallback armour"
    );
}

#[test]
fn preview_cape_suppression_follows_the_admitted_chest_definition() {
    let base = catalog(&[Definition {
        name: ITEM,
        condition: None,
        color: STARTUP,
        dyed: false,
    }]);
    let mut pack = catalog(&[Definition {
        name: "fixture:z_wings",
        condition: Some("v.is_enchanted && q.is_in_ui && c.item_slot == 'torso'"),
        color: PLAYER,
        dyed: false,
    }]);
    let equipment = pack.equipment.as_ref().unwrap();
    let mut bindings = equipment.bindings().to_vec();
    bindings[0].category = assets::EquipmentCategory::Elytra;
    pack.equipment = Some(Arc::new(
        assets::RuntimeEquipmentCatalog::from_parts(
            [2; 32],
            bindings,
            equipment.textures().to_vec(),
        )
        .unwrap(),
    ));
    let world = owner(Arc::clone(&base.assets));
    let mut presentation = material_tests::presentation(base.equipment.as_ref().unwrap());
    let cape_page = 100;
    presentation.menu_preview_model.cape_key = Some("fixture-cape".into());
    presentation.menu_artwork.refs.insert(
        "fixture-cape".into(),
        ui::IconRef {
            page: cape_page,
            uv: [0, 0, 64, 32],
            glint: false,
        },
    );
    presentation.gui_models.live_player.cape_vertices =
        player_preview::cape::rest_vertices().to_vec();
    presentation.set_preview_equipment_assets(Some(base.assets), Some(pack.assets));
    presentation.set_preview_pack_equipment(pack.equipment);
    for enchanted in [false, true, false] {
        presentation.set_player_preview_gear_with_owner(
            [None, Some((ITEM, None, enchanted)), None, None],
            None,
            PreviewEquipmentOwner {
                actor: world.authority().actor(1).unwrap(),
                rig: world.authority().actor_rig(1).unwrap(),
                input: actor_animation::AttachableAnimationInput {
                    is_local_player: true,
                    frame_alpha: 1.0,
                    ..Default::default()
                },
            },
        );
        let selected = presentation.player_preview_gear.armor[1].as_ref().unwrap();
        assert_eq!(
            &selected.rgba[..4],
            if enchanted { &PLAYER } else { &STARTUP }
        );
        assert_eq!(presentation.gui_models.wearing_elytra, enchanted);
        for view in material_tests::views() {
            presentation.player_preview_view = view;
            let mesh = presentation.gui_player_mesh().unwrap();
            assert_eq!(
                mesh.batches()
                    .iter()
                    .any(|batch| batch.texture_page == cape_page),
                !enchanted,
                "{view:?} enchanted={enchanted}"
            );
        }
    }
    presentation.set_player_preview_gear([None, Some((ITEM, None)), None, None], None);
    assert!(!presentation.gui_models.wearing_elytra);
    presentation.set_player_preview_gear([None; 4], None);
    assert!(!presentation.gui_models.wearing_elytra);
}

struct Definition<'a> {
    name: &'a str,
    condition: Option<&'a str>,
    color: [u8; 4],
    dyed: bool,
}

/// Compiles original player and worn-item resources with independently identifiable artwork.
fn catalog(definitions: &[Definition<'_>]) -> assets::SessionEntityPack {
    let mut files = vec![
        ("models/fixture.json".into(), br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.fixture","texture_width":64,"texture_height":32},"bones":[{"name":"body","pivot":[0,24,0],"cubes":[{"origin":[-4,12,-2],"size":[8,12,4],"uv":[16,16]}]}]}]}"#.to_vec()),
        ("render_controllers/fixture.json".into(), br#"{"format_version":"1.8.0","render_controllers":{"controller.render.fixture":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}"#.to_vec()),
        ("entity/player.json".into(), br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:player","materials":{"default":"plain"},"textures":{"default":"textures/fixture/player"},"geometry":{"default":"geometry.fixture"},"render_controllers":["controller.render.fixture"]}}}"#.to_vec()),
        ("materials/fixture.material".into(), br#"{"materials":{"plain:entity_alphatest":{},"dyed:entity_alphatest":{"+defines":["USE_COLOR_MASK"]}}}"#.to_vec()),
    ];
    let mut textures = Vec::new();
    for definition in definitions {
        let texture = format!("textures/fixture/{}", definition.name.replace(':', "_"));
        let mut description = serde_json::json!({
            "identifier": definition.name,
            "materials":{"default":if definition.dyed {"dyed"} else {"plain"}},
            "textures":{"default":texture},
            "geometry":{"default":"geometry.fixture"},
            "render_controllers":["controller.render.fixture"]
        });
        if let Some(condition) = definition.condition {
            description["item"] = serde_json::json!({(ITEM):condition});
        }
        files.push((
            format!("attachables/{}.json", definition.name.replace(':', "_")).into(),
            serde_json::to_vec(&serde_json::json!({
                "format_version":"1.10.0","minecraft:attachable":{"description":description}
            }))
            .unwrap(),
        ));
        textures.push(assets::EquipmentTexture {
            identifier: texture.into(),
            width: 64,
            height: 32,
            rgba8: definition.color.repeat(64 * 32).into(),
        });
    }
    let compiled = pack_entity_compiler::compile_entity_pack(files)
        .unwrap()
        .unwrap();
    assets::SessionEntityPack {
        assets: Arc::new(assets::RuntimeEntityAssets::from_compiled(compiled.assets).unwrap()),
        textures: Arc::from([]),
        bindings: Arc::from([]),
        equipment: Some(Arc::new(
            assets::RuntimeEquipmentCatalog::from_parts(
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

/// Dresses all preview views and checks the artwork, atlas texels and GPU dye policy.
fn check_preview(pack: assets::SessionEntityPack, expected: [u8; 4], dyed: bool, name: &str) {
    let base = catalog(&[Definition {
        name: ITEM,
        condition: None,
        color: STARTUP,
        dyed: true,
    }]);
    let world = owner(Arc::clone(&base.assets));
    let mut presentation = material_tests::presentation(base.equipment.as_ref().unwrap());
    presentation.set_preview_equipment_assets(Some(base.assets), Some(pack.assets));
    presentation.set_preview_pack_equipment(pack.equipment);
    let input = actor_animation::AttachableAnimationInput {
        is_local_player: true,
        frame_alpha: 1.0,
        ..Default::default()
    };
    presentation.set_player_preview_gear_with_owner(
        [None, Some((ITEM, Some(DYE), false)), None, None],
        None,
        PreviewEquipmentOwner {
            actor: world.authority().actor(1).unwrap(),
            rig: world.authority().actor_rig(1).unwrap(),
            input,
        },
    );
    let mut capture = image::RgbaImage::new(
        player_preview::PREVIEW_WIDTH * 3,
        player_preview::PREVIEW_HEIGHT,
    );
    for (column, view) in material_tests::views().into_iter().enumerate() {
        let pixels = player_preview::render(
            &[120, 150, 180, 255].repeat(64 * 64),
            Default::default(),
            view,
            0.0,
            &presentation.player_preview_gear,
        );
        let image = image::RgbaImage::from_raw(
            player_preview::PREVIEW_WIDTH,
            player_preview::PREVIEW_HEIGHT,
            pixels,
        )
        .unwrap();
        image::imageops::replace(
            &mut capture,
            &image,
            column as i64 * i64::from(image.width()),
            0,
        );
    }
    super::super::super::forms::snapshot::write_image(&capture, name);
    let armor = presentation.player_preview_gear.armor[1].as_ref().unwrap();
    assert_eq!(&armor.rgba[..4], &expected);
    let tint = dyed.then_some([(DYE >> 16) as u8, (DYE >> 8) as u8, DYE as u8]);
    assert_eq!(armor.tint, tint);
    let icon = presentation
        .gui_models
        .pack_equipment
        .region(1)
        .unwrap_or_else(|| {
            presentation.gui_models.sources.textures
                [&atlas::key([armor.width, armor.height], &armor.rgba)]
        });
    let page = &presentation.textures.pages()[usize::from(icon.page)];
    let offset =
        (usize::from(icon.uv[1]) * page.dimensions()[0] as usize + usize::from(icon.uv[0])) * 4;
    assert_eq!(&page.pixels()[offset..offset + 4], &expected);
    for view in material_tests::views() {
        presentation.player_preview_view = view;
        let mesh = presentation.gui_player_mesh().unwrap();
        assert_eq!(mesh.batches()[1].texture_page, icon.page);
        assert_eq!(
            material_tests::armor_shading(&presentation),
            (
                vec![tint.map_or([255; 4], |[r, g, b]| [r, g, b, 255])],
                dyed
            ),
            "{view:?}"
        );
    }
}

#[test]
fn player_preview_ignores_a_stand_definition_that_sorts_after_the_player() {
    let pack = catalog(&[
        Definition {
            name: "fixture:a_player",
            condition: Some(
                "q.owner_identifier == 'minecraft:player' && c.item_slot == 'torso' && q.is_local_player",
            ),
            color: PLAYER,
            dyed: false,
        },
        Definition {
            name: "fixture:z_stand",
            condition: Some("q.owner_identifier == 'minecraft:armor_stand'"),
            color: STAND,
            dyed: true,
        },
    ]);
    assert_eq!(
        pack.equipment
            .as_ref()
            .unwrap()
            .binding(ITEM)
            .unwrap()
            .attachable_identifier(),
        "fixture:z_stand"
    );
    check_preview(pack, PLAYER, false, "armour-preview-owner");
}

#[test]
fn player_preview_keeps_startup_art_when_only_a_stand_override_is_admitted() {
    check_preview(
        catalog(&[Definition {
            name: "fixture:z_stand",
            condition: Some("q.owner_identifier == 'minecraft:armor_stand'"),
            color: STAND,
            dyed: false,
        }]),
        STARTUP,
        true,
        "armour-preview-startup",
    );
}

#[test]
fn player_preview_rejects_malformed_and_exhausted_owner_predicates() {
    for condition in [
        "q.owner_identifier ==",
        "loop(1024, { v.spin = v.spin + 1; }); return 1;",
    ] {
        check_preview(
            catalog(&[Definition {
                name: "fixture:z_invalid",
                condition: Some(condition),
                color: STAND,
                dyed: false,
            }]),
            STARTUP,
            true,
            "armour-preview-invalid",
        );
    }
}

/// Checks actual preview dressing, semantic selection, dye and shine for each stack state.
fn check_stack_enchantment_selection(condition: &str) {
    let base = catalog(&[Definition {
        name: ITEM,
        condition: None,
        color: STARTUP,
        dyed: true,
    }]);
    let pack = catalog(&[Definition {
        name: "fixture:enchanted",
        condition: Some(condition),
        color: PLAYER,
        dyed: false,
    }]);
    let world = owner(Arc::clone(&base.assets));
    let mut presentation = material_tests::presentation(base.equipment.as_ref().unwrap());
    presentation.set_preview_equipment_assets(Some(base.assets), Some(pack.assets));
    presentation.set_preview_pack_equipment(pack.equipment);
    let runtime = crate::ui_runtime::UiRuntime::new(1);
    let mut player = player_state::PlayerState::new(1);
    for (enchanted, intrinsic) in [
        (false, false),
        (true, false),
        (false, true),
        (true, true),
        (false, false),
    ] {
        let extra_data: Arc<[u8]> = if enchanted {
            Arc::from([
                255, 255, 1, 10, 0, 0, 9, 4, 0, b'e', b'n', b'c', b'h', 10, 0, 0, 0, 0, 0,
            ])
        } else {
            Arc::from([])
        };
        let stack = protocol::NetworkItemStack {
            network_id: 1,
            count: 1,
            extra_data,
            ..Default::default()
        };
        assert_eq!(runtime.stack_facts(&stack).has_enchantment_list, enchanted);
        player
            .inventory
            .ledger_mut()
            .apply(&protocol::InventoryEvent::Content(
                protocol::InventoryContentEvent {
                    container: protocol::ContainerIdentity::window(protocol::ARMOR_WINDOW_ID),
                    slots: Arc::from([protocol::NetworkItemStack::default(), stack]),
                    storage_item: Default::default(),
                },
            ));
        presentation.dress_player_preview(
            &player,
            &runtime,
            Some(&world),
            Default::default(),
            |stack| {
                Some(client_world::CanonicalItemStack {
                    identifier: Some(ITEM.into()),
                    identity: assets::ItemStackIdentity::empty(),
                    visual: assets::ItemVisualRoute::Missing,
                    enchanted: runtime.stack_facts(stack).has_enchantment_list,
                    glint: assets::item::glint::ItemGlint::Session { enabled: intrinsic },
                    use_animation: assets::item_use::ItemUseAnimation::None,
                    grip: assets::item::grip::ItemGrip::Unresolved,
                    charged_projectile: None,
                    charged_projectile_role: assets::item_use::ItemProjectileRole::None,
                    damage: None,
                    durability: assets::item::durability::ItemDurability::NotDamageable,
                })
            },
        );
        let selected = presentation.player_preview_gear.armor[1].as_ref().unwrap();
        assert_eq!(
            &selected.rgba[..4],
            if enchanted { &PLAYER } else { &STARTUP }
        );
        assert_eq!(selected.tint.is_none(), enchanted);
        assert_eq!(
            presentation.player_preview_gear.armor_glint[1],
            enchanted || intrinsic
        );
    }
}

#[test]
fn worn_selection_in_previews_uses_stack_enchantments_independently_of_visual_glint() {
    check_stack_enchantment_selection("v.is_enchanted && c.item_slot == 'torso'");
}

#[test]
fn owner_context_preview_predicates_observe_ui_while_world_selection_stays_ordinary() {
    let pack = catalog(&[
        Definition {
            name: ITEM,
            condition: None,
            color: STARTUP,
            dyed: true,
        },
        Definition {
            name: "fixture:ui",
            condition: Some("q.is_in_ui && c.item_slot == 'torso'"),
            color: PLAYER,
            dyed: false,
        },
    ]);
    let world = owner(Arc::clone(&pack.assets));
    let mut selector = actor_animation::AttachablesRuntime::new(Arc::clone(&pack.assets));
    let mut budget = actor_animation::MAX_MOLANG_OPS_PER_ACTOR_TICK;
    let catalog = pack.equipment.as_ref().unwrap();
    let index = selector
        .select_equipment_binding(
            catalog,
            ITEM,
            world.authority().actor(1).unwrap(),
            &world.authority().actor_rig(1).unwrap(),
            actor_animation::AttachableAnimationInput {
                worn: true,
                worn_slot: 1,
                ..Default::default()
            },
            &mut budget,
            |_| true,
        )
        .unwrap();
    assert_eq!(
        catalog.bindings()[index].attachable_identifier(),
        ITEM,
        "a world rig must not admit a UI-only mapping"
    );
    check_preview(pack, PLAYER, false, "armour-owner-context-ui");
    check_stack_enchantment_selection("q.is_in_ui && v.is_enchanted && c.item_slot == 'torso'");
}
