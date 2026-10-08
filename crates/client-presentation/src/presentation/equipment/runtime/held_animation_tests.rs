use super::{local_carrier, pack_runtime, player_body};
use crate::presentation::equipment::runtime::*;
use bevy::math::{Quat, Vec3};
use client_world::{ActorLifetimeId, ActorRigSnapshot, WorldAuthority};
use protocol::{ActorEvent, ActorKind, ActorSpawnEvent, WorldBootstrap, WorldEvent};
use std::sync::Arc;

fn owner() -> client_world::ActorSnapshot {
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
                kind: ActorKind::Entity {
                    identifier: "minecraft:skeleton".into(),
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

fn owner_rig<'a>(
    owner: &client_world::ActorSnapshot,
    names: &'a [Box<str>],
) -> ActorRigSnapshot<'a> {
    ActorRigSnapshot {
        actor: ActorLifetimeId {
            session_id: 1,
            dimension: 0,
            runtime_id: owner.runtime_id,
            spawn_revision: owner.spawn_revision,
        },
        rig: client_world::EntityRigId(0),
        previous: &[],
        current: &[],
        rest: &[],
        rest_completed_tick: 1,
        rest_reset_generation: 0,
        completed_tick: 1,
        reset_generation: 0,
        fallback: assets::EntityRigFallback::Skip,
        scale: 1.0,
        axis_scale: [1.0; 3],
        previous_body_yaw: 0.0,
        body_yaw: 0.0,
        render: &[],
        bone_names: names,
        skin_geometry: None,
        skin_mesh: None,
        skin_layers: &[],
        hand: [client_world::HandPhase::default(); 2],
        item_animation: [client_world::ItemAnimationState::default(); 2],
        off_hand_animation: [client_world::ItemAnimationState::default(); 2],
        animation_variables: Default::default(),
        java: Default::default(),
        java_equipped: None,
    }
}

fn held(identifier: &str) -> ActorEquipmentInput {
    ActorEquipmentInput {
        main: Some(WornItem {
            identifier: Arc::from(identifier),
            metadata: 0,
            damage: None,
            kind: HeldKind::Sprite,
            dye_rgb: None,
            enchanted: false,
        }),
        ..Default::default()
    }
}

#[test]
fn third_person_held_attachable_runs_authored_perspective_and_owner_bone_channels() {
    let raster = image::RgbaImage::from_pixel(16, 16, image::Rgba([255, 0, 0, 255]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    raster
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    let files = vec![
        (
            "attachables/held.json".into(),
            br#"{"format_version":"1.10.0","minecraft:attachable":{"description":{
            "identifier":"test:held","materials":{"default":"entity_alphatest"},
            "textures":{"default":"textures/items/held"},"geometry":{"default":"geometry.held"},
            "animations":{"wield":"animation.held"},"scripts":{"animate":["wield"]},
            "render_controllers":["controller.render.held"]}}}"#
                .to_vec(),
        ),
        (
            "models/entity/held.json".into(),
            br#"{"format_version":"1.16.0","minecraft:geometry":[{
            "description":{"identifier":"geometry.held","texture_width":16,"texture_height":16},
            "bones":[{"name":"rightitem","texture_meshes":[{"texture":"default"}]}]}]}"#
                .to_vec(),
        ),
        (
            "animations/held.json".into(),
            br#"{"format_version":"1.10.0","animations":{
            "animation.held":{"loop":true,"bones":{"rightitem":{
                "position":["c.is_first_person ? 50 : 2",3,4],"rotation":[0,0,0]}}}}}"#
                .to_vec(),
        ),
        (
            "render_controllers/held.json".into(),
            br#"{"format_version":"1.8.0","render_controllers":{
            "controller.render.held":{"geometry":"Geometry.default","textures":["Texture.default"],
                "materials":[{"*":"Material.default"}]}}}"#
                .to_vec(),
        ),
        ("textures/items/held.png".into(), bytes.into_inner()),
    ];
    let (mut runtime, _) = pack_runtime(files);
    let mut body = player_body(&mut runtime);
    let owner = owner();
    let names = [
        "root",
        "body",
        "head",
        "rightArm",
        "leftArm",
        "rightItem",
        "leftItem",
    ]
    .map(Box::from);
    let rig = owner_rig(&owner, &names);
    let hand = names
        .iter()
        .position(|name| name.as_ref() == "rightItem")
        .unwrap();
    let mut posed = body.input.current_bones.to_vec();
    posed[hand].rotation = Quat::from_rotation_x(0.7).to_array();
    posed[hand].translation_scale = [0.2, 0.8, -0.3, 1.0];
    body.input.previous_bones = posed.clone().into();
    body.input.current_bones = posed.into();
    let input = held("test:held");
    let layers = runtime.layers_for(
        &body,
        &input,
        Some(EquipmentAnimation {
            owner: &owner,
            rig: &rig,
            frame_alpha: 1.0,
        }),
    );
    assert_eq!(
        layers.len(),
        1,
        "an animated attachable must draw without an icon fallback"
    );
    let parent = body.input.current_bones[hand];
    let scale = Vec3::from_array(std::array::from_fn(|axis| {
        parent.axis_scale[axis] * parent.translation_scale[3]
    }));
    let expected = Vec3::from_slice(&parent.translation_scale[..3])
        + Quat::from_array(parent.rotation) * (Vec3::new(-2.0, 3.0, 4.0) / 16.0 * scale);
    let placed = layers[0].submission.input.current_bones[0];
    assert!(Vec3::from_slice(&placed.translation_scale[..3]).abs_diff_eq(expected, 1e-5));
    assert!(Quat::from_array(placed.rotation).abs_diff_eq(Quat::from_array(parent.rotation), 1e-5));
    runtime.take_pending_geometries();
    let repeated = runtime.layers_for(
        &body,
        &input,
        Some(EquipmentAnimation {
            owner: &owner,
            rig: &rig,
            frame_alpha: 1.0,
        }),
    );
    assert_eq!(
        repeated[0].submission.input.rig,
        layers[0].submission.input.rig
    );
    assert!(
        runtime.take_pending_geometries().is_empty(),
        "unchanged held geometry stays resident"
    );
}

#[test]
fn installed_bow_third_person_uses_authored_texture_mesh_and_wield_pose() {
    let (Some(entities), Some(icons), Some(equipment)) = (
        local_carrier("vanilla-v1.mcbeent"),
        local_carrier("vanilla-v1.mcbeico"),
        local_carrier("vanilla-v1.mcbeeqp"),
    ) else {
        return;
    };
    let entities = Arc::new(assets::RuntimeEntityAssets::decode(&entities).unwrap());
    let icons = Arc::new(assets::RuntimeIconCatalog::decode(&icons).unwrap());
    let catalog = Arc::new(assets::RuntimeEquipmentCatalog::decode(&equipment).unwrap());
    let (mut runtime, _, _) = EquipmentRuntime::build(
        entities,
        Some(catalog),
        icons,
        None,
        None,
        render::ActorArtworkPages::default(),
    );
    let body = player_body(&mut runtime);
    let owner = owner();
    let names = [
        "root",
        "body",
        "head",
        "rightArm",
        "leftArm",
        "rightItem",
        "leftItem",
    ]
    .map(Box::from);
    let rig = owner_rig(&owner, &names);
    let input = held("minecraft:bow");
    let expected = runtime
        .first_person_attachable(
            &body,
            input.main.as_ref().unwrap(),
            &owner,
            &rig,
            input.attachable_input(client_world::AttachableAnimationInput {
                frame_alpha: 1.0,
                ..Default::default()
            }),
            None,
        )
        .unwrap();
    let layers = runtime.layers_for(
        &body,
        &input,
        Some(EquipmentAnimation {
            owner: &owner,
            rig: &rig,
            frame_alpha: 1.0,
        }),
    );
    assert_eq!(layers.len(), 1);
    assert_eq!(
        layers[0].submission.input.rig, expected.presentation.submission.input.rig,
        "the third-person bow must use its authored texture mesh"
    );
    assert_eq!(
        layers[0].submission.input.current_bones,
        expected.presentation.submission.input.current_bones,
        "the third-person bow must retain its authored wield channels on the owner's hand"
    );
}
