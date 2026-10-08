use super::*;
use bevy::math::Mat4;

/// Builds the canonical six-part player fixture used by pose publication tests.
fn head_assets() -> Arc<assets::RuntimeEntityAssets> {
    let entity = br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:player","materials":{"default":"entity"},"textures":{"default":"textures/entity/test"},"geometry":{"default":"geometry.test"},"render_controllers":["controller.render.test"]}}}"#;
    let geometry = br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.test","texture_width":64,"texture_height":64},"bones":[{"name":"head","pivot":[0,24,0]},{"name":"body","pivot":[0,24,0]},{"name":"rightarm","pivot":[5,22,0]},{"name":"leftarm","pivot":[-5,22,0]},{"name":"rightleg","pivot":[1.9,12,0]},{"name":"leftleg","pivot":[-1.9,12,0]}]}]}"#;
    let controller = br#"{"format_version":"1.8.0","render_controllers":{"controller.render.test":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}"#;
    let compiled = pack_compiler::compile_entity_pack(vec![
        ("entity/player.json".into(), entity.to_vec()),
        ("models/entity/test.geo.json".into(), geometry.to_vec()),
        ("render_controllers/test.json".into(), controller.to_vec()),
    ])
    .unwrap()
    .unwrap();
    Arc::new(assets::RuntimeEntityAssets::from_compiled(compiled.assets).unwrap())
}

/// Creates a local player stream with the fixture player assets.
pub(super) fn head_stream() -> WorldStream {
    WorldStream::new_with_asset_sets(
        protocol::WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: [0.0, 64.0, 0.0],
            world_spawn_position: [0, 64, 0],
            air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        },
        Arc::new(assets::RuntimeAssets::diagnostic()),
        head_assets(),
        [0.0, 64.0, 0.0],
        None,
    )
}

/// Starts the local player with a stationary, empty-handed pose.
pub(super) fn head_feed() -> client_world::LocalPlayerFeed {
    client_world::LocalPlayerFeed {
        prefer_client_skin: false,
        uuid: [1; 16],
        username: Arc::from("Player"),
        skin: protocol::PlayerSkin::Unavailable(protocol::PlayerSkinUnavailable::InvalidDimensions),
        position: [0.0, 64.0, 0.0],
        velocity: [0.0; 3],
        on_ground: true,
        yaw: 170.0,
        head_yaw: 170.0,
        pitch: 5.0,
        main_hand: None,
        off_hand: None,
        main_hand_metadata: 0,
        main_hand_slot: 0,
        main_hand_stack_id: None,
        bedrock_swing_ticks: client_world::ACTOR_SWING_TICKS,
        java_swing_ticks: client_world::ACTOR_SWING_TICKS,
        flying: false,
        gliding: false,
        fall_fly_ticks: 0,
        teleported: false,
        first_person: false,
        view_bobbing: true,
        sneaking: false,
        sprinting: false,
        item_use: client_world::LocalItemUse::Unpredicted,
    }
}

/// Adds uploaded player geometry and optional animated face layers.
pub(super) fn uploaded_skin_feed(animated: bool) -> client_world::LocalPlayerFeed {
    let mut feed = head_feed();
    let geometry = r#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.uploaded","texture_width":64,"texture_height":64},"bones":[{"name":"head","pivot":[0,24,0]},{"name":"body","pivot":[0,24,0]},{"name":"rightarm","pivot":[4,22,0]},{"name":"leftarm","pivot":[-4,22,0]},{"name":"rightleg","pivot":[1.9,12,0]},{"name":"leftleg","pivot":[-1.9,12,0]}]}]}"#;
    let image = protocol::SkinAnimation {
        kind: protocol::SkinAnimationKind::Face,
        width: 64,
        height: 64,
        rgba8: vec![255; 64 * 64 * 4].into(),
        frames: 1,
        blinking: false,
    };
    feed.skin = protocol::PlayerSkin::Standard(protocol::StandardSkin {
        width: 64,
        height: 64,
        rgba8: vec![255; 64 * 64 * 4].into(),
        cape: None,
        geometry: Some(Arc::new(protocol::SkinGeometrySource {
            resource_patch: Arc::from(
                r#"{"geometry":{"default":"geometry.uploaded","animated_face":"geometry.uploaded"}}"#,
            ),
            geometry_data: Arc::from(geometry),
            animations: if animated {
                Arc::from([image])
            } else {
                Arc::from([])
            },
        })),
    });
    feed
}

#[test]
fn third_person_keeps_authored_pack_poses_and_accepts_uploaded_skin_geometry() {
    let mut stream = head_stream();
    stream.sync_local_player_pose(&uploaded_skin_feed(false));
    stream.prepare_actor_appearance_fixture();
    stream.advance_actor_interpolation_frame(1);
    let rig = stream.authority().actor_rig(1).unwrap();
    let actor = stream.authority().actor(1).unwrap();
    let equipment = ActorEquipmentInput::default();
    assert!(
        rig.skin_geometry.is_some(),
        "the uploaded model must be parsed"
    );
    assert!(!render_model::is_pack_rig_id(render_model::EntityRigId(
        rig.rig.0
    )));
    assert!(third_person(&stream, &rig, actor, Some(&equipment), 0.5).is_some());
    let authored = ActorRigSnapshot {
        rig: client_world::EntityRigId(assets::PACK_RIG_ID_BASE),
        ..rig
    };
    assert!(third_person(&stream, &authored, actor, Some(&equipment), 0.5).is_none());
}

#[test]
fn published_persona_layers_follow_the_sampled_local_emote_only() {
    let mut stream = head_stream();
    stream.sync_local_player_pose(&uploaded_skin_feed(true));
    stream.prepare_actor_appearance_fixture();
    stream.advance_actor_interpolation_frame(1);
    let rig = stream.authority().actor_rig(1).unwrap();
    assert_eq!(rig.skin_layers.len(), 1);
    let native_layer = &rig.skin_layers[0];
    let native_pose = native_layer.current.clone();
    let emote = client_world::sample_custom_emote(
        &rig,
        client_world::CustomEmote::Twerk,
        0.0,
        client_world::CustomEmote::Twerk.duration_seconds() / 4.0,
    )
    .unwrap();
    assert_ne!(emote.skin_layers[0].current, native_pose);
    let published =
        super::super::emote_geometry::skin_layer_snapshot(rig, Some(&emote), None, None);
    assert_eq!(published.current, emote.current.as_ref());
    assert_eq!(
        published.skin_layers[0].current,
        emote.skin_layers[0].current
    );
    assert_eq!(
        published.skin_layers[0].previous,
        emote.skin_layers[0].previous
    );
    assert_eq!(published.skin_layers[0].image, native_layer.image);
    assert_eq!(published.skin_layers[0].uv_anim, native_layer.uv_anim);
    assert_eq!(native_layer.current, native_pose);
    let unchanged = super::super::emote_geometry::skin_layer_snapshot(rig, None, None, None);
    assert_eq!(unchanged.skin_layers[0].current, native_pose);
}

#[test]
fn local_head_tracks_between_tick_look_while_moving_without_committing_a_pose() {
    let mut stream = head_stream();
    let mut feed = head_feed();
    stream.sync_local_player_pose(&feed);
    stream.prepare_actor_appearance_fixture();
    stream.advance_actor_interpolation_frame(1);
    feed.position[0] += 0.2;
    feed.velocity[0] = 0.2;
    feed.head_yaw = -170.0;
    feed.yaw = -170.0;
    stream.sync_local_player_pose(&feed);
    stream.prepare_actor_appearance_fixture();
    stream.advance_actor_interpolation_frame(1);
    let rig = stream.authority().actor_rig(1).unwrap();
    let original_motion = rig.java;
    let original_tick = rig.completed_tick;
    let original_pose = rig.current.to_vec();
    let tick_head = stream.authority().actor(1).unwrap().head_yaw;
    for (pitch, head_yaw) in [(20.0, -165.0), (-30.0, 179.0), (40.0, -179.0)] {
        feed.pitch = pitch;
        feed.head_yaw = head_yaw;
        feed.yaw = head_yaw;
        stream.sync_local_player_pose(&feed);
        stream.prepare_actor_appearance_fixture();
        stream.advance_actor_interpolation_frame(0);
        let actor = stream.authority().actor(1).unwrap();
        let original_actor = actor.clone();
        let rig = stream.authority().actor_rig(1).unwrap();
        let rig = ActorRigSnapshot {
            previous_body_yaw: rig.java.body_yaw[0],
            body_yaw: rig.java.body_yaw[1],
            ..rig
        };
        for alpha in [0.0, 0.25, 0.75, 1.0] {
            let input = third_person_input(&rig, actor, None, alpha, true);
            let body = lerp_degrees(rig.java.body_yaw[0], rig.java.body_yaw[1], alpha);
            assert_eq!(input.head_pitch, pitch);
            assert!(wrap_degrees(input.head_yaw + body - head_yaw).abs() < 1e-4);
            let posed = java::java_biped(&input);
            let expected = java::java_biped(&JavaBipedInput {
                head_pitch: pitch,
                head_yaw: wrap_degrees(head_yaw - body),
                ..input
            });
            assert_eq!(posed.head, expected.head);
        }
        assert_eq!(*actor, original_actor);
        assert_eq!(actor.head_yaw, tick_head);
        assert_eq!(actor.pitch, 5.0);
        assert_eq!(rig.java, original_motion);
        assert_eq!(rig.completed_tick, original_tick);
        assert_eq!(rig.current, original_pose.as_slice());
    }
}

#[test]
fn nonlocal_head_keeps_tick_interpolation_and_takes_the_short_yaw_path() {
    let mut stream = head_stream();
    stream.sync_local_player_pose(&head_feed());
    stream.prepare_actor_appearance_fixture();
    stream.advance_actor_interpolation_frame(1);
    let mut actor = stream.authority().actor(1).unwrap().clone();
    actor.previous_pose.head_yaw = 179.0;
    actor.head_yaw = -179.0;
    actor.previous_pose.pitch = 10.0;
    actor.pitch = 30.0;
    actor.received_pose.head_yaw = 25.0;
    actor.received_pose.pitch = -40.0;
    let rig = stream.authority().actor_rig(1).unwrap();
    let rig = ActorRigSnapshot {
        previous_body_yaw: rig.java.body_yaw[0],
        body_yaw: rig.java.body_yaw[1],
        ..rig
    };
    for alpha in [0.0, 0.25, 0.75, 1.0] {
        let input = third_person_input(&rig, &actor, None, alpha, false);
        let body = lerp_degrees(rig.java.body_yaw[0], rig.java.body_yaw[1], alpha);
        assert_eq!(input.head_pitch, 10.0 + 20.0 * alpha);
        assert!(wrap_degrees(input.head_yaw + body - (179.0 + 2.0 * alpha)).abs() < 1e-4);
    }
}

/// Draw and eat timing count from the first using tick, a tick behind the frame.
#[test]
fn use_timing_maps_java_counts() {
    assert_eq!(
        java_use("minecraft:apple", Some("minecraft:apple"), 0, Some(32), 0.5),
        None
    );
    assert_eq!(
        java_use("minecraft:bow", Some("minecraft:bow"), 1, None, 0.25),
        Some(JavaUse::Bow { pull: -0.75 })
    );
    assert_eq!(
        java_use("minecraft:apple", Some("minecraft:apple"), 3, Some(32), 0.5),
        Some(JavaUse::Consume {
            remaining: 30.5,
            duration: 32.0
        })
    );
    assert_eq!(
        java_use(
            "minecraft:iron_sword",
            Some("minecraft:iron_sword"),
            1,
            None,
            0.0
        ),
        Some(JavaUse::Block)
    );
    assert_eq!(
        java_use("minecraft:stick", Some("minecraft:stick"), 4, None, 0.0),
        None
    );
}

#[test]
fn retained_item_does_not_use_the_selected_items_clock() {
    for (retained, selected, consume) in [
        ("minecraft:bow", Some("minecraft:apple"), Some(32)),
        ("minecraft:apple", Some("minecraft:bow"), None),
        ("minecraft:iron_sword", Some("minecraft:apple"), Some(32)),
        ("minecraft:bow", None, None),
    ] {
        assert_eq!(java_use(retained, selected, 4, consume, 0.5), None);
    }
}

#[test]
fn native_and_modern_items_stay_retained_until_equip_adopts_them() {
    let item = |identifier: &str| WornItem {
        identifier: identifier.into(),
        metadata: 0,
        damage: None,
        kind: crate::presentation::equipment::HeldKind::Sprite,
        dye_rgb: None,
        enchanted: false,
    };
    for modern in [FILLED_MAP, "minecraft:crossbow", "minecraft:shield"] {
        let sword = item("minecraft:iron_sword");
        let modern = item(modern);
        for (old, new) in [(&sword, &modern), (&modern, &sword)] {
            let cache = HandCache {
                shown: Some(old.clone()),
                ..Default::default()
            };
            let outgoing = cache.displayed_main(Some(&held(old)), Some(new)).unwrap();
            assert_eq!(outgoing.identifier, old.identifier);
            let incoming = cache.displayed_main(Some(&held(new)), Some(new)).unwrap();
            assert_eq!(incoming.identifier, new.identifier);
            assert!(cache.displayed_main(None, Some(new)).is_none());
        }
    }
}

#[test]
fn outgoing_main_use_is_idle_while_offhand_keeps_the_actual_owner_use() {
    let item = |identifier: &str| WornItem {
        identifier: identifier.into(),
        metadata: 0,
        damage: None,
        kind: crate::presentation::equipment::HeldKind::Sprite,
        dye_rgb: None,
        enchanted: false,
    };
    let owner = ActorEquipmentInput {
        main: Some(item("minecraft:apple")),
        off: Some(item("minecraft:shield")),
        ..Default::default()
    };
    let rendered = ActorEquipmentInput {
        main: Some(item(BOW)),
        ..owner.clone()
    };
    let timing = client_world::AttachableAnimationInput {
        first_person: true,
        frame_alpha: 0.75,
        animation_frame: 3,
        use_elapsed_ticks: Some(19),
        max_use_ticks: 32,
        hand_charged: true,
        ..Default::default()
    };
    let outgoing = super::super::hand::attachable_hand_input(&rendered, &owner, timing, false);
    assert_eq!(outgoing.animation_frame, 0);
    assert_eq!(outgoing.use_elapsed_ticks, None);
    assert!(!outgoing.hand_charged);
    assert!(outgoing.first_person);
    assert_eq!(outgoing.frame_alpha, timing.frame_alpha);
    assert_eq!(outgoing.owner_main_hand, Some(BOW));
    let off = super::super::hand::attachable_hand_input(&rendered, &owner, timing, true);
    assert_eq!(off.use_elapsed_ticks, timing.use_elapsed_ticks);
    assert_eq!(off.owner_main_hand, Some("minecraft:apple"));
    assert_eq!(off.owner_off_hand, Some("minecraft:shield"));
    assert_eq!(off.animation_frame, 0);
    assert!(!off.hand_charged);
    let adopted = super::super::hand::attachable_hand_input(&owner, &owner, timing, false);
    assert_eq!(adopted.animation_frame, timing.animation_frame);
    assert_eq!(adopted.use_elapsed_ticks, timing.use_elapsed_ticks);
    assert!(adopted.hand_charged);
}

#[test]
fn mixed_native_map_swaps_publish_the_outgoing_mesh_until_adoption() {
    let ids = ["minecraft:iron_sword", FILLED_MAP];
    let sprites = [100, 200].map(|value| assets::IconSprite {
        width: 16,
        height: 16,
        rgba8: std::iter::repeat_n([value, 255, 255, 255], 16 * 16)
            .flatten()
            .collect::<Vec<_>>()
            .into(),
    });
    let mut entries = ids
        .iter()
        .enumerate()
        .map(|(index, id)| assets::IconEntry {
            identifier: (*id).into(),
            metadata: 0,
            sprite: index as u32,
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.identifier.cmp(&right.identifier));
    let icons = Arc::new(
        assets::RuntimeIconCatalog::decode(
            &assets::encode_icon_catalog([0; 32], &sprites, &entries).unwrap(),
        )
        .unwrap(),
    );
    let item = |identifier: &str| WornItem {
        identifier: identifier.into(),
        metadata: 0,
        damage: None,
        kind: crate::presentation::equipment::HeldKind::Sprite,
        dye_rgb: None,
        enchanted: false,
    };
    for (old, new) in [(ids[0], ids[1]), (ids[1], ids[0])] {
        let mut stream = head_stream();
        let mut feed = uploaded_skin_feed(false);
        feed.first_person = true;
        feed.main_hand = Some(old.into());
        stream.sync_local_player_pose(&feed);
        stream.prepare_actor_appearance_fixture();
        stream.advance_actor_interpolation_frame(6);
        let (mut equipment, artwork, _) = EquipmentRuntime::build(
            head_assets(),
            None,
            Arc::clone(&icons),
            None,
            None,
            render::ActorArtworkPages::default(),
        );
        let rig = stream.authority().actor_rig(1).unwrap();
        equipment.register_skin_rig(
            render_model::EntityRigId(rig.rig.0),
            rig.bone_names.to_vec(),
        );
        let mut cache = HandCache::default();
        cache.remember(&rig, Some(&item(old)));
        let actor = stream.authority().actor(1).unwrap();
        let presentation =
            crate::presentation::actors::entity_rig_presentation(&rig, actor, &artwork, 0.5)
                .unwrap();
        let expected = equipment
            .first_person_java_item(
                &presentation.submission,
                &item(old),
                JavaHand {
                    swing: 0.0,
                    equip: 1.0,
                    using: None,
                },
            )
            .unwrap()
            .presentation
            .submission
            .input
            .rig;
        feed.main_hand = Some(new.into());
        stream.sync_local_player_pose(&feed);
        stream.prepare_actor_appearance_fixture();
        let selected = ActorEquipmentInput {
            main: Some(item(new)),
            ..Default::default()
        };
        for tick in 1..=3 {
            stream.advance_actor_interpolation_frame(1);
            let rig = stream.authority().actor_rig(1).unwrap();
            cache.remember(&rig, selected.main.as_ref());
            let actor = stream.authority().actor(1).unwrap();
            let presentation =
                crate::presentation::actors::entity_rig_presentation(&rig, actor, &artwork, 0.5)
                    .unwrap();
            let source = hand_source(
                HandInputs {
                    stream: &stream,
                    presentation,
                    equipment_input: &selected,
                    owner_equipment: &selected,
                    consume_ticks: None,
                    item_animation: None,
                    alpha: 0.5,
                    artwork: &artwork,
                    motion: Mat4::IDENTITY,
                    sampling_camera: None,
                },
                &mut equipment,
                &mut cache,
            )
            .expect("the retained item must publish through a mixed swap");
            let drawn = source.items[0]
                .as_ref()
                .unwrap()
                .0
                .presentation
                .submission
                .input
                .rig;
            if tick < 3 {
                assert_eq!(drawn, expected, "{old} → {new} before adoption");
            } else {
                assert_ne!(drawn, expected, "{old} → {new} after adoption");
            }
        }
    }
}

#[test]
fn bow_frames_follow_java_draw_thresholds() {
    let frames = [0, 1, 2, 14, 15, 18, 19, 40].map(java_bow_frame);
    assert_eq!(frames, [0, 0, 1, 1, 2, 2, 3, 3]);
}

/// A head-only skin layer takes the head target and skips the parts it lacks.
#[test]
fn partial_layers_skip_missing_parts() {
    let names = [Box::from("head")];
    let rest = [BoneTransform {
        rotation: [0.0, 0.0, 0.0, 1.0],
        translation_scale: [0.0, 24.0, 0.0, 1.0],
        axis_scale: [1.0; 3],
    }];
    let pose = java::java_biped(&JavaBipedInput::default());
    let parts = [0, 1, 2, 3, 4, 5];
    assert!(targets(&names, &rest, &pose, &parts, true).unwrap()[0].is_some());
    assert!(targets(&names, &rest, &pose, &parts, false).is_none());
}

/// Builds a sprite stack for the hand transition tests.
fn worn(identifier: &str) -> WornItem {
    WornItem {
        identifier: Arc::from(identifier),
        metadata: 0,
        damage: None,
        kind: crate::presentation::equipment::HeldKind::Sprite,
        dye_rgb: None,
        enchanted: false,
    }
}

/// Extracts the identity that the equip animation retains.
fn held(item: &WornItem) -> JavaHeldItem {
    JavaHeldItem {
        identifier: Arc::clone(&item.identifier),
        metadata: item.damage.unwrap_or(item.metadata),
        stack_id: None,
    }
}

/// Between a Java item and one only vanilla draws, the old item stays in its own hand
/// until the dip's bottom, in both directions.
#[test]
fn mixed_swaps_change_hands_at_the_bottom_of_the_dip() {
    const CROSSBOW: &str = "minecraft:crossbow";
    const SHIELD: &str = "minecraft:shield";
    let vanilla = |identifier: &str| matches!(identifier, CROSSBOW | SHIELD);
    let id = |item: Option<WornItem>| item.map(|item| item.identifier);
    for (old, new) in [
        ("minecraft:iron_sword", CROSSBOW),
        (CROSSBOW, "minecraft:iron_sword"),
        ("minecraft:iron_sword", SHIELD),
        (SHIELD, "minecraft:iron_sword"),
    ] {
        let (old, new) = (worn(old), worn(new));
        let cache = HandCache {
            shown: Some(old.clone()),
            ..HandCache::default()
        };
        let dipping = cache.displayed_main(Some(&held(&old)), Some(&new));
        assert_eq!(
            vanilla_draws(dipping.as_ref(), None, vanilla),
            vanilla(&old.identifier)
        );
        assert_eq!(id(dipping), Some(Arc::clone(&old.identifier)));
        let adopted = cache.displayed_main(Some(&held(&new)), Some(&new));
        assert_eq!(
            vanilla_draws(adopted.as_ref(), None, vanilla),
            vanilla(&new.identifier)
        );
        assert_eq!(id(adopted), Some(Arc::clone(&new.identifier)));
    }
}

/// Two data values of one item are different stacks to the dip.
#[test]
fn displayed_stack_keeps_its_data_value_through_the_dip() {
    let water = worn("minecraft:potion");
    let healing = WornItem {
        metadata: 21,
        ..worn("minecraft:potion")
    };
    let cache = HandCache {
        shown: Some(water.clone()),
        ..HandCache::default()
    };
    let shown = cache.displayed_main(Some(&held(&water)), Some(&healing));
    assert_eq!(shown.map(|item| item.metadata), Some(0));
}

/// Java's body reaches its side during the fourteenth tick, while the native curve is still falling.
#[test]
fn death_body_reaches_java_angle_without_moving_its_feet() {
    use crate::presentation::actors::death_tilted;
    let base = [
        [1.0, 0.0, 0.0, 3.0],
        [0.0, 1.0, 0.0, 64.0],
        [0.0, 0.0, 1.0, 5.0],
    ];
    for ticks in [1.0_f32, 5.5, 13.5, 20.0] {
        let native = ticks / f32::from(client_world::DEATH_DURATION_TICKS);
        let rows = death_tilt(death_tilted(base, Some(native)), native, ticks);
        let angle = (((ticks - 1.0) / 20.0 * 1.6).sqrt().min(1.0) * 90.0).to_radians();
        assert!((rows[0][0] - angle.cos()).abs() < 1e-6);
        assert!((rows[1][0] - angle.sin()).abs() < 1e-6);
        assert_eq!([rows[0][3], rows[1][3], rows[2][3]], [3.0, 64.0, 5.0]);
    }
}

#[test]
fn third_person_lift_and_sneak_drop() {
    let mut rows = [
        [-1.0, 0.0, 0.0, 5.0],
        [0.0, 0.9375, 0.0, 64.0],
        [0.0, 0.0, -1.0, 2.0],
    ];
    lift(&mut rows, true, false);
    assert!((rows[1][3] - (64.0 + 0.9375 / 128.0 - 0.125)).abs() < 1e-6);
    assert_eq!(rows[0][3], 5.0);
}

/// Local root reconstruction retains Java's faster death curve and leaves native output unchanged.
#[test]
fn local_death_root_reapplies_java_tilt_and_preserves_native_mode() {
    let base = [
        [1.0, 0.0, 0.0, 3.0],
        [0.0, 1.0, 0.0, 64.0],
        [0.0, 0.0, 1.0, 5.0],
    ];
    let ticks = 13.5;
    let progress = ticks / f32::from(client_world::DEATH_DURATION_TICKS);
    let native = crate::presentation::actors::death_tilted(base, Some(progress));
    assert_eq!(local_death_tilt(base, Some(progress), None), native);
    let java = local_death_tilt(base, Some(progress), Some(ticks));
    assert!(java[0][0].abs() < 1e-5);
    assert!((java[1][0] - 1.0).abs() < 1e-5);
    assert!((java[0][1] + 1.0).abs() < 1e-5);
    assert_eq!([java[0][3], java[1][3], java[2][3]], [3.0, 64.0, 5.0]);
    assert_ne!(java, native);
    assert_eq!(local_death_tilt(base, None, Some(ticks)), base);
}

/// A retained potion does not borrow use timing from a newly selected potion metadata value.
#[test]
fn outgoing_metadata_variant_keeps_its_own_idle_use_clock() {
    let owner = ActorEquipmentInput {
        main: Some(WornItem {
            metadata: 21,
            ..worn("minecraft:potion")
        }),
        ..Default::default()
    };
    let rendered = ActorEquipmentInput {
        main: Some(worn("minecraft:potion")),
        ..owner.clone()
    };
    let timing = client_world::AttachableAnimationInput {
        animation_frame: 3,
        use_elapsed_ticks: Some(19),
        max_use_ticks: 32,
        hand_charged: true,
        ..Default::default()
    };
    let outgoing = super::super::hand::attachable_hand_input(&rendered, &owner, timing, false);
    assert_eq!(outgoing.use_elapsed_ticks, None);
    assert_eq!(outgoing.animation_frame, 0);
    let off = super::super::hand::attachable_hand_input(&rendered, &owner, timing, true);
    assert_eq!(off.use_elapsed_ticks, timing.use_elapsed_ticks);
}
