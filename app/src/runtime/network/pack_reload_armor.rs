//! Startup bindings follow texture-only pack publication without loose base files.

use actor_animation::{
    ActorAnimationVariables, ActorLifetimeId, ActorRigSnapshot, HandPhase, ItemAnimationState,
};
use assets::EntityRigFallback;
use client_presentation::presentation::equipment::{
    ActorEquipmentInput, EquipmentRuntime, HeldKind, WornItem,
};
use client_world::{ActorSnapshot, WorldAuthority};
use protocol::{ActorEvent, ActorKind, ActorSpawnEvent, WorldBootstrap, WorldEvent};
use render_model::{ActorArtworkPages, ActorRigSubmission};
use std::sync::Arc;

const ITEM: &str = "fixture:chestplate";
const TEXTURE: &str = "textures/models/fixture_armor.png";

/// Encodes original, solid-colour armour artwork for a synthetic pack.
fn png(color: [u8; 4]) -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(4, 2, image::Rgba(color))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

/// Compiles the base equipment and its retained definitions from tiny synthetic documents.
fn base(
    texture: &str,
) -> (
    assets::VanillaEntityRefs,
    assets::SessionEntityPack,
    tempfile::TempDir,
) {
    let files: Vec<(Box<str>, Vec<u8>)> = vec![
        (
            "attachables/fixture.json".into(),
            serde_json::json!({
                "format_version": "1.10.0", "minecraft:attachable": {"description": {
                    "identifier": ITEM, "materials": {"default": "armor_leather"},
                    "textures": {"default": texture.strip_suffix(".png").unwrap(),
                        "enchanted": assets::ACTOR_GLINT_TEXTURE_IDENTIFIER},
                    "geometry": {"default": "geometry.fixture.chestplate"},
                    "animations": {"idle": "animation.fixture.idle"},
                    "scripts": {"animate": ["idle"]},
                    "render_controllers": ["controller.render.fixture"]
                }}
            })
            .to_string()
            .into_bytes(),
        ),
        (
            "models/entity/fixture.json".into(),
            serde_json::json!({
                "format_version": "1.12.0", "minecraft:geometry": [{
                    "description": {"identifier": "geometry.fixture.chestplate",
                        "texture_width": 4, "texture_height": 2},
                    "bones": [{"name": "body", "pivot": [0, 0, 0],
                        "cubes": [{"origin": [0, 0, 0], "size": [1, 1, 1], "uv": [0, 0]}]}]
                }]
            })
            .to_string()
            .into_bytes(),
        ),
        (
            "render_controllers/fixture.json".into(),
            serde_json::json!({
                "format_version": "1.8.0", "render_controllers": {"controller.render.fixture": {
                    "geometry": "Geometry.default", "materials": [{"*": "Material.default"}],
                    "textures": ["Texture.default", "Texture.enchanted"]
                }}
            })
            .to_string()
            .into_bytes(),
        ),
        (
            "animations/fixture.json".into(),
            serde_json::to_vec(&serde_json::json!({
                "format_version": "1.8.0", "animations": {"animation.fixture.idle": {
                    "loop": true, "bones": {"body": {"rotation": [1, 2, 3]}}
                }}
            }))
            .unwrap(),
        ),
        (texture.into(), png([10, 20, 30, 255])),
        (
            format!("{}.png", assets::ACTOR_GLINT_TEXTURE_IDENTIFIER).into(),
            png([1, 2, 3, 255]),
        ),
    ];
    let root = tempfile::tempdir().unwrap();
    for (path, bytes) in &files {
        let path = root.path().join(path.as_ref());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    let refs = pack_entity_compiler::compile_vanilla_entity_refs(root.path()).unwrap();
    let pack = compiled_pack(files).unwrap();
    (refs, pack, root)
}

/// Compiles an optional session layer through the same entity and equipment compiler.
fn compiled_pack(files: Vec<(Box<str>, Vec<u8>)>) -> Option<assets::SessionEntityPack> {
    let compiled = pack_entity_compiler::compile_actor_pack(files).unwrap()?;
    Some(assets::SessionEntityPack {
        assets: Arc::new(assets::RuntimeEntityAssets::from_compiled(compiled.entities).unwrap()),
        textures: compiled.textures.into(),
        bindings: compiled.bindings.into(),
        equipment: Some(Arc::new(
            assets::RuntimeEquipmentCatalog::from_parts(
                compiled.identity,
                compiled.equipment_bindings,
                compiled.equipment_textures,
            )
            .unwrap(),
        )),
    })
}

/// Admits the fixture wearable and keeps its body pose unchanged across pack publication.
fn body(runtime: &mut EquipmentRuntime) -> ActorRigSubmission {
    let items = client_presentation::session_assets::SessionItems {
        components: Arc::new(
            [(
                Arc::from(ITEM),
                protocol::ItemComponents {
                    wearable_slot: Some("slot.armor.chest".into()),
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
        ),
        icons: None,
    };
    runtime.set_session_items(Some(&items), None, Vec::new());
    let rig = render_model::skin_rig_id(0);
    runtime.register_skin_rig(rig, vec!["body".into()]);
    let pose: Arc<[render_model::RenderBoneTransform]> =
        Arc::from([render_model::RenderBoneTransform {
            rotation: [0.0, 0.0, 0.0, 1.0],
            translation_scale: [0.0, 0.0, 0.0, 1.0],
            axis_scale: render_model::UNIT_AXIS_SCALE,
        }]);
    ActorRigSubmission {
        material: Default::default(),
        culling_bounds: Default::default(),
        input: render_model::ActorRigRenderInput {
            identity: render_model::ActorRenderIdentity {
                session_id: 1,
                dimension: 0,
                runtime_id: 2,
                spawn_revision: 1,
                ingress_sequence: 1,
                source_tick: None,
                movement_revision: 0,
                pose_generation: render_api::PoseGeneration::new(1),
                layer: render_model::ACTOR_LAYER_BODY,
            },
            rig,
            previous_bones: Arc::clone(&pose),
            current_bones: pose,
            completed_tick: 0,
            reset_generation: 0,
        },
        world_from_actor: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ],
        texture_layer: 0,
        route: render_model::ActorRigRoute::Compiled,
        tint: 0,
        overlay_rgba8: 0,
        uv_anim: render_model::IDENTITY_UV_ANIM,
        light: 0,
    }
}

/// Produces a real player snapshot through the public event admission path.
fn owner() -> ActorSnapshot {
    let mut world = WorldAuthority::new(
        WorldBootstrap {
            local_player_unique_id: 1,
            local_player_runtime_id: 1,
            dimension: 0,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(assets::RuntimeAssets::diagnostic()),
        None,
        [0.0; 3],
        None,
    );
    world
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: 2,
                runtime_id: 2,
                kind: ActorKind::Player {
                    uuid: [2; 16],
                    username: "wing-test".into(),
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
            Some(1),
        )
        .unwrap();
    world.actor(2).unwrap().clone()
}

/// Supplies tick-owned actor identity and body-name bindings to the authored attachable.
fn owner_rig<'a>(owner: &ActorSnapshot, names: &'a [Box<str>], tick: u64) -> ActorRigSnapshot<'a> {
    ActorRigSnapshot {
        placement_frame: assets::placement::EntityPlacementFrame::Unresolved,
        actor: ActorLifetimeId {
            session_id: 1,
            dimension: 0,
            runtime_id: owner.runtime_id,
            spawn_revision: owner.spawn_revision,
        },
        rig: actor_animation::EntityRigId(0),
        previous: &[],
        current: &[],
        rest: &[],
        rest_completed_tick: tick,
        rest_reset_generation: 0,
        completed_tick: tick,
        pose_evaluated: true,
        reset_generation: 0,
        fallback: EntityRigFallback::Skip,
        scale: 1.0,
        axis_scale: [1.0; 3],
        held_item_scale: None,
        previous_body_yaw: 0.0,
        body_yaw: 0.0,
        render: &[],
        bone_names: names,
        attachments: None,
        skin_geometry: None,
        skin_mesh: None,
        skin_layers: &[],
        hand: [HandPhase::default(); 2],
        item_animation: [ItemAnimationState::default(); 2],
        off_hand_animation: [ItemAnimationState::default(); 2],
        animation_variables: ActorAnimationVariables::default(),
        java: Default::default(),
        main_hand_equipped: None,
        java_equipped: None,
    }
}

/// Equips the fixture chestplate with explicit dye and enchantment state.
fn equipped(dye_rgb: Option<u32>, enchanted: bool) -> ActorEquipmentInput {
    ActorEquipmentInput {
        armor: [
            None,
            Some(WornItem {
                variant: Default::default(),
                charged_projectile: None,
                grip: assets::item::grip::builtin_item_grip(ITEM),
                use_facts: Default::default(),
                identifier: ITEM.into(),
                metadata: 0,
                damage: None,
                kind: HeldKind::Other,
                dye_rgb,
                enchanted,
                glint: Default::default(),
            }),
            None,
            None,
        ],
        ..Default::default()
    }
}

/// Reads the selected armour texel without assuming an atlas page or layer number.
fn worn_pixel(
    runtime: &mut EquipmentRuntime,
    body: &ActorRigSubmission,
    equipped: &ActorEquipmentInput,
    pages: &ActorArtworkPages,
) -> [u8; 4] {
    let owner = owner();
    let names = [Box::from("body")];
    let rig = owner_rig(&owner, &names, 1);
    let layers = runtime.layers_for(
        body,
        equipped,
        Some(
            client_presentation::presentation::equipment::EquipmentAnimation {
                owner: &owner,
                rig: &rig,
                frame_alpha: 1.0,
                delta_seconds: simulation_clock::TICK_DURATION.as_secs_f32(),
            },
        ),
    );
    let [layer] = layers else {
        panic!("one worn armour layer");
    };
    assert_eq!(
        layer.submission.input.identity.runtime_id,
        body.input.identity.runtime_id
    );
    let page = &pages.pages()[usize::from(layer.location.page() - 1)];
    let (width, height) = page.dimensions();
    let start = layer.location.layer() as usize * usize::from(width) * usize::from(height) * 4;
    page.pixels()[start..start + 4].try_into().unwrap()
}

#[test]
fn hot_reload_replaces_armour_on_an_already_equipped_actor_and_restores_base() {
    for texture in [TEXTURE, "textures/items/fixture_armor.png"] {
        let (refs, base_pack, _installed) = base(texture);
        let icons = Arc::new(
            assets::RuntimeIconCatalog::decode(
                &assets::encode_icon_catalog([0; 32], &[], &[]).unwrap(),
            )
            .unwrap(),
        );
        let base_catalog = base_pack.equipment.clone();
        let (mut runtime, base_pages, _) = EquipmentRuntime::build(
            base_pack.assets,
            base_pack.equipment,
            icons,
            None,
            None,
            ActorArtworkPages::default(),
        );
        let body = body(&mut runtime);
        let equipped = equipped(Some(0x234567), false);
        assert_eq!(
            worn_pixel(&mut runtime, &body, &equipped, &base_pages),
            [10, 20, 30, 255]
        );
        let startup_rig = runtime.layers_for(&body, &equipped, None)[0]
            .submission
            .input
            .rig;
        for color in [[40, 50, 60, 255], [70, 80, 90, 255]] {
            let pixels = png(color);
            let view = resource_pack::LayeredPackView::new(super::super::pack_reload_tests::stack(
                &[(texture, &pixels)],
            ));
            let pack = pack_runtime::entity_pack::compile(
                &view,
                Some(&refs),
                None,
                base_catalog.as_deref(),
            );
            let (pages, locations) = match &pack {
                Some(pack) => {
                    base_pages
                        .clone()
                        .with_equipment_rasters(&EquipmentRuntime::pack_rasters(
                            pack.equipment.as_ref().unwrap(),
                        ))
                }
                None => (base_pages.clone(), Vec::new()),
            };
            runtime.set_pack_layer(
                pack.map(|pack| (Arc::clone(&pack.assets), pack.equipment.clone(), locations)),
            );
            assert_ne!(pages.identity(), base_pages.identity());
            assert_eq!(worn_pixel(&mut runtime, &body, &equipped, &pages), color);
            assert_eq!(
                runtime.layers_for(&body, &equipped, None)[0]
                    .submission
                    .input
                    .rig,
                startup_rig
            );
        }
        runtime.set_pack_layer(None);
        assert_eq!(
            worn_pixel(&mut runtime, &body, &equipped, &base_pages),
            [10, 20, 30, 255]
        );
    }
}
#[test]
fn a_sidecar_only_glint_override_keeps_the_base_armour_visible() {
    let (refs, base_pack, _installed) = base(TEXTURE);
    let icons = Arc::new(
        assets::RuntimeIconCatalog::decode(
            &assets::encode_icon_catalog([0; 32], &[], &[]).unwrap(),
        )
        .unwrap(),
    );
    let base_catalog = base_pack.equipment.clone();
    let (mut runtime, base_pages, _) = EquipmentRuntime::build(
        base_pack.assets,
        base_pack.equipment,
        icons,
        None,
        None,
        ActorArtworkPages::default(),
    );
    let body = body(&mut runtime);
    let equipped = equipped(None, true);
    let path = format!("{}.png", assets::ACTOR_GLINT_TEXTURE_IDENTIFIER);
    let view = resource_pack::LayeredPackView::new(super::super::pack_reload_tests::stack(&[(
        &path,
        &png([40, 50, 60, 255]),
    )]));
    let pack =
        pack_runtime::entity_pack::compile(&view, Some(&refs), None, base_catalog.as_deref())
            .expect("the supplied raster forms an artwork-only layer");
    let catalog = pack.equipment.as_ref().unwrap();
    assert!(catalog.texture("textures/models/fixture_armor").is_none());
    let glint = EquipmentRuntime::actor_glint(catalog).expect("the glint override is retained");
    let (pages, locations) =
        base_pages.with_equipment_rasters(&EquipmentRuntime::pack_rasters(catalog));
    let pages = pages.with_actor_glint(glint);
    runtime.set_pack_layer(Some((
        Arc::clone(&pack.assets),
        pack.equipment.clone(),
        locations,
    )));
    assert_eq!(
        worn_pixel(&mut runtime, &body, &equipped, &pages),
        [10, 20, 30, 255]
    );
    assert_eq!(&pages.actor_glint().unwrap().rgba8[..4], &[40, 50, 60, 255]);
    assert!(catalog.binding(ITEM).is_none());
}

#[test]
fn mixed_texture_and_geometry_reload_keeps_the_authored_equipment_model() {
    let (refs, base_pack, installed) = base(TEXTURE);
    let base_catalog = base_pack.equipment.clone();
    let icons = Arc::new(
        assets::RuntimeIconCatalog::decode(
            &assets::encode_icon_catalog([0; 32], &[], &[]).unwrap(),
        )
        .unwrap(),
    );
    let (mut runtime, base_pages, _) = EquipmentRuntime::build(
        base_pack.assets,
        base_pack.equipment,
        icons,
        None,
        None,
        ActorArtworkPages::default(),
    );
    let body = body(&mut runtime);
    let equipped = equipped(None, false);
    let startup_rig = runtime.layers_for(&body, &equipped, None)[0]
        .submission
        .input
        .rig;
    let replacement = serde_json::to_vec(&serde_json::json!({
        "format_version": "1.12.0", "minecraft:geometry": [{
            "description": {"identifier": "geometry.fixture.chestplate", "texture_width": 4, "texture_height": 2},
            "bones": [{"name": "body", "pivot": [0, 0, 0],
                "cubes": [{"origin": [0, 0, 0], "size": [2, 3, 4], "uv": [0, 0]}]}]
        }]
    })).unwrap();
    let pixels = png([40, 50, 60, 255]);
    let view = resource_pack::LayeredPackView::new(super::super::pack_reload_tests::stack(&[
        (TEXTURE, &pixels),
        ("models/entity/replacement.json", &replacement),
    ]));
    let pack = pack_runtime::entity_pack::compile(
        &view,
        Some(&refs),
        Some(installed.path()),
        base_catalog.as_deref(),
    )
    .unwrap();
    let geometry =
        render_model::find_geometry_index(&pack.assets, "geometry.fixture.chestplate").unwrap();
    let catalog = pack.equipment.as_ref().unwrap();
    let (pages, locations) =
        base_pages.with_equipment_rasters(&EquipmentRuntime::pack_rasters(catalog));
    runtime.set_pack_layer(Some((
        Arc::clone(&pack.assets),
        pack.equipment.clone(),
        locations,
    )));
    assert_eq!(
        worn_pixel(&mut runtime, &body, &equipped, &pages),
        [40, 50, 60, 255]
    );
    let rig = runtime.layers_for(&body, &equipped, None)[0]
        .submission
        .input
        .rig;
    assert_ne!(rig, startup_rig);
    assert_eq!(rig, render_model::pack_equipment_rig_id(geometry));
    assert_eq!(
        pack.assets.geometries()[geometry as usize].bones[0].cubes[0]
            .size
            .map(|value| value.get()),
        [2.0, 3.0, 4.0]
    );
    assert!(catalog.binding(ITEM).is_some());
}

#[test]
fn mixed_texture_and_material_reload_disables_authored_armour_dye() {
    let (refs, base_pack, installed) = base(TEXTURE);
    let base_catalog = base_pack.equipment.clone();
    let dye = Some(0x234567);
    assert_eq!(
        base_catalog
            .as_ref()
            .unwrap()
            .binding(ITEM)
            .unwrap()
            .color_mask_rgb(dye),
        dye
    );
    let icons = Arc::new(
        assets::RuntimeIconCatalog::decode(
            &assets::encode_icon_catalog([0; 32], &[], &[]).unwrap(),
        )
        .unwrap(),
    );
    let (mut runtime, base_pages, _) = EquipmentRuntime::build(
        base_pack.assets,
        base_pack.equipment,
        icons,
        None,
        None,
        ActorArtworkPages::default(),
    );
    let body = body(&mut runtime);
    let equipped = equipped(dye, false);
    assert_ne!(
        runtime.layers_for(&body, &equipped, None)[0]
            .submission
            .tint,
        0
    );
    let materials = serde_json::to_vec(&serde_json::json!({"materials": {
        "armor_leather": {"defines": []}
    }}))
    .unwrap();
    let pixels = png([40, 50, 60, 255]);
    let view = resource_pack::LayeredPackView::new(super::super::pack_reload_tests::stack(&[
        (TEXTURE, &pixels),
        ("materials/fixture.material", &materials),
    ]));
    let pack = pack_runtime::entity_pack::compile(
        &view,
        Some(&refs),
        Some(installed.path()),
        base_catalog.as_deref(),
    )
    .unwrap();
    let catalog = pack.equipment.as_ref().unwrap();
    let binding = catalog
        .binding(ITEM)
        .expect("the changed material requires a pack binding");
    assert_eq!(binding.color_mask, assets::EquipmentColorMask::NoMask);
    assert_eq!(binding.color_mask_rgb(dye), None);
    let (pages, locations) =
        base_pages.with_equipment_rasters(&EquipmentRuntime::pack_rasters(catalog));
    runtime.set_pack_layer(Some((
        Arc::clone(&pack.assets),
        pack.equipment.clone(),
        locations,
    )));
    assert_eq!(
        worn_pixel(&mut runtime, &body, &equipped, &pages),
        [40, 50, 60, 255]
    );
    for profile in [
        render_api::CombatVisuals::Modern,
        render_api::CombatVisuals::Legacy112,
    ] {
        let layers = runtime
            .layers_for_profile(&body, &equipped, profile, None)
            .layers;
        assert_eq!(layers[0].submission.tint, 0);
        assert!(!layers[0].submission.material.color_mask);
    }
    runtime.set_pack_layer(None);
    assert_ne!(
        runtime.layers_for(&body, &equipped, None)[0]
            .submission
            .tint,
        0
    );
}
