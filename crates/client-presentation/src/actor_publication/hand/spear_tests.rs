//! Real spear controllers keep their owner's authored pose under the default animation setting.
use super::*;
use bevy::ecs::system::SystemState;
use bevy::math::{EulerRot, Quat};
use bevy::prelude::{PerspectiveProjection, World};

/// Reads a spear fixture, naming missing carriers while rejecting other I/O errors.
fn installed_carrier(path: &std::path::Path) -> Option<Vec<u8>> {
    match std::fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "skipping installed_spear_charge_keeps_the_authored_third_person_arm_with_java_enabled: missing fixture {}; run make assets",
                path.display()
            );
            None
        }
        Err(error) => panic!("read {}: {error}", path.display()),
    }
}

#[test]
fn installed_spear_charge_keeps_the_authored_third_person_arm_with_java_enabled() {
    let configured = std::env::var_os("CINNABAR_ENTITY_ASSETS");
    let path = configured.map(std::path::PathBuf::from).unwrap_or_else(|| {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(assets::carriers::COMPILED_DIR)
            .join(assets::carriers::ENTITY.output)
    });
    let Some(bytes) = installed_carrier(&path) else {
        return;
    };
    let Some(equipment) =
        installed_carrier(&path.with_file_name(assets::carriers::EQUIPMENT.output))
    else {
        return;
    };
    let entities = Arc::new(assets::RuntimeEntityAssets::decode(&bytes).unwrap());
    let equipment = Arc::new(assets::RuntimeEquipmentCatalog::decode(&equipment).unwrap());
    let icons = Arc::new(
        assets::RuntimeIconCatalog::decode(
            &assets::encode_icon_catalog(entities.source_manifest_sha256(), &[], &[]).unwrap(),
        )
        .unwrap(),
    );
    let (equipment, artwork, _) = EquipmentRuntime::build(
        Arc::clone(&entities),
        Some(equipment),
        icons,
        None,
        None,
        render_model::ActorArtworkPages::default(),
    );
    let timings = equipment.item_attack_timings();
    let timing = timings
        .get("minecraft:iron_spear")
        .expect("installed spear facts");
    assert!(timing.is_spear);
    let delay = timing
        .kinetic_weapon
        .expect("spear kinetic timing")
        .delay_ticks;
    let feet = [0.0, 64.0, 0.0];
    let mut stream = WorldStream::new_with_asset_sets(
        protocol::WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: feet,
            world_spawn_position: [0, 64, 0],
            air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        },
        Arc::new(assets::RuntimeAssets::diagnostic()),
        Arc::clone(&entities),
        feet,
        None,
    );
    stream.set_item_use_durations(equipment.item_use_durations());
    stream.set_item_attack_timings(timings);
    let mut feed = super::native_tests::player_feed();
    feed.position = feet;
    feed.first_person = false;
    feed.yaw = 0.0;
    feed.head_yaw = 0.0;
    feed.pitch = 0.0;
    feed.main_hand = Some("minecraft:iron_spear".into());
    feed.held_network_ids[0] = protocol::vanilla_item_registry()
        .iter()
        .find(|entry| entry.identifier.as_ref() == feed.main_hand.as_deref().unwrap())
        .expect("registered spear fixture")
        .network_id;
    let use_facts = stream
        .authority()
        .item_use_profile(feed.held_network_ids[0]);
    assert!(use_facts.ticks.is_some_and(|ticks| ticks > delay + 2));
    feed.item_use = client_world::LocalItemUse::Using;
    stream.sync_local_player_pose(&feed);
    stream.prepare_actor_appearance_fixture();
    stream.advance_actor_interpolation_frame(delay + 2);
    let input = ActorEquipmentInput {
        main: Some(crate::presentation::equipment::WornItem {
            variant: Default::default(),
            charged_projectile: None,
            use_facts,
            grip: assets::item::grip::builtin_item_grip("minecraft:iron_spear"),
            identifier: "minecraft:iron_spear".into(),
            kind: crate::presentation::equipment::HeldKind::Sprite,
            metadata: 0,
            damage: None,
            dye_rgb: None,
            enchanted: false,
            glint: Default::default(),
        }),
        ..Default::default()
    };
    let mut world = World::new();
    let mut time = Time::<Real>::default();
    time.advance_by(simulation_clock::TICK_DURATION);
    world.insert_resource(time);
    world.insert_resource(equipment);
    world.insert_resource(render_scene::ActorArtworkResource(artwork));
    world.insert_resource(
        render_scene::ActorRenderScene::with_runtime_entity_assets(&entities).unwrap(),
    );
    world.insert_resource(super::super::HandRigBuilder::from_runtime_assets(&entities).unwrap());
    world.init_resource::<super::super::ActorFrameState>();
    world.init_resource::<super::super::PreparedActorPublication>();
    world.init_resource::<super::super::ActorFramePartialTick>();
    world.init_resource::<render_scene::HandRigScene>();
    let mut avatar = crate::local_player::LocalAvatarPresentation::default();
    avatar.begin_session(
        simulation_clock::identity::SessionGeneration::new(stream.authority().actor_session_id()),
        1,
    );
    world.insert_resource(avatar);
    world.init_resource::<crate::local_player::LocalAvatarVisibilityCarrier>();
    let mut settings = crate::camera::CameraSettingsAuthority::default();
    let mut user = ui::UserSettings::default();
    user.video.java_animations = true;
    user.gameplay.default_perspective = semantic_input::PerspectiveMode::ThirdPersonBack;
    settings.replace(1, &user).unwrap();
    world.insert_resource(settings);
    world.insert_resource(crate::local_player::LocalViewPose::default());
    world.spawn((
        Transform::default(),
        Projection::Perspective(PerspectiveProjection::default()),
        crate::camera::FlyCamera::default(),
    ));
    let mut params = SystemState::<super::super::ActorFramePublication>::new(&mut world);
    let mut prepared_artwork = None;
    super::super::advance_actor_frame(
        super::super::ActorWorld {
            stream: Some(&mut stream),
            collisions: None,
            entity_assets: Some(&entities),
            pack_entities: None,
            session_items: None,
            prepared_actor_artwork: &mut prepared_artwork,
        },
        super::super::ActorFrameInput {
            cosmetics: Vec::new(),
            local_feed: None,
            predicted_eye: Some([feet[0], feet[1] + protocol::PLAYER_NETWORK_OFFSET, feet[2]]),
            predicted_feet: Some(feet),
            local_equipment: input,
            swing_progress: None,
            renders_game: true,
            hide_hand: false,
            remote_emotes: Vec::new(),
            remote_hats: Vec::new(),
            custom_emote: None,
        },
        |_| {},
        |_, _| (None, None),
        params.get_mut(&mut world).unwrap(),
    );
    super::super::prepare_actor_render_frame(
        super::super::ActorWorld {
            stream: Some(&mut stream),
            collisions: None,
            entity_assets: Some(&entities),
            pack_entities: None,
            session_items: None,
            prepared_actor_artwork: &mut prepared_artwork,
        },
        None,
        |_, _, _, _| false,
        None,
        params.get_mut(&mut world).unwrap(),
    );
    let rig = stream.authority().actor_rig(1).unwrap();
    assert!(rig.hand[1].use_ticks > delay);
    let arm = rig
        .bone_names
        .iter()
        .position(|name| name.as_ref() == "rightarm")
        .unwrap();
    let body = world
        .resource::<super::super::PreparedActorPublication>()
        .submissions()
        .unwrap()
        .iter()
        .find(|draw| draw.input.identity.layer == render_model::ACTOR_LAYER_BODY)
        .unwrap();
    let authored = crate::presentation::actors::convert_bones(rig.current).unwrap();
    let rotation = Quat::from_array(body.input.current_bones[arm].rotation);
    assert!(
        rotation.abs_diff_eq(Quat::from_array(authored[arm].rotation), 1e-4),
        "charged spear must retain its complete authored arm pose"
    );
    // Keep the authored Z sway while checking the spear controller's raised X pose.
    assert!(
        (rotation.to_euler(EulerRot::ZYX).2 - 60.0f32.to_radians()).abs() < 1e-4,
        "charged spear must raise its arm: {:?}",
        body.input.current_bones[arm]
    );
}
