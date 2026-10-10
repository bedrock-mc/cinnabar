use super::{local_carrier, pack_runtime, player_body};
use crate::presentation::equipment::runtime::*;
use bevy::math::{Quat, Vec3};
use client_world::{ActorLifetimeId, ActorRigSnapshot, WorldAuthority};
use protocol::{ActorEvent, ActorKind, ActorSpawnEvent, WorldBootstrap, WorldEvent};
use std::sync::Arc;

/// Builds a protocol-spawned skeleton with a distinct owner lifetime.
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

/// Supplies retained owner timing and named bones to equipment publication.
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

/// Creates an ordinary main-hand equipment input.
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

/// Compiles an original texture-mesh model with perspective-dependent placement.
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

/// Uses a controller-owned constant pose, as ordinary third-person held models do.
fn static_held_pack() -> Vec<(Box<str>, Vec<u8>)> {
    let mut files = held_pack();
    let description = String::from_utf8(files[0].1.clone()).unwrap().replace(
        "\"wield\":\"animation.held\"",
        "\"wield\":\"controller.animation.held\",\"pose\":\"animation.held\"",
    );
    files[0].1 = description.into_bytes();
    files.push((
        "animation_controllers/held.json".into(),
        br#"{"format_version":"1.10.0","animation_controllers":{
        "controller.animation.held":{"initial_state":"default","states":{"default":{
        "animations":["pose"]}}}}}"#
            .to_vec(),
    ));
    files
}

#[test]
fn warm_static_held_publication_retains_pose_without_evaluation_allocations() {
    let (mut runtime, _) = pack_runtime(static_held_pack());
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
    let mut rig = owner_rig(&owner, &names);
    let input = held("test:held");
    let mut batch = crate::presentation::actors::ActorPresentationBatch {
        submissions: vec![body],
        skin_layers: Vec::new(),
        artwork: Default::default(),
    };
    for tick in 0..3 {
        rig.completed_tick = tick;
        batch.submissions.truncate(1);
        batch.artwork.clear();
        crate::presentation::actors::attach_layers(&mut batch, &mut runtime, |runtime, body| {
            runtime.layers_for(
                body,
                &input,
                Some(EquipmentAnimation {
                    owner: &owner,
                    rig: &rig,
                    frame_alpha: 0.25,
                    delta_seconds: 0.016,
                }),
            )
        });
        assert_eq!(batch.submissions.len(), 2);
    }
    let allocations = crate::test_allocations::count();
    for tick in 3..13 {
        rig.completed_tick = tick;
        runtime.begin_frame();
        batch.submissions.truncate(1);
        batch.artwork.clear();
        crate::presentation::actors::attach_layers(&mut batch, &mut runtime, |runtime, body| {
            runtime.layers_for(
                body,
                &input,
                Some(EquipmentAnimation {
                    owner: &owner,
                    rig: &rig,
                    frame_alpha: 0.75,
                    delta_seconds: 0.016,
                }),
            )
        });
        assert_eq!(batch.submissions.len(), 2);
        assert_eq!(batch.artwork.len(), 1);
    }
    assert_eq!(
        crate::test_allocations::count() - allocations,
        0,
        "unchanged held publication must reuse evaluation, placement, and output storage"
    );
}

/// Replaces the constant channel while preserving controller-owned animation state.
fn controller_channel(expression: &str) -> Vec<(Box<str>, Vec<u8>)> {
    let mut files = static_held_pack();
    let mut clip: serde_json::Value = serde_json::from_slice(&files[2].1).unwrap();
    clip["animations"]["animation.held"]["bones"]["rightitem"]["position"] =
        serde_json::json!([expression, 0, 0]);
    files[2].1 = serde_json::to_vec(&clip).unwrap();
    files
}

/// Draws through the equipment route and returns the selected authored X channel.
fn authored_x(
    runtime: &mut EquipmentRuntime,
    body: &render::ActorRigSubmission,
    owner: &client_world::ActorSnapshot,
    rig: &ActorRigSnapshot<'_>,
    delta: f32,
) -> f32 {
    let layers = runtime.layers_for(
        body,
        &held("test:held"),
        Some(EquipmentAnimation {
            owner,
            rig,
            frame_alpha: 0.5,
            delta_seconds: delta,
        }),
    );
    assert_eq!(layers.len(), 1);
    -layers[0].submission.input.current_bones[0].translation_scale[0] * 16.0
}

#[test]
fn static_held_publication_invalidates_owner_target_and_lifetime() {
    let (mut runtime, _) = pack_runtime(controller_channel("query.has_target ? 6 : 2"));
    let body = player_body(&mut runtime);
    let mut owner = owner();
    let names = [Box::from("rightItem")];
    for target in [0, 0, 1, 1, 0, 1] {
        owner
            .metadata
            .insert(6, protocol::ActorMetadataValue::Long(target));
        let rig = owner_rig(&owner, &names);
        assert!(
            (authored_x(&mut runtime, &body, &owner, &rig, 0.016)
                - if target == 0 { 2.0 } else { 6.0 })
            .abs()
                < 1e-6
        );
    }
    let mut rig = owner_rig(&owner, &names);
    rig.actor.session_id += 1;
    owner
        .metadata
        .insert(6, protocol::ActorMetadataValue::Long(0));
    assert!((authored_x(&mut runtime, &body, &owner, &rig, 0.016) - 2.0).abs() < 1e-6);
}

#[test]
fn controller_held_publication_keeps_time_and_persistent_scripts_live() {
    let (mut runtime, _) = pack_runtime(controller_channel("query.delta_time"));
    let body = player_body(&mut runtime);
    let owner = owner();
    let names = [Box::from("rightItem")];
    let rig = owner_rig(&owner, &names);
    for delta in [0.01, 0.01, 0.01, 0.035, 0.02] {
        assert!((authored_x(&mut runtime, &body, &owner, &rig, delta) - delta).abs() < 1e-6);
    }
    let mut files = controller_channel("variable.draw_count");
    let mut document: serde_json::Value = serde_json::from_slice(&files[0].1).unwrap();
    document["minecraft:attachable"]["description"]["scripts"]["initialize"] =
        serde_json::json!(["variable.draw_count = 0;"]);
    document["minecraft:attachable"]["description"]["scripts"]["pre_animation"] =
        serde_json::json!(["variable.draw_count = variable.draw_count + 1;"]);
    files[0].1 = serde_json::to_vec(&document).unwrap();
    let (mut runtime, _) = pack_runtime(files);
    let body = player_body(&mut runtime);
    for count in 1..=5 {
        assert!((authored_x(&mut runtime, &body, &owner, &rig, 0.016) - count as f32).abs() < 1e-6);
    }
}

#[test]
fn cached_blocking_pose_preserves_conditionally_saved_owner_pitch() {
    let mut files = static_held_pack();
    let mut attachable: serde_json::Value = serde_json::from_slice(&files[0].1).unwrap();
    let description = &mut attachable["minecraft:attachable"]["description"];
    description["animations"]["released"] = serde_json::json!("animation.released");
    description["scripts"]["pre_animation"] = serde_json::json!([
        "query.blocking ? { variable.saved_pitch = query.head_x_rotation(0); } : 0;"
    ]);
    files[0].1 = serde_json::to_vec(&attachable).unwrap();
    let mut clips: serde_json::Value = serde_json::from_slice(&files[2].1).unwrap();
    clips["animations"]["animation.released"] = serde_json::json!({
        "loop": true, "bones": {"rightitem": {"position": ["variable.saved_pitch", 0, 0]}}
    });
    files[2].1 = serde_json::to_vec(&clips).unwrap();
    let controller = files
        .iter_mut()
        .find(|(name, _)| name.as_ref() == "animation_controllers/held.json")
        .unwrap();
    controller.1 = serde_json::to_vec(&serde_json::json!({
        "format_version": "1.10.0", "animation_controllers": {
            "controller.animation.held": {"initial_state": "default", "states": {
                "default": {"animations": [{"pose": "query.blocking"}, {"released": "!query.blocking"}]}
            }}
        }
    })).unwrap();
    let (mut runtime, _) = pack_runtime(files);
    let body = player_body(&mut runtime);
    let mut owner = owner();
    let names = [Box::from("rightItem")];
    owner
        .metadata
        .insert(92, protocol::ActorMetadataValue::FlagsExtended(1 << 8));
    for pitch in [10.0, 10.0, 30.0, 30.0] {
        owner.pitch = pitch;
        let rig = owner_rig(&owner, &names);
        assert!((authored_x(&mut runtime, &body, &owner, &rig, 0.016) - 2.0).abs() < 1e-6);
    }
    owner.pitch = 99.0;
    owner
        .metadata
        .insert(92, protocol::ActorMetadataValue::FlagsExtended(0));
    let rig = owner_rig(&owner, &names);
    assert!(
        (authored_x(&mut runtime, &body, &owner, &rig, 0.016) - 30.0).abs() < 1e-6,
        "release must use the last pitch saved during blocking"
    );
}

#[test]
fn third_person_bow_publication_tracks_use_frames_duration_and_release() {
    let mut files = held_pack();
    files[0].1 = String::from_utf8(files[0].1.clone())
        .unwrap()
        .replace("test:held", "minecraft:bow")
        .into_bytes();
    files[2].1 = br#"{"format_version":"1.10.0","animations":{
        "animation.held":{"loop":true,"bones":{"rightitem":{
        "position":["query.get_animation_frame","query.main_hand_item_use_duration",0]}}}}}"#
        .to_vec();
    let (mut runtime, _) = pack_runtime(files);
    let body = player_body(&mut runtime);
    let mut owner = owner();
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
    let input = held("minecraft:bow");
    let duration = 40;
    let items = crate::session_assets::SessionItems {
        components: Arc::new(
            [(
                Arc::from("minecraft:bow"),
                protocol::ItemComponents {
                    use_duration_ticks: Some(duration),
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
        ),
        icons: None,
    };
    runtime.set_session_items(Some(&items), None, Vec::new());
    for (elapsed, frame) in [
        (None, 0),
        (Some(0), 1),
        (Some(8), 1),
        (Some(9), 2),
        (Some(14), 2),
        (Some(15), 3),
        (Some(30), 3),
        (Some(39), 3),
        (Some(40), 0),
        (Some(41), 0),
        (None, 0),
    ] {
        owner.metadata.insert(
            0,
            protocol::ActorMetadataValue::Flags(if elapsed.is_some() { 1 << 4 } else { 0 }),
        );
        let mut rig = owner_rig(&owner, &names);
        rig.hand[1].use_ticks = elapsed.unwrap_or_default();
        let layers = runtime.layers_for(
            &body,
            &input,
            Some(EquipmentAnimation {
                owner: &owner,
                rig: &rig,
                frame_alpha: 0.5,
                delta_seconds: 0.016,
            }),
        );
        assert_eq!(layers.len(), 1);
        let bone = layers[0].submission.input.current_bones[0];
        assert!(
            (bone.translation_scale[0] + frame as f32 / 16.0).abs() < 1e-6,
            "elapsed {elapsed:?} must select authored bow frame {frame}"
        );
        let remaining = elapsed.map_or(0, |elapsed| duration.saturating_sub(elapsed));
        assert!(
            (bone.translation_scale[1] - remaining as f32 / 16.0).abs() < 1e-6,
            "elapsed {elapsed:?} must expose remaining duration {remaining}"
        );
    }
    let mut offhand = input.clone();
    offhand.off = offhand.main.take();
    offhand.main = Some(crate::presentation::equipment::WornItem {
        identifier: "minecraft:trident".into(),
        ..offhand.off.as_ref().unwrap().clone()
    });
    owner
        .metadata
        .insert(0, protocol::ActorMetadataValue::Flags(1 << 4));
    let mut rig = owner_rig(&owner, &names);
    rig.hand[1].use_ticks = 15;
    let layers = runtime.layers_for(
        &body,
        &offhand,
        Some(EquipmentAnimation {
            owner: &owner,
            rig: &rig,
            frame_alpha: 0.5,
            delta_seconds: 0.016,
        }),
    );
    assert_eq!(layers.len(), 1);
    let off_bone = layers[0].submission.input.current_bones[0];
    assert!(
        off_bone.translation_scale[0].abs() < 1e-6,
        "an offhand bow must stay on frame zero while the main hand is using another item"
    );
    assert!(
        (off_bone.translation_scale[1] - (inventory::LONG_WEAPON_USE_TICKS - 15) as f32 / 16.0)
            .abs()
            < 1e-3,
        "offhand models retain the owner's shared remaining-use query"
    );
    let mut both = input.clone();
    both.off = both.main.clone();
    let layers = runtime.layers_for(
        &body,
        &both,
        Some(EquipmentAnimation {
            owner: &owner,
            rig: &rig,
            frame_alpha: 0.5,
            delta_seconds: 0.016,
        }),
    );
    assert_eq!(layers.len(), 2);
    for layer in layers {
        assert!(
            (layer.submission.input.current_bones[0].translation_scale[0] + 3.0 / 16.0).abs()
                < 1e-6,
            "both held models must see the owner's main-hand animation frame"
        );
    }
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
    let initial_rig = layers[0].submission.input.rig;
    let initial_bones = Arc::clone(&layers[0].submission.input.current_bones);
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
    assert_eq!(repeated[0].submission.input.rig, initial_rig);
    assert!(
        Arc::ptr_eq(&repeated[0].submission.input.current_bones, &initial_bones),
        "unchanged poses retain their matrix-cache key"
    );
    assert!(
        runtime.take_pending_geometries().is_empty(),
        "unchanged held geometry stays resident"
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
fn installed_shield_and_trident_warm_draws_reuse_output_storage() {
    let (Some(entities), Some(icons), Some(equipment)) = (
        local_carrier("vanilla-v1.mcbeent"),
        local_carrier("vanilla-v1.mcbeico"),
        local_carrier("vanilla-v1.mcbeeqp"),
    ) else {
        return;
    };
    let (mut runtime, _, _) = EquipmentRuntime::build(
        Arc::new(assets::RuntimeEntityAssets::decode(&entities).unwrap()),
        Some(Arc::new(
            assets::RuntimeEquipmentCatalog::decode(&equipment).unwrap(),
        )),
        Arc::new(assets::RuntimeIconCatalog::decode(&icons).unwrap()),
        None,
        None,
        render::ActorArtworkPages::default(),
    );
    let body = player_body(&mut runtime);
    let owner = owner();
    let names = [Box::from("rightItem")];
    let mut rig = owner_rig(&owner, &names);
    for identifier in ["minecraft:shield", "minecraft:trident"] {
        let input = held(identifier);
        for _ in 0..3 {
            assert_eq!(
                runtime
                    .layers_for(
                        &body,
                        &input,
                        Some(EquipmentAnimation {
                            owner: &owner,
                            rig: &rig,
                            frame_alpha: 0.25,
                            delta_seconds: 0.016,
                        })
                    )
                    .len(),
                1
            );
        }
        let allocations = crate::test_allocations::count();
        for tick in 2..12 {
            rig.completed_tick = tick;
            runtime.begin_frame();
            assert_eq!(
                runtime
                    .layers_for(
                        &body,
                        &input,
                        Some(EquipmentAnimation {
                            owner: &owner,
                            rig: &rig,
                            frame_alpha: 0.75,
                            delta_seconds: 0.016,
                        })
                    )
                    .len(),
                1
            );
        }
        assert_eq!(
            crate::test_allocations::count() - allocations,
            0,
            "unchanged {identifier} publication must retain evaluation and output storage"
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
    let mut owner = owner();
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
    let mut rig = owner_rig(&owner, &names);
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
    let standby = layers[0].submission.input.rig;
    let duration = *runtime
        .item_use_durations()
        .get("minecraft:bow")
        .expect("the installed bow must retain its authored use duration");
    assert!(duration > 15);
    let mut stages = Vec::new();
    for (elapsed, pulling) in [(0, true), (9, true), (15, true), (duration, false)] {
        owner
            .metadata
            .insert(0, protocol::ActorMetadataValue::Flags(1 << 4));
        rig.hand[1].use_ticks = elapsed;
        let layers = runtime.layers_for(
            &body,
            &input,
            Some(EquipmentAnimation {
                owner: &owner,
                rig: &rig,
                frame_alpha: 1.0,
                delta_seconds: 0.016,
            }),
        );
        assert_eq!(layers.len(), 1);
        let selected = layers[0].submission.input.rig;
        if pulling {
            assert_ne!(
                selected, standby,
                "elapsed {elapsed} must draw a pulling texture mesh"
            );
            assert!(
                !stages.contains(&selected),
                "each draw stage must select its authored texture"
            );
            stages.push(selected);
        } else {
            assert_eq!(
                selected, standby,
                "a completed use counter must return the standby mesh"
            );
        }
    }
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
    let (mut runtime, _) = pack_runtime(static_held_pack());
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

#[test]
fn animated_held_attachables_skip_the_hurt_overlay_in_both_views() {
    let (mut runtime, _) = pack_runtime(held_pack());
    let mut body = player_body(&mut runtime);
    body.overlay_rgba8 = 0x6600_00ff;
    let owner = owner();
    let names = [Box::from("rightItem"), Box::from("leftItem")];
    let rig = owner_rig(&owner, &names);
    let input = held("test:held");
    let layers = runtime.layers_for(
        &body,
        &input,
        Some(EquipmentAnimation {
            owner: &owner,
            rig: &rig,
            frame_alpha: 0.5,
            delta_seconds: 0.016,
        }),
    );
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].submission.overlay_rgba8, 0);
    for off_hand in [false, true] {
        let item = runtime
            .first_person_attachable(
                &body,
                input.main.as_ref().unwrap(),
                &owner,
                &rig,
                client_world::AttachableAnimationInput {
                    first_person: true,
                    off_hand,
                    ..Default::default()
                },
                None,
            )
            .unwrap();
        assert_eq!(item.presentation.submission.overlay_rgba8, 0);
    }
    assert_eq!(body.overlay_rgba8, 0x6600_00ff);
}
