//! Body-controller visibility must stay independent of worn equipment.
use super::*;
use crate::presentation::equipment::{ActorEquipmentInput, HeldKind, WornItem};
use assets::{RuntimeAssets, RuntimeEntityAssets, RuntimeEquipmentCatalog, RuntimeIconCatalog};
use protocol::{ActorEvent, ActorKind, ActorSpawnEvent, WorldBootstrap, WorldEvent};

/// Compiles original player and armour geometry with a controller that hides the skin torso.
fn fixture() -> (Arc<RuntimeEntityAssets>, Arc<RuntimeEquipmentCatalog>) {
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(16, 16, image::Rgba([180, 80, 20, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let files: Vec<(Box<str>, Vec<u8>)> = vec![
        ("entity/player.json".into(), br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{
            "identifier":"minecraft:player","materials":{"default":"entity_alphatest"},
            "textures":{"default":"textures/fixture"},"geometry":{"default":"geometry.fixture"},
            "render_controllers":["controller.render.body"]}}}"#.to_vec()),
        ("attachables/chest.json".into(), br#"{"format_version":"1.10.0","minecraft:attachable":{"description":{
            "identifier":"minecraft:iron_chestplate","materials":{"default":"armor"},
            "textures":{"default":"textures/fixture"},"geometry":{"default":"geometry.fixture"},
            "render_controllers":["controller.render.armour"]}}}"#.to_vec()),
        ("models/entity/fixture.json".into(), br#"{"format_version":"1.12.0","minecraft:geometry":[{
            "description":{"identifier":"geometry.fixture","texture_width":16,"texture_height":16},
            "bones":[{"name":"body","pivot":[0,24,0],"cubes":[{"origin":[-4,12,-2],"size":[8,12,4],"uv":[0,0]}]}]}]}"#.to_vec()),
        ("render_controllers/fixture.json".into(), br#"{"format_version":"1.8.0","render_controllers":{
            "controller.render.body":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],
                "textures":["Texture.default"],"part_visibility":[{"body":"0"}]},
            "controller.render.armour":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],
                "textures":["Texture.default"]}}}"#.to_vec()),
        ("textures/fixture.png".into(), png.into_inner()),
    ];
    let pack = pack_entity_compiler::compile_actor_pack(files)
        .unwrap()
        .unwrap();
    let catalog = RuntimeEquipmentCatalog::from_parts(
        pack.identity,
        pack.equipment_bindings,
        pack.equipment_textures,
    )
    .unwrap();
    (
        Arc::new(RuntimeEntityAssets::from_compiled(pack.entities).unwrap()),
        Arc::new(catalog),
    )
}

#[test]
fn hiding_a_player_torso_keeps_its_chestplate_visible() {
    let (entities, catalog) = fixture();
    let mut stream = WorldStream::new_with_asset_sets(
        WorldBootstrap {
            local_player_unique_id: 1,
            local_player_runtime_id: 1,
            dimension: 0,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(RuntimeAssets::diagnostic()),
        Arc::clone(&entities),
        [0.0; 3],
        None,
    );
    stream
        .submit(
            1,
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: 2,
                runtime_id: 2,
                kind: ActorKind::Player {
                    uuid: [2; 16],
                    username: "fixture".into(),
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
    stream.prepare_actor_appearance_fixture();
    stream.advance_actor_interpolation_ticks(1);
    let authority = stream.authority();
    let rig = authority.actor_rig(2).unwrap();
    assert!(
        rig.pose_evaluated,
        "the fixture must sample its body controller"
    );
    assert_eq!(rig.render.len(), 1);
    assert_eq!(rig.render[0].hidden_bones.as_ref(), &[0]);
    let body = crate::presentation::actors::actor_rig_presentation(
        &rig,
        authority.actor(2).unwrap(),
        None,
        1.0,
    )
    .unwrap();
    let mut batch =
        crate::presentation::actors::select_actor_presentations(2, true, Some(body), []);
    assert_eq!(batch.submissions[0].texture_layer, 0);
    let icons = Arc::new(
        RuntimeIconCatalog::decode(&assets::encode_icon_catalog([1; 32], &[], &[]).unwrap())
            .unwrap(),
    );
    let (mut equipment, pages, _) = EquipmentRuntime::build(
        Arc::clone(&entities),
        Some(catalog),
        icons,
        None,
        None,
        Default::default(),
    );
    let input = ActorEquipmentInput {
        armor: [
            None,
            Some(WornItem {
                identifier: "minecraft:iron_chestplate".into(),
                kind: HeldKind::Other,
                grip: assets::item::grip::ItemGrip::default(),
                use_facts: Default::default(),
                metadata: 0,
                damage: None,
                dye_rgb: None,
                enchanted: false,
                glint: Default::default(),
                variant: Default::default(),
                charged_projectile: None,
            }),
            None,
            None,
        ],
        ..Default::default()
    };
    let mut frame = authority.actor_render_frame(1.0);
    let mut state = ActorFrameState::default();
    layers::attach(
        &mut batch,
        Some(&stream),
        Some(&mut frame),
        layers::LayerPoses {
            local_runtime_id: 2,
            local_equipment: &input,
            java_posed: &[],
            combat_visuals: render_api::DEFAULT_COMBAT_VISUALS,
            local_emote_pose: None,
            remote_emote_poses: &Default::default(),
            native_body_sampled: false,
            remote_skin_layers: &Default::default(),
            native_pose: &state.java_hand.native_pose,
        },
        layers::LayerCaches {
            equipment: Some(&mut equipment),
            layer_poses: &mut state.layer_poses,
            cape_state: &mut state.cape_state,
            skin_layers: &mut state.skin_layers,
            skin_rigs: &mut state.skin_rigs,
        },
        &pages,
        Some(&entities),
        &mut ActorRenderScene::default(),
        (1.0, 0.05, None),
        &mut Vec::new(),
    );
    let body = batch
        .submissions
        .iter()
        .find(|draw| draw.input.identity.layer == render_model::ACTOR_LAYER_BODY)
        .unwrap();
    assert_eq!(body.input.current_bones[0].translation_scale[3], 0.0);
    let armour = batch
        .submissions
        .iter()
        .find(|draw| draw.input.identity.layer == crate::presentation::equipment::ELYTRA_LAYER)
        .unwrap();
    assert!(
        armour
            .input
            .current_bones
            .iter()
            .any(|bone| bone.translation_scale[3] > 0.0)
    );
    let mut pending = equipment.take_pending_geometries();
    pending.push(
        render_model::equipment_geometry(
            &entities,
            render_model::find_geometry_index(&entities, "geometry.fixture").unwrap() as usize,
            armour.input.rig,
        )
        .unwrap(),
    );
    let scene = ActorRenderScene::default();
    let body = body.clone();
    let contacts = cloth_contacts::collect(
        &body,
        &["body".into()],
        &batch.submissions,
        &scene,
        1.0,
        (&pending, Some(&equipment)),
    );
    assert_eq!(
        contacts.len(),
        1,
        "hidden skin must still leave its drawn armor contact"
    );
    for draw in &mut batch.submissions {
        if draw.input.identity.layer == crate::presentation::equipment::ELYTRA_LAYER {
            draw.route = render_model::ActorRigRoute::NoDraw;
        }
    }
    assert!(
        cloth_contacts::collect(
            &body,
            &["body".into()],
            &batch.submissions,
            &scene,
            1.0,
            (&pending, Some(&equipment)),
        )
        .is_empty(),
        "undrawn armor must not push cloth away from the body"
    );
}
