use super::*;
use crate::presentation::actors::{PoseConversions, select_actor_presentations};
use crate::presentation::skin_rig::{MAX_SKIN_RIGS, SkinRigCache};
use bevy::math::Vec3;
use protocol::{
    ActorEvent, ActorKind, ActorSpawnEvent, PlayerListEntry, PlayerListUpdateEvent, WorldEvent,
};
use render_model::{ActorArtworkPages, ActorRigFrameBuilder, ActorRigRoute};
use std::sync::Arc;

/// Original narrow and wide models share one frame-conditional controller.
fn catalog(
    layer_condition: Option<&str>,
    wide_bounds: bool,
    choice: &str,
) -> (Arc<assets::RuntimeEntityAssets>, ActorArtworkPages) {
    let side = render_api::CLASSIC_SKIN_SIDE;
    let model = |identifier, width, origin, size| {
        serde_json::json!({
            "description": {
                "identifier": identifier, "texture_width": side, "texture_height": side,
                "visible_bounds_width": width, "visible_bounds_height": 2,
                "visible_bounds_offset": [0, 1, 0]
            },
            "bones": [{"name": "body", "pivot": [0, 0, 0],
                "cubes": [{"origin": origin, "size": size, "uv": [0, 0]}]}]
        })
    };
    let mut wide = model("geometry.wide", 20, [-80, 0, -4], [160, 8, 8]);
    if !wide_bounds {
        for field in [
            "visible_bounds_width",
            "visible_bounds_height",
            "visible_bounds_offset",
        ] {
            wide["description"].as_object_mut().unwrap().remove(field);
        }
    }
    let mut files = vec![
        (
            "models/entity/fixture.geo.json".into(),
            serde_json::to_vec(&serde_json::json!({
                "format_version": "1.12.0", "minecraft:geometry": [
                    model("geometry.narrow", 2, [-4, 0, -4], [8, 8, 8]),
                    wide
                ]
            }))
            .unwrap(),
        ),
        (
            "render_controllers/fixture.json".into(),
            serde_json::to_vec(&serde_json::json!({
                "format_version": "1.8.0", "render_controllers": {"controller.render.fixture": {
                    "geometry": choice,
                    "materials": [{"*": "Material.default"}], "textures": ["Texture.default"]
                }}
            }))
            .unwrap(),
        ),
    ];
    let controllers = match layer_condition {
        Some(condition) => serde_json::json!([{"controller.render.fixture": condition}]),
        None => serde_json::json!(["controller.render.fixture"]),
    };
    for name in ["player", "creeper"] {
        files.push((
            format!("entity/{name}.json").into(),
            serde_json::to_vec(&serde_json::json!({
                "format_version": "1.10.0", "minecraft:client_entity": {"description": {
                    "identifier": format!("minecraft:{name}"), "materials": {"default": "entity"},
                    "textures": {"default": "textures/entity/fixture"},
                    "geometry": {"default": "geometry.narrow", "wide": "geometry.wide"},
                    "render_controllers": controllers
                }}
            }))
            .unwrap(),
        ));
    }
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(1, 1, image::Rgba([40, 180, 220, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    files.push(("textures/entity/fixture.png".into(), png.into_inner()));
    let compiled = pack_entity_compiler::compile_actor_pack(files)
        .unwrap()
        .unwrap();
    let artwork =
        ActorArtworkPages::default().with_pack_artwork(&compiled.textures, &compiled.bindings);
    let assets = Arc::new(assets::RuntimeEntityAssets::from_compiled(compiled.entities).unwrap());
    (assets, artwork)
}

/// Every roster entry has real distinct pixels and, when requested, distinct parsed model data.
fn skin(id: u64, custom_model: bool) -> protocol::PlayerSkin {
    let side = render_api::CLASSIC_SKIN_SIDE;
    let mut pixels = [40, 180, 220, 255].repeat(side * side);
    pixels[..std::mem::size_of::<u64>()].copy_from_slice(&id.to_le_bytes());
    let geometry = custom_model.then(|| {
        let identifier = format!("geometry.fixture_{id}");
        Arc::new(render_api::SkinGeometrySource {
            resource_patch: serde_json::json!({"geometry": {"default": identifier}}).to_string().into(),
            geometry_data: serde_json::json!({"format_version": "1.12.0", "minecraft:geometry": [{
                "description": {"identifier": identifier, "texture_width": side, "texture_height": side,
                    "visible_bounds_width": 2, "visible_bounds_height": 2, "visible_bounds_offset": [0, 1, 0]},
                "bones": [{"name": "body", "pivot": [0, 0, 0],
                    "cubes": [{"origin": [-4, 0, -4], "size": [8, 8, 8], "uv": [0, 0]}]}]
            }]}).to_string().into(),
            animations: Arc::from([]),
        })
    });
    protocol::PlayerSkin::Standard(render_api::StandardSkin {
        width: side as u32,
        height: side as u32,
        rgba8: pixels.into(),
        cape: None,
        geometry,
    })
}

/// Uses the real pack/roster/spawn/appearance path, leaving the local player out of the scene.
fn new_stream(assets: &Arc<assets::RuntimeEntityAssets>, local_id: u64) -> WorldStream {
    let mut stream = WorldStream::new_with_asset_sets(
        protocol::WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: local_id,
            local_player_unique_id: local_id as i64,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        },
        Arc::new(assets::RuntimeAssets::diagnostic()),
        Arc::clone(assets),
        [0.0; 3],
        None,
    );
    stream.set_pack_entities(Some((
        Arc::clone(assets),
        (0..assets.rig_geometries().len() as u32).collect(),
    )));
    stream
}

/// Publishes real roster appearances before spawning remote players in runtime-id order.
fn player_world(
    assets: &Arc<assets::RuntimeEntityAssets>,
    offscreen: usize,
    skin_of: impl Fn(u64) -> protocol::PlayerSkin,
    visible_feet: [f32; 3],
) -> WorldStream {
    let mut stream = new_stream(assets, offscreen as u64 + 2);
    let visible = offscreen as u64 + 1;
    let entries: Vec<_> = (1..=visible)
        .map(|id| PlayerListEntry::Add {
            uuid: (id as u128).to_le_bytes(),
            unique_id: id as i64,
            username: "fixture".into(),
            verified: true,
            xuid: "".into(),
            skin: skin_of(id),
        })
        .collect();
    stream
        .submit(
            1,
            WorldEvent::Actor(ActorEvent::PlayerList(PlayerListUpdateEvent {
                entries: entries.into(),
            })),
        )
        .unwrap();
    for id in 1..=visible {
        stream
            .submit(
                id + 1,
                WorldEvent::Actor(ActorEvent::Spawn(spawn(
                    id,
                    ActorKind::Player {
                        uuid: (id as u128).to_le_bytes(),
                        username: "fixture".into(),
                    },
                    if id == visible {
                        visible_feet
                    } else {
                        [8.0, 0.0, -5.0]
                    },
                ))),
            )
            .unwrap();
    }
    prepare_player_crowd(&mut stream, visible as usize);
    stream.advance_actor_interpolation_frame(3);
    stream
}

/// Awaits workers and drives bounded appearance publication until the entire crowd is drawable.
fn prepare_player_crowd(stream: &mut WorldStream, expected: usize) {
    for _ in 0..expected {
        stream.prepare_actor_appearance_fixture();
        // Model-free skins also publish over several frames, without pending worker jobs.
        stream.advance_actor_interpolation_frame(0);
        if stream
            .authority()
            .actor_rigs()
            .filter(|rig| rig.pose_evaluated && !rig.render.is_empty())
            .count()
            == expected
        {
            break;
        }
    }
    let rigs: Vec<_> = stream.authority().actor_rigs().collect();
    assert_eq!(
        rigs.len(),
        expected,
        "every roster appearance must be admitted"
    );
    assert!(
        rigs.iter()
            .all(|rig| rig.pose_evaluated && !rig.render.is_empty()),
        "every crowd member must have a completed controller result before capacity sampling"
    );
}

/// Supplies a complete ordinary actor event with deterministic lifetime and transform.
fn spawn(id: u64, kind: ActorKind, position: [f32; 3]) -> ActorSpawnEvent {
    ActorSpawnEvent {
        dimension: 0,
        unique_id: id as i64,
        runtime_id: id,
        kind,
        position,
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
    }
}

/// Looks forward from eye height through a symmetric right-handed camera frustum.
fn view() -> render_model::ActorCullView {
    render_model::ActorCullView {
        camera_position: Vec3::Y,
        clip_from_world: glam::camera::rh::proj::directx::perspective_infinite_reverse(
            90_f32.to_radians(),
            1.0,
            render_api::CAMERA_NEAR_PLANE_BLOCKS,
        ) * glam::camera::rh::view::look_to_mat4(Vec3::Y, -Vec3::Z, Vec3::Y),
        max_distance: render_model::MAX_ACTOR_RENDER_DISTANCE_BLOCKS,
    }
}

/// Samples only remote bodies, leaving local hand and movement ownership untouched.
fn input() -> ActorFrameInput {
    ActorFrameInput {
        cosmetics: Vec::new(),
        local_feed: None,
        predicted_eye: None,
        predicted_feet: None,
        local_equipment: Default::default(),
        swing_progress: None,
        renders_game: true,
        hide_hand: true,
        custom_emote: None,
        remote_emotes: Vec::new(),
        remote_hats: Vec::new(),
    }
}

#[derive(Default)]
struct Caches {
    coverage: Vec<u64>,
    occluded: bool,
    poses: PoseConversions,
    java_poses: java::PoseScratch,
    native_pose: hand::NativePoseCache,
    skin_rigs: SkinRigCache,
    scratch: bodies::BodyScratch,
}

impl Caches {
    /// Calls the production sampler, retaining the same bounded caches across fixture frames.
    fn sample<'a>(
        &mut self,
        stream: &'a WorldStream,
        artwork: &ActorArtworkPages,
        alpha: f32,
    ) -> (
        bodies::SampledBodies<'a>,
        Vec<render_model::ActorRigGeometry>,
    ) {
        self.sample_with_view(stream, artwork, alpha, Some(view()))
    }

    /// Allows the same production sampler to admit a complete crowd without a camera frustum.
    fn sample_with_view<'a>(
        &mut self,
        stream: &'a WorldStream,
        artwork: &ActorArtworkPages,
        alpha: f32,
        cull_view: Option<render_model::ActorCullView>,
    ) -> (
        bodies::SampledBodies<'a>,
        Vec<render_model::ActorRigGeometry>,
    ) {
        self.poses.begin_frame();
        self.skin_rigs.begin_frame();
        let mut geometries = Vec::new();
        let sampled = bodies::sample(
            Some(stream),
            &input(),
            (
                bodies::PoseView {
                    partial_tick: alpha,
                    first_person: true,
                    java_mode: false,
                    combat_visuals: Default::default(),
                    sampling_camera: Some(([0.0; 2], Vec3::Y.to_array())),
                    aspect: 1.0,
                },
                cull_view,
            ),
            artwork,
            bodies::BodyCaches {
                coverage: &self.coverage,
                poses: &mut self.poses,
                java_poses: &mut self.java_poses,
                native_pose: &mut self.native_pose,
                skin_rigs: &mut self.skin_rigs,
                equipment: None,
                scratch: &mut self.scratch,
            },
            |_, _, _, _| self.occluded,
            &mut geometries,
        );
        (sampled, geometries)
    }
}

/// Exercises production body sampling before both bounded model registration and image selection.
fn assert_player_capacity(custom_model: bool, offscreen: usize) {
    let (assets, artwork) = catalog(
        None,
        true,
        "query.frame_alpha > 0.75 ? Geometry.wide : Geometry.default",
    );
    let stream = player_world(
        &assets,
        offscreen,
        |id| skin(id, custom_model),
        [0.0, 0.0, -5.0],
    );
    let visible = offscreen as u64 + 1;
    let mut caches = Caches::default();
    let mut builder = ActorRigFrameBuilder::new(render_model::pack_geometries(&assets)).unwrap();
    for (index, alpha) in [0.5, 1.0].into_iter().enumerate() {
        let mut frame = stream.authority().actor_render_frame(alpha);
        assert!(frame.may_replace_body_geometry(1));
        let selected = frame.layers(1).unwrap();
        assert_eq!(
            selected.len(),
            1,
            "prepared crowd has one selected render layer"
        );
        assert_eq!(selected[0].geometry.is_some(), alpha > 0.75);
        let (sampled, geometries) = caches.sample(&stream, &artwork, alpha);
        let actor = sampled
            .remotes
            .iter()
            .find(|p| p.submission.input.identity.runtime_id == visible)
            .expect("visible high-id player remains eligible");
        assert_eq!(
            actor.submission.route,
            ActorRigRoute::Compiled,
            "offscreen potential replacements must not exhaust the model cache"
        );
        if custom_model {
            assert_eq!(
                geometries.len(),
                usize::from(index == 0),
                "only the visible model is registered, then reused"
            );
        }
        let batch =
            select_actor_presentations(sampled.local_runtime_id, false, None, sampled.remotes);
        let visible_draw = batch
            .submissions
            .iter()
            .find(|draw| draw.input.identity.runtime_id == visible)
            .expect("visible player reaches bounded skin selection");
        assert_eq!(
            visible_draw.route,
            ActorRigRoute::Compiled,
            "offscreen skins must not displace the visible player from residency"
        );
        assert_eq!(batch.skin_layers.len(), 1);
        assert_eq!(batch.submissions.len(), 1);
        assert_eq!(batch.submissions[0].input.identity.runtime_id, visible);
        assert_eq!(batch.submissions[0].route, ActorRigRoute::Compiled);
        builder.insert_geometries(geometries).unwrap();
        let built = builder.build(alpha, None, batch.submissions);
        assert_eq!(built.manifest.len(), 1);
        assert_eq!(built.manifest[0].identity.runtime_id, visible);
    }
}

#[test]
fn offscreen_replacement_players_leave_skin_capacity_for_the_visible_player() {
    assert_player_capacity(false, render_model::MAX_RENDERED_PLAYERS);
}

#[test]
fn offscreen_replacement_players_leave_model_cache_for_the_visible_player() {
    assert_player_capacity(true, MAX_SKIN_RIGS);
}

/// One compiled entity enters the same production admission path as roster-backed bodies.
fn entity_world(assets: &Arc<assets::RuntimeEntityAssets>, feet: [f32; 3]) -> WorldStream {
    let mut stream = new_stream(assets, 2);
    stream
        .submit(
            1,
            WorldEvent::Actor(ActorEvent::Spawn(spawn(
                1,
                ActorKind::Entity {
                    identifier: "minecraft:creeper".into(),
                },
                feet,
            ))),
        )
        .unwrap();
    stream.advance_actor_interpolation_frame(3);
    stream
}

#[test]
fn selected_wide_geometry_is_drawn_when_only_its_base_is_outside() {
    let (assets, artwork) = catalog(
        None,
        true,
        "query.frame_alpha > 0.75 ? Geometry.wide : Geometry.default",
    );
    let stream = entity_world(&assets, [8.0, 0.0, -5.0]);
    let rig = stream.authority().actor_rig(1).unwrap();
    assert!(!render_model::actor_bounds_are_visible(
        [8.0, 0.0, -5.0],
        1.0,
        rig.culling_bounds(),
        Some(view())
    ));
    let (mut sampled, _) = Caches::default().sample(&stream, &artwork, 1.0);
    assert_eq!(sampled.remotes.len(), 1);
    let frame = sampled.render_frame.as_mut().unwrap();
    let geometry = frame.layers(1).unwrap()[0].geometry.unwrap();
    let remaining = frame.remaining_ops();
    let mut batch =
        select_actor_presentations(sampled.local_runtime_id, false, None, sampled.remotes);
    crate::presentation::entity_layers::apply_render_layers_cached(
        &mut batch,
        |id| frame.layers(id),
        &artwork,
        &mut crate::presentation::entity_layers::LayerPoseCache::default(),
    );
    assert_eq!(
        frame.remaining_ops(),
        remaining,
        "drawing reuses admission's frame result"
    );
    assert_eq!(batch.submissions.len(), 1);
    let expected_rig =
        render_model::layer_geometry_rig_id(render_model::EntityRigId(rig.rig.0), geometry);
    assert_eq!(batch.submissions[0].input.rig, expected_rig);
    let mut builder = ActorRigFrameBuilder::new(render_model::pack_geometries(&assets)).unwrap();
    let built = builder.build(1.0, None, batch.submissions);
    assert_eq!(built.manifest.len(), 1);
    assert_eq!(built.manifest[0].identity.runtime_id, 1);
    assert_eq!(built.manifest[0].rig, expected_rig);
}

#[test]
fn selected_geometry_outside_every_box_is_rejected_before_submission() {
    let (assets, artwork) = catalog(
        None,
        true,
        "query.frame_alpha > 0.75 ? Geometry.wide : Geometry.default",
    );
    let stream = entity_world(&assets, [40.0, 0.0, -5.0]);
    for alpha in [0.5, 1.0] {
        let (sampled, geometries) = Caches::default().sample(&stream, &artwork, alpha);
        assert!(sampled.remotes.is_empty());
        assert!(geometries.is_empty());
        let batch =
            select_actor_presentations(sampled.local_runtime_id, false, None, sampled.remotes);
        let mut builder =
            ActorRigFrameBuilder::new(render_model::pack_geometries(&assets)).unwrap();
        let built = builder.build(alpha, None, batch.submissions);
        assert!(built.manifest.is_empty());
    }
}

#[test]
fn selected_geometry_keeps_empty_layer_base_fallback_and_unknown_bounds() {
    let (assets, artwork) = catalog(Some("query.frame_alpha > 0.75"), true, "Geometry.wide");
    let stream = entity_world(&assets, [0.0, 0.0, -5.0]);
    let (mut sampled, _) = Caches::default().sample(&stream, &artwork, 0.5);
    assert!(
        sampled
            .render_frame
            .as_mut()
            .unwrap()
            .layers(1)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        sampled.remotes.len(),
        1,
        "empty layers retain the visible base body"
    );
    let base = sampled.remotes[0].submission.input.rig;
    let mut batch =
        select_actor_presentations(sampled.local_runtime_id, false, None, sampled.remotes);
    crate::presentation::entity_layers::apply_render_layers_cached(
        &mut batch,
        |id| sampled.render_frame.as_mut().unwrap().layers(id),
        &artwork,
        &mut crate::presentation::entity_layers::LayerPoseCache::default(),
    );
    assert_eq!(batch.submissions[0].input.rig, base);

    let (assets, artwork) = catalog(None, false, "Geometry.wide");
    let stream = entity_world(&assets, [40.0, 0.0, -5.0]);
    let (sampled, _) = Caches::default().sample(&stream, &artwork, 1.0);
    assert_eq!(
        sampled.remotes.len(),
        1,
        "unknown chosen bounds cannot prove exclusion"
    );
}

/// Gives the animated face an independent box that extends beyond the narrow body.
fn persona_skin() -> protocol::PlayerSkin {
    let protocol::PlayerSkin::Standard(mut skin) = skin(1, true) else {
        unreachable!()
    };
    let source = Arc::make_mut(skin.geometry.as_mut().unwrap());
    let mut patch: serde_json::Value = serde_json::from_str(&source.resource_patch).unwrap();
    patch["geometry"]["animated_face"] = "geometry.face".into();
    source.resource_patch = patch.to_string().into();
    let mut data: serde_json::Value = serde_json::from_str(&source.geometry_data).unwrap();
    let mut face = data["minecraft:geometry"][0].clone();
    face["description"]["identifier"] = "geometry.face".into();
    face["description"]["visible_bounds_width"] = 20.into();
    face["bones"][0]["cubes"][0]["origin"] = serde_json::json!([-80, 0, -4]);
    face["bones"][0]["cubes"][0]["size"] = serde_json::json!([160, 8, 8]);
    data["minecraft:geometry"]
        .as_array_mut()
        .unwrap()
        .push(face);
    source.geometry_data = data.to_string().into();
    let side = render_api::CLASSIC_SKIN_SIDE as u32;
    source.animations = Arc::from([render_api::SkinAnimation {
        kind: render_api::SkinAnimationKind::Face,
        width: side,
        height: side,
        rgba8: vec![255; side as usize * side as usize * 4].into(),
        frames: 1,
        blinking: false,
    }]);
    protocol::PlayerSkin::Standard(skin)
}

#[test]
fn selected_persona_geometry_is_admitted_before_skin_registration() {
    let (assets, artwork) = catalog(None, true, "Geometry.default");
    for (feet, visible) in [([8.0, 0.0, -5.0], true), ([40.0, 0.0, -5.0], false)] {
        let stream = player_world(&assets, 0, |_| persona_skin(), feet);
        let rig = stream.authority().actor_rig(1).unwrap();
        assert_eq!(rig.skin_layers.len(), 1);
        assert!(!render_model::actor_bounds_are_visible(
            feet,
            1.0,
            rig.culling_bounds(),
            Some(view())
        ));
        assert!(
            !stream
                .authority()
                .actor_render_frame(1.0)
                .may_replace_body_geometry(1)
        );
        let mut caches = Caches::default();
        let (sampled, mut geometries) = caches.sample(&stream, &artwork, 1.0);
        assert_eq!(sampled.remotes.len(), usize::from(visible));
        assert_eq!(geometries.len(), usize::from(visible));
        if !visible {
            continue;
        }
        let mut batch =
            select_actor_presentations(sampled.local_runtime_id, false, None, sampled.remotes);
        let mut layers = crate::presentation::skin_layers::SkinLayerCache::default();
        let _ = layers.apply(
            &mut batch,
            &artwork,
            |id| {
                let rig = stream.authority().actor_rig(id)?;
                Some(emote_geometry::skin_layer_snapshot(
                    rig,
                    None,
                    None,
                    sampled.remote_skin_layers.get(&id).map(AsRef::as_ref),
                ))
            },
            &mut caches.skin_rigs,
            |geometry| geometries.push(geometry),
        );
        let persona = batch
            .submissions
            .iter()
            .find(|s| crate::presentation::skin_layers::is_skin_layer(s.input.identity.layer))
            .expect("the wide persona reaches its real draw path");
        let persona_identity = persona.input.identity;
        geometries.extend(render_model::pack_geometries(&assets));
        let mut builder = ActorRigFrameBuilder::new(geometries).unwrap();
        let built = builder.build(1.0, None, batch.submissions);
        assert!(
            built
                .manifest
                .iter()
                .any(|draw| draw.identity == persona_identity)
        );
    }
}

#[test]
fn remote_body_batches_keep_serial_order_layers_and_budget() {
    let (assets, artwork) = catalog(Some("query.frame_alpha < 0.75"), true, "Geometry.default");
    let serial = player_world(&assets, 127, |id| skin(id, false), [0.0, 0.0, -5.0]);
    let parallel = player_world(&assets, 127, |id| skin(id, false), [0.0, 0.0, -5.0]);
    let mut caches = Caches::default();
    let mut conversions = PoseConversions::default();
    for alpha in [0.0, 0.25, 0.5, 0.75, 1.0, 0.25] {
        let mut frame = serial.authority().actor_render_frame(alpha);
        let mut expected = Vec::new();
        conversions.begin_frame();
        for rig in serial.authority().actor_rigs() {
            let actor = serial.authority().actor(rig.actor.runtime_id).unwrap();
            let mut body = crate::presentation::actors::actor_rig_presentation_cached(
                &rig,
                actor,
                serial
                    .authority()
                    .actor_player_profile(rig.actor.runtime_id),
                alpha,
                &mut conversions,
            )
            .unwrap();
            hand::NativePoseCache::apply_frame_body(&mut frame, &mut body);
            expected.push((
                body.submission.input.identity.runtime_id,
                body.submission.input.previous_bones,
                body.submission.input.current_bones,
            ));
        }
        let (mut sampled, _) = caches.sample_with_view(&parallel, &artwork, alpha, None);
        let actual: Vec<_> = sampled
            .remotes
            .iter()
            .map(|body| {
                (
                    body.submission.input.identity.runtime_id,
                    Arc::clone(&body.submission.input.previous_bones),
                    Arc::clone(&body.submission.input.current_bones),
                )
            })
            .collect();
        assert_eq!(actual, expected, "alpha={alpha}");
        for (id, _, _) in &expected {
            assert_eq!(
                sampled
                    .render_frame
                    .as_mut()
                    .unwrap()
                    .layers_with_skin(*id)
                    .unwrap()
                    .render,
                frame.layers_with_skin(*id).unwrap().render,
                "layers for actor {id}, alpha={alpha}"
            );
        }
        assert_eq!(
            sampled.render_frame.as_ref().unwrap().remaining_ops(),
            frame.remaining_ops()
        );
        caches
            .scratch
            .recycle(sampled.java_posed, sampled.remote_skin_layers);
    }
}

#[test]
fn translated_coverage_prepares_remote_sources_outside_the_live_view_or_behind_terrain() {
    for (feet, occluded) in [
        ([0., 0., 5.], false),
        ([8., 0., -5.], false),
        ([500., 0., -5.], false),
        ([0., 0., -5.], true),
    ] {
        let (assets, artwork) = catalog(None, true, "Geometry.default");
        let stream = player_world(&assets, 0, |id| skin(id, false), feet);
        let mut caches = Caches {
            occluded,
            ..Default::default()
        };
        assert!(caches.sample(&stream, &artwork, 0.5).0.remotes.is_empty());
        caches.coverage.push(1);
        let sampled = caches.sample(&stream, &artwork, 0.5).0;
        assert_eq!(sampled.remotes.len(), 1, "source at {feet:?}");
        assert_eq!(sampled.mask_only_runtime_ids, [1]);
    }
}
