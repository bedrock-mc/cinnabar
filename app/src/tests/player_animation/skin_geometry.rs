//! Skin models, capes, and animated layers share the player animation state.
use super::*;
use render_model::{ACTOR_LAYER_BODY, ActorRigRejects, ActorRigRoute};
use render_scene::ActorRenderScene;

// Players draw from their own skin, never pack artwork: the body samples its skin layer and the
// cape samples the layer appended after it.
#[test]
fn skinned_player_publishes_a_drawable_body_and_cape_on_the_skin_page() {
    use client_presentation::presentation::{actors, cape};
    let entities = entities();
    let mut world = stream(Arc::clone(&entities));
    world.submit(1, skinned_player_list(200, 90)).unwrap();
    world.submit(2, spawn_player()).unwrap();
    world.prepare_actor_appearance_fixture();
    world.advance_actor_interpolation_ticks(2);
    let rig = world.authority().actor_rig(42).unwrap();
    let body = actors::actor_rig_presentation(
        &rig,
        world.authority().actor(42).unwrap(),
        world.authority().actor_player_profile(42),
        0.5,
    )
    .unwrap();
    let mut batch = actors::select_actor_presentations(1, false, None, [body]);
    let mut scene = ActorRenderScene::with_runtime_entity_assets(&entities).unwrap();
    let mut capes = cape::CapeState::default();
    let (cape_rig, preparation) = capes.rig(Some(&entities)).expect("cape geometry resolves");
    let PlayerSkin::Standard(skin) = &world.authority().actor_player_profile(42).unwrap().skin
    else {
        panic!("standard fixture skin");
    };
    preparation.request(skin.cape.as_ref().expect("fixture cape"));
    preparation.submit();
    preparation.finish_fixture_batch();
    scene.insert_geometry(cape_rig.geometry.clone()).unwrap();
    cape::apply_capes(
        &mut batch,
        cape_rig,
        preparation,
        |runtime_id| world.authority().actor_rig(runtime_id),
        |runtime_id| world.authority().actor_player_profile(runtime_id),
        |_| None,
        |_| false,
    );
    let frame = actors::update_actor_rig_scene(&mut scene, 0.5, batch);
    assert_eq!(frame.rig.rejects, ActorRigRejects::default());
    let layers = frame
        .rig
        .manifest
        .iter()
        .zip(frame.rig.instances.iter())
        .map(|(entry, instance)| {
            let pixels = frame
                .player_skin(instance.texture_layer)
                .expect("resident skin");
            assert_eq!(pixels.len(), STANDARD_SKIN_BYTES);
            (
                entry.identity.layer,
                entry.route,
                pixels[0],
                pixels.iter().all(|byte| *byte == pixels[0]),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        layers,
        [
            (ACTOR_LAYER_BODY, ActorRigRoute::Compiled, 200, true),
            (cape::ACTOR_LAYER_CAPE, ActorRigRoute::Compiled, 90, true),
        ]
    );
}

const NPC_PATCH: &str = r#"{"geometry":{"default":"geometry.npc"}}"#;
const NPC_GEOMETRY: &str = r#"{"format_version":"1.12.0","minecraft:geometry":[{
 "description":{"identifier":"geometry.npc","texture_width":128,"texture_height":128},
 "bones":[
  {"name":"root","pivot":[0,0,0]},
  {"name":"body","parent":"root","pivot":[0,24,0],"cubes":[{"origin":[-6,10,-3],"size":[12,14,6],"uv":[0,0]}]},
  {"name":"head","parent":"body","pivot":[0,24,0],"cubes":[{"origin":[-5,24,-5],"size":[10,10,10],"uv":[0,40]}]},
  {"name":"tail","parent":"body","pivot":[0,12,3],"META_BoneType":"base","cubes":[{"origin":[-1,10,3],"size":[2,2,8],"uv":{"north":{"uv":[64,0],"uv_size":[2,2]}}}]}]}]}"#;

// A skin carrying its own model is drawn with it: the player's animations drive its bones by
// name and the skin's own rig geometry reaches the frame.
#[test]
fn skin_geometry_replaces_the_default_model_and_keeps_the_player_animations() {
    use client_presentation::presentation::{actors, skin_rig::SkinRigCache};
    let entities = entities();
    let mut world = stream(Arc::clone(&entities));
    world
        .submit(
            1,
            player_list_with(200, None, Some((NPC_PATCH, NPC_GEOMETRY))),
        )
        .unwrap();
    world.submit(2, spawn_player()).unwrap();
    world.submit(3, move_player(0.0, 0.0, 30.0, 1)).unwrap();
    world.prepare_actor_appearance_fixture();
    world.advance_actor_interpolation_ticks(4);
    let rig = world.authority().actor_rig(42).unwrap();
    let geometry = rig.skin_geometry.expect("the skin model resolves").clone();
    assert_eq!(
        rig.bone_names,
        ["root", "body", "head", "tail"].map(Box::<str>::from)
    );
    assert_eq!(rig.current.len(), 4);
    assert!(
        turned(rig.current[2]),
        "the look animation turns the skin's head"
    );
    assert!(
        !turned(rig.current[3]),
        "a bone the player rig lacks stays at rest"
    );
    let mut presentation = actors::actor_rig_presentation(
        &rig,
        world.authority().actor(42).unwrap(),
        world.authority().actor_player_profile(42),
        0.5,
    )
    .unwrap();
    let mut scene = ActorRenderScene::with_runtime_entity_assets(&entities).unwrap();
    let mut cache = SkinRigCache::default();
    cache.begin_frame();
    let id = cache
        .rig(&geometry, rig.skin_mesh, |built| {
            scene.insert_geometry(built).unwrap()
        })
        .unwrap();
    assert_eq!(
        cache.rig(&geometry, rig.skin_mesh, |_| panic!("the model is cached")),
        Some(id)
    );
    presentation.submission.input.rig = id;
    let batch = actors::select_actor_presentations(1, false, None, [presentation]);
    let frame = actors::update_actor_rig_scene(&mut scene, 0.5, batch);
    assert_eq!(frame.rig.rejects, ActorRigRejects::default());
    assert_eq!(frame.rig.manifest[0].rig, id);
    assert_eq!(frame.rig.manifest[0].bone_count, 4);
}

// An unusable skin model falls back to the default geometry instead of hiding the player.
#[test]
fn malformed_skin_geometry_falls_back_to_the_default_model() {
    let entities = entities();
    let mut world = stream(Arc::clone(&entities));
    world
        .submit(
            1,
            player_list_with(200, None, Some((NPC_PATCH, "{not json"))),
        )
        .unwrap();
    world.submit(2, spawn_player()).unwrap();
    world.prepare_actor_appearance_fixture();
    world.advance_actor_interpolation_ticks(2);
    let rig = world.authority().actor_rig(42).unwrap();
    assert!(rig.skin_geometry.is_none());
    assert_eq!(rig.bone_names.len(), 7);
    assert_eq!(
        world
            .authority()
            .actor_animation_stats()
            .invalid_skin_geometries,
        1
    );
}

#[test]
fn animated_skin_uses_its_own_rectangular_texture_geometry_and_uv_frame() {
    use client_presentation::presentation::{actors, skin_layers, skin_rig};
    let patch =
        r#"{"geometry":{"default":"geometry.humanoid.custom","animated_32x32":"geometry.cape"}}"#;
    let WorldEvent::Actor(ActorEvent::PlayerList(mut update)) =
        player_list_with(200, None, Some((patch, GEOMETRY)))
    else {
        panic!("player list fixture");
    };
    let PlayerListEntry::Add {
        skin: PlayerSkin::Standard(skin),
        ..
    } = &mut Arc::make_mut(&mut update.entries)[0]
    else {
        panic!("standard skin fixture");
    };
    Arc::make_mut(skin.geometry.as_mut().unwrap()).animations =
        Arc::from([render_api::SkinAnimation {
            kind: render_api::SkinAnimationKind::Body32,
            width: 24,
            height: 512,
            rgba8: vec![255; 24 * 512 * 4].into(),
            frames: 16,
            blinking: false,
        }]);
    let entities = entities();
    let mut world = stream(Arc::clone(&entities));
    world
        .submit(1, WorldEvent::Actor(ActorEvent::PlayerList(update)))
        .unwrap();
    world.submit(2, spawn_player()).unwrap();
    world.prepare_actor_appearance_fixture();
    world.advance_actor_interpolation_ticks(4);
    let rig = world.authority().actor_rig(42).unwrap();
    assert_eq!(rig.skin_layers.len(), 1);
    let body = actors::actor_rig_presentation(
        &rig,
        world.authority().actor(42).unwrap(),
        world.authority().actor_player_profile(42),
        0.5,
    )
    .unwrap();
    let mut batch = actors::select_actor_presentations(1, false, None, [body]);
    let mut scene = ActorRenderScene::with_runtime_entity_assets(&entities).unwrap();
    let mut layers = skin_layers::SkinLayerCache::default();
    let mut geometries = Vec::new();
    let artwork = layers
        .apply(
            &mut batch,
            &Default::default(),
            |id| world.authority().actor_rig(id),
            &mut skin_rig::SkinRigCache::default(),
            |geometry| geometries.push(geometry),
        )
        .unwrap();
    assert_eq!(artwork.pages()[0].dimensions(), (24, 512));
    for geometry in geometries {
        scene.insert_geometry(geometry).unwrap();
    }
    scene.configure_artwork(artwork);
    let layer = &batch.submissions[1];
    assert_eq!(layer.uv_anim, rig.skin_layers[0].uv_anim);
    assert_eq!(
        layer.input.current_bones.len(),
        rig.skin_layers[0].current.len()
    );
    assert_eq!(
        layer.world_from_actor,
        batch.submissions[0].world_from_actor
    );
    let frame = actors::update_actor_rig_scene(&mut scene, 0.5, batch);
    assert_eq!(frame.rig.rejects, ActorRigRejects::default());
    assert_eq!(frame.rig.manifest.len(), 2);
    assert_eq!(
        frame.rig.manifest[1].identity.layer,
        render_model::ACTOR_LAYER_SKIN_BASE + render_api::SkinAnimationKind::Body32.slot() as u8
    );
    assert_eq!(frame.rig.manifest[1].bone_count, 2);
    assert_ne!(frame.instance_pages()[1], 0);
}

/// Replays the pinned carrier's real blink controller into the animated face UV frame.
#[test]
fn persona_face_blinks_with_the_pinned_runtime_controller() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let path = root.join(assets::carriers::startup::DEFAULT_ASSET_PATH);
    for fixture in [
        path.clone(),
        assets::carriers::startup::entity_asset_path(&path),
        assets::carriers::startup::atmosphere_asset_path(&path),
        path.with_file_name(assets::carriers::ITEM_FACTS.output),
    ] {
        if !fixture.is_file() {
            eprintln!(
                "skipping persona_face_blinks_with_the_pinned_runtime_controller: missing pinned runtime fixture {}",
                fixture.display()
            );
            return;
        }
    }
    let loaded =
        assets::carriers::startup::load_runtime_assets(assets::carriers::startup::AssetSelection {
            path,
            source: assets::carriers::startup::AssetPathSource::CommandLine,
        })
        .unwrap();
    let entities = Arc::clone(loaded.entities.runtime());
    assert!(entities.controllers().iter().any(|controller| {
        entities.symbols()[controller.symbol as usize]
            .identifier
            .as_ref()
            == "controller.animation.persona.blink"
    }));
    let patch =
        r#"{"geometry":{"default":"geometry.humanoid.custom","animated_face":"geometry.cape"}}"#;
    let WorldEvent::Actor(ActorEvent::PlayerList(mut update)) =
        player_list_with(200, None, Some((patch, GEOMETRY)))
    else {
        panic!("player list fixture");
    };
    let PlayerListEntry::Add {
        skin: PlayerSkin::Standard(skin),
        ..
    } = &mut Arc::make_mut(&mut update.entries)[0]
    else {
        panic!("standard skin fixture");
    };
    Arc::make_mut(skin.geometry.as_mut().unwrap()).animations =
        Arc::from([render_api::SkinAnimation {
            kind: render_api::SkinAnimationKind::Face,
            width: 32,
            height: 64,
            rgba8: vec![255; 32 * 64 * 4].into(),
            frames: 2,
            blinking: true,
        }]);
    let mut world = stream(entities);
    world
        .submit(1, WorldEvent::Actor(ActorEvent::PlayerList(update)))
        .unwrap();
    world.submit(2, spawn_player()).unwrap();
    let mut opened = false;
    let mut closed = false;
    for _ in 0..800 {
        world.prepare_actor_appearance_fixture();
        world.advance_actor_interpolation_ticks(1);
        let rig = world.authority().actor_rig(42).unwrap();
        assert_eq!(rig.skin_layers.len(), 1);
        let offset = rig.skin_layers[0].uv_anim[1];
        opened |= offset == 0.0;
        closed |= offset == 0.5;
        if opened && closed {
            break;
        }
    }
    assert!(opened && closed, "face must publish both blink frames");
}

#[test]
fn selected_custom_local_model_shares_preparation_in_first_and_third_person() {
    let entities = entities();
    let mut world = stream(Arc::clone(&entities));
    let model = serde_json::json!({"format_version":"1.12.0","minecraft:geometry":[{
        "description":{"identifier":"geometry.local.fixture","texture_width":64,"texture_height":64},
        "bones":[
            {"name":"root"},
            {"name":"body","parent":"root","cubes":[{"origin":[-12,0,-4],"size":[24,8,8],"uv":[0,0]}]},
            {"name":"rightArm","parent":"body","pivot":[-12,8,0],"cubes":[{"origin":[-20,0,-2],"size":[8,8,4],"uv":[0,20]}]}
        ]
    }]}).to_string();
    let geometry = launcher::skin_import::parse_skin_geometry(model.as_bytes(), None).unwrap();
    let skin = StandardSkin {
        width: render_api::CLASSIC_SKIN_SIDE as u32,
        height: render_api::CLASSIC_SKIN_SIDE as u32,
        rgba8: vec![200; render_api::CLASSIC_SKIN_SIDE * render_api::CLASSIC_SKIN_SIDE * 4].into(),
        cape: None,
        geometry: Some(geometry.clone()),
    };
    let mut local = client_shell::player_skin::LocalPlayerSkin::generated_default("fixture");
    local.set_selection(&skin, launcher::dressing_room::SkinModel::Custom);
    assert_eq!(
        local.to_client_skin().geometry.unwrap().geometry_data,
        model
    );
    world
        .submit(
            1,
            player_list_with(
                200,
                None,
                Some((&geometry.resource_patch, &geometry.geometry_data)),
            ),
        )
        .unwrap();
    world.submit(2, spawn_player()).unwrap();
    let mut feed = LocalPlayerFeed {
        skin: local.player_skin(),
        ..local_feed(None)
    };
    for first_person in [false, true] {
        feed.first_person = first_person;
        world.sync_local_player_pose(&feed);
        world.prepare_actor_appearance_fixture();
        world.advance_actor_interpolation_ticks(1);
        let local = world.authority().actor_rig(1).unwrap();
        let remote = world.authority().actor_rig(42).unwrap();
        assert_eq!(
            &*local.skin_geometry.unwrap().identifier,
            "geometry.local.fixture"
        );
        assert!(
            local
                .bone_names
                .iter()
                .any(|name| name.eq_ignore_ascii_case("rightArm"))
        );
        assert!(
            Arc::ptr_eq(local.skin_geometry.unwrap(), remote.skin_geometry.unwrap()),
            "both camera modes reuse the remote worker's model cache"
        );
        assert!(Arc::ptr_eq(
            &local.skin_mesh.unwrap().vertices,
            &remote.skin_mesh.unwrap().vertices
        ));
        let mut cache = client_presentation::presentation::skin_rig::SkinRigCache::default();
        cache.begin_frame();
        let mut registered = Vec::new();
        let local_id = cache
            .rig(local.skin_geometry.unwrap(), local.skin_mesh, |mesh| {
                registered.push(mesh)
            })
            .unwrap();
        let remote_id = cache
            .rig(remote.skin_geometry.unwrap(), remote.skin_mesh, |mesh| {
                registered.push(mesh)
            })
            .unwrap();
        assert_eq!(local_id, remote_id);
        assert_eq!(registered.len(), 1);
    }
}
