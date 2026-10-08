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

fn held_pack() -> Vec<(Box<str>, Vec<u8>)> {
    let raster = image::RgbaImage::from_pixel(16, 16, image::Rgba([255, 0, 0, 255]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    raster
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    vec![
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
    ]
}

#[test]
fn third_person_held_attachable_runs_authored_perspective_and_owner_bone_channels() {
    let (mut runtime, _) = pack_runtime(held_pack());
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
            delta_seconds: client_world::ACTOR_TICK_DURATION.as_secs_f32(),
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
            delta_seconds: client_world::ACTOR_TICK_DURATION.as_secs_f32(),
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
    assert!(
        Arc::ptr_eq(
            &repeated[0].submission.input.current_bones,
            &layers[0].submission.input.current_bones
        ),
        "unchanged poses retain their matrix-cache key"
    );
}

#[test]
fn third_person_held_attachable_receives_the_render_delta() {
    let mut files = held_pack();
    let (_, clip) = files
        .iter_mut()
        .find(|(path, _)| path.as_ref() == "animations/held.json")
        .unwrap();
    let mut document: serde_json::Value = serde_json::from_slice(clip).unwrap();
    document["animations"]["animation.held"]["bones"]["rightitem"]["position"] =
        serde_json::json!(["query.delta_time", 0, 0]);
    *clip = serde_json::to_vec(&document).unwrap();
    let (mut runtime, _) = pack_runtime(files);
    let body = player_body(&mut runtime);
    let owner = owner();
    let names = ["rightItem".into()];
    let rig = owner_rig(&owner, &names);
    let input = held("test:held");
    for delta_seconds in [0.01, 0.035] {
        let layers = runtime.layers_for(
            &body,
            &input,
            Some(EquipmentAnimation {
                owner: &owner,
                rig: &rig,
                frame_alpha: 1.0,
                delta_seconds,
            }),
        );
        assert_eq!(layers.len(), 1);
        let x = layers[0].submission.input.current_bones[0].translation_scale[0];
        assert!(
            (x + delta_seconds / 16.0).abs() < 1e-6,
            "authored channel must receive the current render delta: {delta_seconds}, x {x}"
        );
    }
}

#[test]
fn worn_attachable_categories_do_not_become_held_models() {
    for identifier in ["minecraft:elytra", "test:helmet"] {
        let mut files = held_pack();
        let (_, attachable) = files
            .iter_mut()
            .find(|(path, _)| path.as_ref() == "attachables/held.json")
            .unwrap();
        let mut document: serde_json::Value = serde_json::from_slice(attachable).unwrap();
        document["minecraft:attachable"]["description"]["identifier"] =
            serde_json::json!(identifier);
        *attachable = serde_json::to_vec(&document).unwrap();
        let (mut runtime, _) = pack_runtime(files);
        let body = player_body(&mut runtime);
        let owner = owner();
        let names = ["rightItem".into()];
        let rig = owner_rig(&owner, &names);
        let layers = runtime.layers_for(
            &body,
            &held(identifier),
            Some(EquipmentAnimation {
                owner: &owner,
                rig: &rig,
                frame_alpha: 1.0,
                delta_seconds: client_world::ACTOR_TICK_DURATION.as_secs_f32(),
            }),
        );
        assert!(
            layers.is_empty(),
            "{identifier}'s worn model must not replace its missing held icon"
        );
    }
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
            delta_seconds: client_world::ACTOR_TICK_DURATION.as_secs_f32(),
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

#[test]
fn custom_wearable_model_draws_only_in_its_effective_armor_slot() {
    let (mut runtime, _) = pack_runtime(held_pack());
    let body = player_body(&mut runtime);
    let owner = owner();
    let names = ["rightItem".into()];
    let rig = owner_rig(&owner, &names);
    let items = super::session_items(
        vec![(
            "test:held",
            protocol::ItemComponents {
                wearable_slot: Some("slot.armor.head".into()),
                ..Default::default()
            },
        )],
        vec![],
    );
    runtime.set_session_items(Some(&items), None, Vec::new());
    let animation = EquipmentAnimation {
        owner: &owner,
        rig: &rig,
        frame_alpha: 0.5,
        delta_seconds: client_world::ACTOR_TICK_DURATION.as_secs_f32(),
    };
    let mut input = held("test:held");
    assert!(
        runtime
            .layers_for(&body, &input, Some(animation))
            .is_empty(),
        "a custom wearable must not draw its worn geometry in the hand"
    );
    input.armor[0] = input.main.take();
    assert_eq!(runtime.layers_for(&body, &input, Some(animation)).len(), 1);
    input.armor[1] = input.armor[0].take();
    assert!(
        runtime
            .layers_for(&body, &input, Some(animation))
            .is_empty(),
        "the model must not draw in another armor slot"
    );
}

#[test]
fn third_person_authored_offset_tracks_the_drawn_rotating_parent() {
    let (mut runtime, _) = pack_runtime(held_pack());
    let mut body = player_body(&mut runtime);
    let owner = owner();
    let names = ["rightItem".into()];
    let rig = owner_rig(&owner, &names);
    let hand = [
        "root",
        "body",
        "head",
        "rightArm",
        "leftArm",
        "rightItem",
        "leftItem",
    ]
    .iter()
    .position(|name| *name == "rightItem")
    .unwrap();
    let mut previous = body.input.previous_bones.to_vec();
    previous[hand].rotation = Quat::IDENTITY.to_array();
    previous[hand].translation_scale = [0.2, 0.8, -0.3, 1.0];
    let mut current = previous.clone();
    current[hand].rotation = Quat::from_rotation_z(1.5).to_array();
    current[hand].translation_scale = [0.6, 0.4, 0.2, 1.0];
    current[hand].axis_scale = [1.5, 0.75, 1.0, 1.0];
    body.input.previous_bones = previous.into();
    body.input.current_bones = current.into();
    let vertex = Vec3::new(0.2, 0.1, 0.05);
    let authored_vertex = vertex + Vec3::new(-2.0, 3.0, 4.0) / 16.0;
    let transform = |pose: render_model::RenderBoneTransform, vertex: Vec3| {
        Vec3::from_slice(&pose.translation_scale[..3])
            + Quat::from_array(pose.rotation)
                * (vertex * Vec3::from_slice(&pose.axis_scale[..3]) * pose.translation_scale[3])
    };
    for alpha in [0.25, 0.5, 0.75] {
        let layers = runtime.layers_for(
            &body,
            &held("test:held"),
            Some(EquipmentAnimation {
                owner: &owner,
                rig: &rig,
                frame_alpha: alpha,
                delta_seconds: client_world::ACTOR_TICK_DURATION.as_secs_f32(),
            }),
        );
        assert_eq!(layers.len(), 1);
        let equipment = &layers[0].submission.input;
        let drawn_item = transform(equipment.previous_bones[0], vertex)
            .lerp(transform(equipment.current_bones[0], vertex), alpha);
        let drawn_parent = transform(body.input.previous_bones[hand], authored_vertex).lerp(
            transform(body.input.current_bones[hand], authored_vertex),
            alpha,
        );
        assert!(
            drawn_item.abs_diff_eq(drawn_parent, 1e-5),
            "authored offset drifted from the drawn parent at {alpha}: {drawn_item:?} vs {drawn_parent:?}"
        );
    }
}
